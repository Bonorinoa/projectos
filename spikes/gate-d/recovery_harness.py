from __future__ import annotations

import copy
import errno
import hashlib
import json
import os
import shutil
import tempfile
import uuid
import fcntl
import tomllib
import re
import stat
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Callable


CURRENT_SCHEMA = 3


class InjectedCrash(RuntimeError):
    pass


class Conflict(RuntimeError):
    pass


class InvalidGraph(Conflict):
    pass


class SecondWriter(RuntimeError):
    pass


def canonical(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def snapshot_envelope(state: dict[str, Any], sequence: int) -> dict[str, Any]:
    envelope = {"graph": state, "sequence": sequence, "snapshot_version": 1}
    return {**envelope, "snapshot_hash": digest(canonical(envelope))}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def validate_state(state: dict[str, Any]) -> None:
    try:
        _validate_state(state)
    except (KeyError, TypeError, AttributeError, ValueError, Conflict) as exc:
        raise InvalidGraph(f"invalid graph: {exc}") from exc


def _validate_state(state: dict[str, Any]) -> None:
    """Model invariants checked independently of serialized-byte checksums."""
    if state.get("schema_version") not in {1, 2, CURRENT_SCHEMA} or state.get("export_version") != 1:
        raise Conflict("unsupported graph/export schema")
    if not isinstance(state.get("project_id"), str) or not state["project_id"]:
        raise Conflict("invalid project identity")
    if not isinstance(state.get("lineage_id"), str) or not state["lineage_id"]:
        raise Conflict("invalid lineage identity")
    if not isinstance(state.get("mutations"), dict):
        raise Conflict("invalid mutation collection")
    for mutation_id, mutation in state["mutations"].items():
        if mutation["id"] != mutation_id or mutation["state"] not in {
                "PREPARED", "FILES_APPLIED", "COMMITTED", "ROLLED_BACK", "EXTERNAL_CONFLICT"}:
            raise Conflict("invalid mutation identity/state")
        safe_relpath(mutation["path"])
        for key in ["before_hash", "after_hash"]:
            if not re.fullmatch(r"[0-9a-f]{64}", mutation[key]):
                raise Conflict("invalid mutation hash")
    for key in ["revisions", "receipts", "derived_chunks"]:
        if not isinstance(state.get(key), list):
            raise Conflict(f"invalid {key} collection")
    revision_ids = [revision["mutation_id"] for revision in state["revisions"] if "mutation_id" in revision]
    if len(revision_ids) != len(set(revision_ids)):
        raise Conflict("duplicate revision identity")
    for mutation_id, mutation in state["mutations"].items():
        if mutation["state"] == "COMMITTED" and mutation_id not in revision_ids:
            raise Conflict("committed mutation has no revision")


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def fsync_dir(path: Path) -> None:
    fd = os.open(path, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def atomic_bytes(path: Path, data: bytes, fail: Callable[[str], None] | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    try:
        with tmp.open("wb") as handle:
            handle.write(data)
            handle.flush()
            if fail:
                fail("before_fsync")
            os.fsync(handle.fileno())
            if fail:
                fail("after_fsync")
        if fail:
            fail("before_swap")
        os.replace(tmp, path)
        if fail:
            fail("after_swap")
        fsync_dir(path.parent)
    finally:
        tmp.unlink(missing_ok=True)


def safe_relpath(raw: str) -> str:
    path = PurePosixPath(raw)
    if path.is_absolute() or ".." in path.parts or not path.parts or "\\" in raw or "\x00" in raw:
        raise ValueError("unsafe project-relative path")
    return path.as_posix()


class ScopedFS:
    """POSIX descriptor-relative I/O, pinned to the opened project directory."""

    def __init__(self, root: Path):
        self.root = root
        self.fd = None
        self.fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)

    def close(self) -> None:
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None

    def __del__(self):
        self.close()

    def relative(self, path: Path) -> str:
        return safe_relpath(path.relative_to(self.root).as_posix())

    @staticmethod
    def check_alias(fd: int, name: str) -> None:
        if any(entry != name and entry.casefold() == name.casefold() for entry in os.listdir(fd)):
            raise Conflict("case-insensitive alias rejected")

    @contextmanager
    def parent(self, path: Path, create: bool = False):
        parts = PurePosixPath(self.relative(path)).parts
        fd = os.dup(self.fd)
        try:
            for component in parts[:-1]:
                self.check_alias(fd, component)
                if create:
                    try:
                        os.mkdir(component, 0o700, dir_fd=fd)
                    except FileExistsError:
                        pass
                try:
                    next_fd = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
                except OSError as exc:
                    if exc.errno in {errno.ELOOP, errno.ENOTDIR}:
                        raise Conflict("symlink/non-directory ancestor rejected") from exc
                    raise
                os.close(fd)
                fd = next_fd
            self.check_alias(fd, parts[-1])
            yield fd, parts[-1]
        finally:
            os.close(fd)

    @staticmethod
    def open_at(fd: int, name: str, flags: int, mode: int = 0o600) -> int:
        try:
            # Validate the opened inode before truncation. Passing O_TRUNC to
            # open itself would damage a hard-linked external file first.
            result = os.open(name, (flags & ~os.O_TRUNC) | os.O_NOFOLLOW | os.O_NONBLOCK, mode, dir_fd=fd)
        except OSError as exc:
            if exc.errno == errno.ELOOP:
                raise Conflict("symlink leaf rejected") from exc
            raise
        metadata = os.fstat(result)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            os.close(result)
            raise Conflict("non-regular or hard-linked file rejected")
        if flags & os.O_TRUNC:
            os.ftruncate(result, 0)
        return result

    @staticmethod
    def read_at(fd: int, name: str) -> bytes:
        leaf = ScopedFS.open_at(fd, name, os.O_RDONLY)
        with os.fdopen(leaf, "rb") as handle:
            return handle.read()

    def read(self, path: Path) -> bytes:
        with self.parent(path) as (fd, name):
            return self.read_at(fd, name)

    def exists(self, path: Path) -> bool:
        try:
            with self.parent(path) as (fd, name):
                metadata = os.stat(name, dir_fd=fd, follow_symlinks=False)
                if stat.S_ISLNK(metadata.st_mode):
                    raise Conflict("symlink leaf rejected")
                return True
        except FileNotFoundError:
            return False

    @contextmanager
    def open(self, path: Path, flags: int, mode: str):
        with self.parent(path, create=bool(flags & os.O_CREAT)) as (fd, name):
            leaf = self.open_at(fd, name, flags)
            with os.fdopen(leaf, mode) as handle:
                yield handle

    def atomic(self, path: Path, data: bytes, fail: Callable[[str], None] | None = None,
               no_replace: bool = False) -> None:
        with self.parent(path, create=True) as (fd, name):
            stage = f".{name}.{uuid.uuid4().hex}.tmp"
            leaf = self.open_at(fd, stage, os.O_WRONLY | os.O_CREAT | os.O_EXCL)
            try:
                with os.fdopen(leaf, "wb") as handle:
                    handle.write(data)
                    handle.flush()
                    if fail:
                        fail("before_fsync")
                    os.fsync(handle.fileno())
                    if fail:
                        fail("after_fsync")
                if fail:
                    fail("before_swap")
                # Destination replacement never follows a swapped symlink.
                self.check_alias(fd, name)
                if self.exists_at(fd, name):
                    metadata = os.stat(name, dir_fd=fd, follow_symlinks=False)
                    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                        raise Conflict("non-regular/hard-linked destination rejected")
                if no_replace:
                    try:
                        os.link(stage, name, src_dir_fd=fd, dst_dir_fd=fd, follow_symlinks=False)
                    except FileExistsError as exc:
                        raise Conflict("restore destination appeared concurrently") from exc
                    os.unlink(stage, dir_fd=fd)
                else:
                    os.replace(stage, name, src_dir_fd=fd, dst_dir_fd=fd)
                if self.read_at(fd, name) != data:
                    raise Conflict("atomic install verification failed")
                if fail:
                    fail("after_swap")
                os.fsync(fd)
            finally:
                try:
                    os.unlink(stage, dir_fd=fd)
                except FileNotFoundError:
                    pass

    @staticmethod
    def exists_at(fd: int, name: str) -> bool:
        try:
            metadata = os.stat(name, dir_fd=fd, follow_symlinks=False)
        except FileNotFoundError:
            return False
        if stat.S_ISLNK(metadata.st_mode):
            raise Conflict("symlink destination rejected")
        return True

    def replace(self, source: Path, destination: Path) -> None:
        with self.parent(source) as (source_fd, source_name), self.parent(destination) as (dest_fd, dest_name):
            source_content = self.read_at(source_fd, source_name)
            if self.exists_at(dest_fd, dest_name):
                leaf = self.open_at(dest_fd, dest_name, os.O_RDONLY)
                os.close(leaf)
            os.replace(source_name, dest_name, src_dir_fd=source_fd, dst_dir_fd=dest_fd)
            if self.read_at(dest_fd, dest_name) != source_content:
                raise Conflict("descriptor replacement verification failed")
            os.fsync(dest_fd)

    def unlink(self, path: Path) -> None:
        try:
            with self.parent(path) as (fd, name):
                os.unlink(name, dir_fd=fd)
        except FileNotFoundError:
            pass

    def sync_parent(self, path: Path) -> None:
        with self.parent(path) as (fd, _):
            os.fsync(fd)

    def names(self, directory: Path) -> list[str]:
        # parent traversal pins the requested directory even after a rename.
        try:
            with self.parent(directory / "placeholder") as (fd, _):
                return sorted(os.listdir(fd))
        except FileNotFoundError:
            return []


@dataclass
class FaultPlan:
    point: str | None = None

    def hit(self, point: str) -> None:
        if self.point == point:
            if point == "disk_full":
                raise OSError(errno.ENOSPC, "injected disk full")
            if point == "permission_loss":
                raise PermissionError(errno.EACCES, "injected permission loss")
            raise InjectedCrash(point)


class RecoveryHarness:
    """Deterministic contract model, not a production storage engine."""

    def __init__(self, root: Path, project_id: str = "project-0001", create: bool = True):
        self.root = root
        self.sidecar = root / ".projectos"
        self.graph_path = self.sidecar / "graph" / "store" / "state.json"
        self.objects = self.sidecar / "objects"
        self.recovery = self.sidecar / "recovery"
        self.journal = self.recovery / "journal.jsonl"
        self.snapshots = self.recovery / "snapshots"
        self.lock_path = self.sidecar / "writer.lock"
        if create:
            self.root.mkdir(parents=True, exist_ok=True)
        self.fs = ScopedFS(root)
        if create:
            self._create(project_id)

    def _create(self, project_id: str) -> None:
        self.fs.atomic(self.root / "artifact.md", b"before\n")
        state = {
            "schema_version": CURRENT_SCHEMA,
            "export_version": 1,
            "project_id": project_id,
            "lineage_id": "lineage-main",
            "mutations": {},
            "revisions": [],
            "receipts": [],
            "derived_chunks": [],
            "checkpoint": None,
        }
        self.fs.atomic(self.graph_path, canonical(state))
        self.fs.atomic(
            self.sidecar / "project.toml",
            f'project_id = {json.dumps(project_id)}\nschema_version = {CURRENT_SCHEMA}\n'.encode(),
        )

    def state(self) -> dict[str, Any]:
        state = json.loads(self.fs.read(self.graph_path))
        validate_state(state)
        envelope = tomllib.loads(self.fs.read(self.sidecar / "project.toml").decode())
        if state["project_id"] != envelope["project_id"]:
            raise Conflict("manifest/graph UUID mismatch; read-only repair required")
        return state

    def _save_state(self, state: dict[str, Any], fault: FaultPlan | None = None) -> None:
        validate_state(state)
        self.fs.atomic(
            self.graph_path,
            canonical(state),
            (lambda point: fault.hit(f"graph_{point}")) if fault else None,
        )

    def acquire_writer(self) -> int:
        try:
            with self.fs.parent(self.lock_path, create=True) as (parent_fd, name):
                fd = self.fs.open_at(parent_fd, name, os.O_CREAT | os.O_WRONLY)
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            os.close(fd)
            raise SecondWriter("single-writer lock is already held") from exc
        os.ftruncate(fd, 0)
        os.write(fd, str(os.getpid()).encode())
        os.fsync(fd)
        return fd

    def release_writer(self, fd: int) -> None:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)

    @contextmanager
    def writer(self):
        fd = self.acquire_writer()
        try:
            yield
        finally:
            self.release_writer(fd)

    def checked_target(self, raw: str) -> Path:
        relative = safe_relpath(raw)
        target = self.root / relative
        with self.fs.parent(target) as (fd, name):
            self.fs.exists_at(fd, name)
        return target

    def _object_path(self, content_hash: str) -> Path:
        if not re.fullmatch(r"[0-9a-f]{64}", content_hash):
            raise Conflict("invalid object hash")
        return self.objects / content_hash[:2] / content_hash

    def _put_object(self, data: bytes, fault: FaultPlan, label: str) -> str:
        content_hash = digest(data)
        final = self._object_path(content_hash)
        if self.fs.exists(final):
            if digest(self.fs.read(final)) != content_hash:
                raise Conflict("content-addressed object mismatch")
            return content_hash
        fault.hit(f"before_{label}_flush")
        stage = final.with_suffix(".stage")
        with self.fs.open(stage, os.O_CREAT | os.O_WRONLY | os.O_TRUNC, "wb") as handle:
            handle.write(data)
            handle.flush()
            fault.hit("disk_full")
            fault.hit("permission_loss")
            os.fsync(handle.fileno())
        fault.hit(f"after_{label}_flush")
        fault.hit(f"before_{label}_rename")
        self.fs.replace(stage, final)
        fault.hit(f"after_{label}_rename")
        return content_hash

    def mutate(
        self,
        new_bytes: bytes,
        mutation_id: str = "mutation-0001",
        expected_hash: str | None = None,
        selection_hash: str | None = None,
        fault_at: str | None = None,
    ) -> str:
        with self.writer():
            return self._mutate(new_bytes, mutation_id, expected_hash, selection_hash, fault_at)

    def _mutate(self, new_bytes: bytes, mutation_id: str, expected_hash: str | None,
                selection_hash: str | None, fault_at: str | None) -> str:
        # Hold the destination parent descriptor throughout the mutation. Moving
        # or replacing the project pathname never retargets this authority.
        with self.fs.parent(self.root / "artifact.md") as (parent_fd, target_name):
            return self._mutate_at(parent_fd, target_name, new_bytes, mutation_id, expected_hash, selection_hash, fault_at)

    def _mutate_at(self, parent_fd: int, target_name: str, new_bytes: bytes, mutation_id: str,
                   expected_hash: str | None, selection_hash: str | None, fault_at: str | None) -> str:
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", mutation_id):
            raise ValueError("unsafe mutation ID")
        fault = FaultPlan(fault_at)
        current = self.fs.read_at(parent_fd, target_name)
        before_hash = digest(current)
        if expected_hash is not None and expected_hash != before_hash:
            raise Conflict("stale base hash")
        if selection_hash is not None and selection_hash != digest(current):
            raise Conflict("stale selection")
        after_hash = digest(new_bytes)
        fault.hit("before_prepared")
        self._put_object(current, fault, "before_object")
        state = self.state()
        if mutation_id in state["mutations"]:
            raise Conflict("duplicate mutation ID")
        state["mutations"][mutation_id] = {
            "id": mutation_id,
            "state": "PREPARED",
            "path": "artifact.md",
            "before_hash": before_hash,
            "after_hash": after_hash,
            "capability_grant": "write:artifact.md",
            "approval_reference": "approval-test",
        }
        self._save_state(state, fault)
        fault.hit("after_prepared")

        stage = self.sidecar / "staging" / f"{mutation_id}.stage"
        fault.hit("before_staged_file_flush")
        with self.fs.open(stage, os.O_CREAT | os.O_WRONLY | os.O_TRUNC, "wb") as handle:
            handle.write(new_bytes)
            handle.flush()
            fault.hit("before_staged_file_fsync")
            os.fsync(handle.fileno())
            fault.hit("after_staged_file_fsync")
        fault.hit("after_staged_file_flush")
        self._put_object(new_bytes, fault, "after_object")
        fault.hit("before_file_rename")
        self.fs.check_alias(parent_fd, target_name)
        if digest(self.fs.read_at(parent_fd, target_name)) != before_hash:
            raise Conflict("external file change before apply")
        fault.hit("after_file_apply_validation")
        with self.fs.parent(stage) as (stage_fd, stage_name):
            if digest(self.fs.read_at(stage_fd, stage_name)) != after_hash:
                raise Conflict("staged file hash mismatch")
            os.replace(stage_name, target_name, src_dir_fd=stage_fd, dst_dir_fd=parent_fd)
        if digest(self.fs.read_at(parent_fd, target_name)) != after_hash:
            raise Conflict("applied file hash mismatch")
        fault.hit("before_file_directory_fsync")
        os.fsync(parent_fd)
        fault.hit("after_file_directory_fsync")
        fault.hit("after_file_rename")

        state = self.state()
        state["mutations"][mutation_id]["state"] = "FILES_APPLIED"
        fault.hit("before_files_applied")
        self._save_state(state)
        fault.hit("after_files_applied")

        state = self.state()
        mutation = state["mutations"][mutation_id]
        mutation["state"] = "COMMITTED"
        state["revisions"].append(
            {"mutation_id": mutation_id, "path": "artifact.md", "before_hash": before_hash, "after_hash": after_hash}
        )
        state["receipts"].append(
            {"mutation_id": mutation_id, "raw_vault": "unavailable", "status": "committed"}
        )
        fault.hit("before_committed")
        self._save_state(state)
        fault.hit("after_committed")
        self._emit_recovery(state, mutation_id, fault)
        return mutation_id

    def _journal_records(self, tolerate_truncated_tail: bool = False) -> list[dict[str, Any]]:
        if not self.fs.exists(self.journal):
            return []
        raw = self.fs.read(self.journal)
        lines = raw.splitlines(keepends=True)
        records: list[dict[str, Any]] = []
        for index, line in enumerate(lines):
            if not line.endswith(b"\n"):
                if tolerate_truncated_tail and index == len(lines) - 1:
                    break
                raise ValueError("truncated journal record")
            item = json.loads(line)
            if item.get("record_hash") != digest(canonical({k: v for k, v in item.items() if k != "record_hash"})):
                raise Conflict("journal integrity mismatch")
            if item["sequence"] != len(records) + 1:
                raise Conflict("journal sequence gap")
            validate_state(item["graph"])
            if item.get("kind", "MUTATION") == "MUTATION":
                mutation = item["graph"]["mutations"].get(item["mutation_id"])
                if not mutation or mutation["state"] != "COMMITTED" or item["after_hash"] != mutation["after_hash"]:
                    raise Conflict("journal does not describe a committed mutation")
            elif item["kind"] != "GRAPH_CHECKPOINT":
                raise Conflict("unknown journal record kind")
            if records and item["graph"]["project_id"] != records[0]["graph"]["project_id"]:
                raise Conflict("journal contains mixed project identities")
            records.append(item)
        return records

    def _append_journal(self, record: dict[str, Any], fault: FaultPlan) -> None:
        records = self._journal_records(tolerate_truncated_tail=True)
        known = {item["mutation_id"] for item in records}
        # Quarantine and remove only an incomplete tail before appending. Never
        # concatenate a complete record onto torn JSON.
        if self.fs.exists(self.journal):
            raw = self.fs.read(self.journal)
            valid = b"".join(canonical(item) for item in records)
            if raw != valid:
                self.fs.atomic(self.recovery / "quarantine" / f"journal-tail-{digest(raw)}", raw[len(valid):])
                self.fs.atomic(self.journal, valid)
        if record["mutation_id"] in known:
            return
        record["sequence"] = len(records) + 1
        record["record_hash"] = digest(canonical(record))
        fault.hit("before_journal_emission")
        with self.fs.open(self.journal, os.O_CREAT | os.O_WRONLY | os.O_APPEND, "ab") as handle:
            handle.write(canonical(record))
            handle.flush()
            fault.hit("before_journal_fsync")
            os.fsync(handle.fileno())
            fault.hit("after_journal_fsync")
        self.fs.sync_parent(self.journal)
        fault.hit("after_journal_emission")

    def _emit_recovery(self, state: dict[str, Any], mutation_id: str, fault: FaultPlan) -> None:
        mutation = state["mutations"][mutation_id]
        self._emit_checkpoint(state, mutation_id, fault, {"kind": "MUTATION", "after_hash": mutation["after_hash"]})

    def _emit_checkpoint(self, state: dict[str, Any], operation_id: str, fault: FaultPlan,
                         details: dict[str, Any] | None = None) -> None:
        snapshot_state = copy.deepcopy(state)
        snapshot_state["checkpoint"] = digest(canonical({k: v for k, v in snapshot_state.items() if k != "checkpoint"}))
        self._append_journal(
            {
                "mutation_id": operation_id,
                **(details or {"kind": "GRAPH_CHECKPOINT"}),
                "state": "COMMITTED",
                "graph": snapshot_state,
            },
            fault,
        )
        sequence = next(item["sequence"] for item in self._journal_records() if item["mutation_id"] == operation_id)
        fault.hit("before_snapshot_creation")
        self.fs.atomic(
            self.snapshots / f"{operation_id}.json",
            canonical(snapshot_envelope(snapshot_state, sequence)),
            lambda point: fault.hit(f"snapshot_{point}"),
        )
        fault.hit("after_snapshot_creation")
        state["checkpoint"] = snapshot_state["checkpoint"]
        self._save_state(state)

    def recover(self) -> list[str]:
        with self.writer():
            actions = self._recover()
            actions.extend(self._cleanup_stages())
            return actions

    def _cleanup_stages(self) -> list[str]:
        """Remove orphan stage entries without following symlink leaves."""
        actions = []
        staging = self.sidecar / "staging"
        for name in self.fs.names(staging):
            if re.fullmatch(r"[A-Za-z0-9_-]{1,128}\.stage", name):
                self.fs.unlink(staging / name)
                actions.append(f"removed_orphan_stage:{name}")
        directories = [self.graph_path.parent, self.snapshots, self.sidecar,
                       self.recovery, self.sidecar / "conflicts"]
        for prefix in self.fs.names(self.objects):
            if re.fullmatch(r"[0-9a-f]{2}", prefix):
                directories.append(self.objects / prefix)
        for directory in directories:
            for name in self.fs.names(directory):
                if re.fullmatch(r"\..+\.[0-9a-f]{32}\.tmp", name) or re.fullmatch(r"[0-9a-f]{64}\.stage", name):
                    self.fs.unlink(directory / name)
                    actions.append(f"removed_orphan_stage:{name}")
        return actions

    def _recover(self) -> list[str]:
        actions: list[str] = []
        try:
            state = self.state()
        except (ValueError, KeyError, FileNotFoundError, InvalidGraph):
            candidates = []
            for name in self.fs.names(self.snapshots):
                if not name.endswith(".json"):
                    continue
                path = self.snapshots / name
                try:
                    snapshot = json.loads(self.fs.read(path))
                    envelope = {key: value for key, value in snapshot.items() if key != "snapshot_hash"}
                    if (snapshot["snapshot_hash"] == digest(canonical(envelope)) and
                            snapshot.get("snapshot_version") == 1 and
                            isinstance(snapshot["sequence"], int) and snapshot["sequence"] >= 0):
                        validate_state(snapshot["graph"])
                        candidates.append(snapshot)
                except (ValueError, KeyError, Conflict):
                    continue
            records = self._journal_records(tolerate_truncated_tail=True)
            if not candidates and not records:
                raise Conflict("corrupt live database and no validated snapshot/journal")
            snapshot = max(candidates, key=lambda item: item["sequence"]) if candidates else {"sequence": 0, "graph": None}
            candidate = snapshot["graph"]
            for record in records:
                if record["sequence"] > snapshot["sequence"]:
                    candidate = record["graph"]
            envelope = tomllib.loads(self.fs.read(self.sidecar / "project.toml").decode())
            if candidate["project_id"] != envelope["project_id"]:
                raise Conflict("snapshot identity mismatch")
            if self.fs.exists(self.graph_path):
                raw = self.fs.read(self.graph_path)
                self.fs.atomic(self.recovery / "quarantine" / f"store-{digest(raw)}", raw)
            self.fs.atomic(self.graph_path, canonical(candidate))
            state = candidate
            actions.append("restored_corrupt_graph_from_snapshot")
        changed = False
        for mutation_id, mutation in sorted(state["mutations"].items()):
            if mutation["state"] in {"ROLLED_BACK", "EXTERNAL_CONFLICT"}:
                continue
            target = self.root / safe_relpath(mutation["path"])
            try:
                current_bytes = self.fs.read(target)
            except FileNotFoundError:
                current_bytes = None
            current_hash = digest(current_bytes) if current_bytes is not None else None
            if mutation["state"] == "COMMITTED":
                self._emit_recovery(state, mutation_id, FaultPlan())
                continue
            if current_hash == mutation["before_hash"]:
                mutation["state"] = "ROLLED_BACK"
                actions.append(f"rolled_back:{mutation_id}")
                changed = True
            elif current_hash == mutation["after_hash"]:
                mutation["state"] = "COMMITTED"
                if not any(r["mutation_id"] == mutation_id for r in state["revisions"]):
                    state["revisions"].append(
                        {"mutation_id": mutation_id, "path": mutation["path"], "before_hash": mutation["before_hash"], "after_hash": mutation["after_hash"]}
                    )
                    state["receipts"].append(
                        {"mutation_id": mutation_id, "raw_vault": "unavailable", "status": "committed-during-recovery"}
                    )
                actions.append(f"completed:{mutation_id}")
                changed = True
            else:
                conflict = self.sidecar / "conflicts" / f"{mutation_id}-{current_hash or 'missing'}.copy"
                if current_bytes is not None:
                    self.fs.atomic(conflict, current_bytes)
                mutation["state"] = "EXTERNAL_CONFLICT"
                mutation["external_hash"] = current_hash
                actions.append(f"preserved_external_conflict:{mutation_id}")
                changed = True
        if changed:
            self._save_state(state)
        for mutation_id, mutation in sorted(state["mutations"].items()):
            if mutation["state"] == "COMMITTED":
                self._emit_recovery(self.state(), mutation_id, FaultPlan())
        return actions

    def observe_external_change(self, before_hash: str) -> dict[str, Any]:
        with self.writer():
            return self._observe_external_change(before_hash)

    def _observe_external_change(self, before_hash: str) -> dict[str, Any]:
        after_hash = digest(self.fs.read(self.root / "artifact.md"))
        if after_hash == before_hash:
            raise ValueError("no external change")
        state = self.state()
        event = {"kind": "EXTERNAL_CHANGE", "actor": "external/unattributed", "before_hash": before_hash, "after_hash": after_hash}
        state["revisions"].append(event)
        self._save_state(state)
        self._emit_checkpoint(state, f"external-{uuid.uuid4().hex}", FaultPlan())
        return event

    def expire_vault(self, mutation_id: str, reason: str = "key_loss") -> None:
        with self.writer():
            state = self.state()
            receipt = next(item for item in state["receipts"] if item["mutation_id"] == mutation_id)
            receipt["raw_vault"] = "unavailable"
            receipt["vault_tombstone"] = reason
            self._save_state(state)
            self._emit_checkpoint(state, f"vault-loss-{uuid.uuid4().hex}", FaultPlan())

    def revoke_source(self, source_hash: str) -> None:
        with self.writer():
            state = self.state()
            state["derived_chunks"] = [chunk for chunk in state["derived_chunks"] if chunk["source_hash"] != source_hash]
            self._save_state(state)
            self._emit_checkpoint(state, f"revoke-{uuid.uuid4().hex}", FaultPlan())

    def restore_revision(self, content_hash: str, destination: Path) -> None:
        """Explicit recovery to a new copy, never overwriting external bytes."""
        if destination.exists() or destination.is_symlink():
            raise Conflict("restore requires a new destination")
        blob = self._object_path(content_hash)
        if not self.fs.exists(blob):
            raise Conflict("revision unavailable: missing blob")
        data = self.fs.read(blob)
        if digest(data) != content_hash:
            raise Conflict("revision unavailable: corrupt blob")
        destination_fs = ScopedFS(destination.parent)
        try:
            destination_fs.atomic(destination, data, no_replace=True)
        finally:
            destination_fs.close()

    def export_transport(self, profile: str) -> dict[str, Any]:
        with self.writer():
            return self._export_transport(profile)

    def _export_transport(self, profile: str) -> dict[str, Any]:
        if profile not in {"full", "metadata-only"}:
            raise ValueError(profile)
        state = self.state()
        payload: dict[str, Any] = {"profile": profile, "state": state, "objects": {}}
        required = sorted({r["before_hash"] for r in state["revisions"] if "before_hash" in r} | {r["after_hash"] for r in state["revisions"] if "after_hash" in r})
        if profile == "full":
            for object_hash in required:
                path = self._object_path(object_hash)
                if self.fs.exists(path):
                    data = self.fs.read(path)
                    if digest(data) != object_hash:
                        raise Conflict("corrupt revision blob at export")
                    payload["objects"][object_hash] = data.hex()
                else:
                    payload.setdefault("tombstones", []).append({"hash": object_hash, "reason": "missing_at_export"})
        else:
            payload["tombstones"] = [{"hash": item, "reason": "excluded_by_metadata_only_profile"} for item in required]
        payload["export_hash"] = digest(canonical({k: v for k, v in payload.items() if k != "export_hash"}))
        return payload

    @classmethod
    def hydrate(cls, root: Path, payload: dict[str, Any]) -> "RecoveryHarness":
        expected = digest(canonical({k: v for k, v in payload.items() if k != "export_hash"}))
        if expected != payload["export_hash"]:
            raise Conflict("transport hash mismatch")
        if root.exists():
            raise Conflict("restore requires a new destination")
        validate_state(payload["state"])
        for object_hash, value in payload.get("objects", {}).items():
            if not re.fullmatch(r"[0-9a-f]{64}", object_hash) or digest(bytes.fromhex(value)) != object_hash:
                raise Conflict("transport object hash mismatch")
        harness = cls(root, payload["state"]["project_id"])
        harness.fs.atomic(harness.graph_path, canonical(payload["state"]))
        for object_hash, value in payload.get("objects", {}).items():
            data = bytes.fromhex(value)
            if digest(data) != object_hash:
                raise Conflict("transport object hash mismatch")
            harness.fs.atomic(harness._object_path(object_hash), data)
        latest = {}
        for revision in payload["state"]["revisions"]:
            if "path" in revision:
                latest[revision["path"]] = revision["after_hash"]
        for relative, content_hash in latest.items():
            target = harness.checked_target(relative)
            blob = harness._object_path(content_hash)
            if harness.fs.exists(blob):
                harness.fs.atomic(target, harness.fs.read(blob))
            else:
                harness.fs.unlink(target)
        harness.fs.atomic(harness.sidecar / "recovery" / "transport-tombstones.json", canonical(payload.get("tombstones", [])))
        return harness

    def migrate(self, source_schema: int, fault_at: str | None = None) -> None:
        with self.writer():
            self._migrate(source_schema, fault_at)

    def _migrate(self, source_schema: int, fault_at: str | None = None) -> None:
        state = self.state()
        if state["schema_version"] != source_schema:
            raise ValueError("source schema does not match live graph")
        backup = self.fs.read(self.graph_path)
        candidate = copy.deepcopy(state)
        if source_schema == 1:
            candidate.setdefault("derived_chunks", [])
            candidate["export_version"] = 1
        if source_schema in {1, 2}:
            candidate["schema_version"] = CURRENT_SCHEMA
        else:
            raise ValueError("unsupported migration source")
        candidate_path = self.graph_path.with_suffix(".migration")
        fault = FaultPlan(fault_at)
        fault.hit("before_migration_snapshot")
        self.fs.atomic(self.snapshots / "pre-migration.json", canonical(snapshot_envelope(state, 0)))
        fault.hit("after_migration_snapshot")
        fault.hit("before_migration_copy")
        self.fs.atomic(candidate_path, canonical(candidate))
        fault.hit("after_migration_copy")
        if candidate["project_id"] != state["project_id"] or candidate["schema_version"] != CURRENT_SCHEMA:
            raise Conflict("migration invariant validation failed")
        fault.hit("after_migration_validation")
        if fault_at == "before_migration_swap":
            self.fs.unlink(candidate_path)
            if self.fs.read(self.graph_path) != backup:
                raise AssertionError("live store changed before migration swap")
            raise InjectedCrash(fault_at)
        self.fs.replace(candidate_path, self.graph_path)
        fault.hit("after_migration_swap")
        self._emit_checkpoint(candidate, f"schema-migration-{uuid.uuid4().hex}", fault)

    def migrate_pack(self, namespace: str, version: int, transform: Callable[[dict[str, Any]], None],
                     fault_at: str | None = None) -> None:
        if not re.fullmatch(r"[a-z][a-z0-9_-]*\.[a-z][a-z0-9_-]*", namespace):
            raise ValueError("pack must be namespaced")
        if not isinstance(version, int) or isinstance(version, bool) or version < 1:
            raise ValueError("invalid pack schema version")
        with self.writer():
            original = self.state()
            self.fs.atomic(self.snapshots / "pre-pack-migration.json", canonical(snapshot_envelope(original, 0)))
            candidate = copy.deepcopy(original)
            # The migrator receives only its namespace. It cannot redefine the
            # global revision/receipt/mutation collections through this API.
            pack_data = candidate.setdefault("pack_data", {}).setdefault(namespace, {})
            transform(pack_data)
            candidate.setdefault("pack_versions", {})[namespace] = version
            if (candidate["project_id"] != original["project_id"] or
                    candidate["schema_version"] != original["schema_version"] or
                    candidate["mutations"] != original["mutations"]):
                raise Conflict("pack migration changed global semantics")
            self.fs.atomic(self.graph_path, canonical(candidate), lambda point: FaultPlan(fault_at).hit(f"pack_{point}"))
            self._emit_checkpoint(candidate, f"pack-migration-{uuid.uuid4().hex}", FaultPlan(fault_at))


def export_profile(state: dict[str, Any]) -> dict[str, Any]:
    portable = {key: copy.deepcopy(value) for key, value in state.items() if key != "path_bindings"}
    return {"state": portable, "export_hash": digest(canonical(portable))}


def restore_profile(export: dict[str, Any], app_data: Path) -> dict[str, Any]:
    if export.get("export_hash") != digest(canonical(export["state"])):
        raise Conflict("profile export integrity mismatch")
    state = export["state"]
    required = {"cross_project_links", "global_preferences", "approved_type_patterns", "profile_id"}
    if not required.issubset(state) or "path_bindings" in state:
        raise ValueError("incomplete profile export")
    target = app_data / "profile" / "graph" / "state.json"
    atomic_bytes(target, canonical(state))
    return read_json(target)


def resolve_folder_identity(existing: dict[str, Any], incoming: dict[str, Any]) -> str:
    if existing["project_id"] != incoming["project_id"]:
        return "distinct"
    if existing["lineage_id"] != incoming["lineage_id"] or existing["checkpoint"] != incoming["checkpoint"]:
        return "decision_required:fork_or_semantic_reconcile"
    if existing["path"] != incoming["path"]:
        return "reconnect"
    return "same"


def resolve_git_divergence(left: dict[str, Any], right: dict[str, Any]) -> str:
    if left["project_id"] == right["project_id"] and left["journal_head"] != right["journal_head"]:
        return "decision_required:reconnect_fork_or_semantic_reconcile"
    return "fast_path"


class ExternalEffectLedger:
    def __init__(self) -> None:
        self.attempts: dict[str, dict[str, Any]] = {}

    def begin(self, attempt_id: str, documented_idempotency: bool) -> dict[str, Any]:
        if attempt_id in self.attempts:
            raise Conflict("attempt ID already exists")
        record = {"attempt_id": attempt_id, "documented_idempotency": documented_idempotency, "state": "STARTED", "send_count": 1,
                  "idempotency_key": attempt_id if documented_idempotency else None}
        self.attempts[attempt_id] = record
        return record

    def ambiguous_timeout(self, attempt_id: str) -> None:
        self.attempts[attempt_id]["state"] = "OUTCOME_UNKNOWN"

    def retry(self, attempt_id: str, remote_query: str | None = None, renewed_approval: bool = False) -> str:
        record = self.attempts[attempt_id]
        if record["state"] != "OUTCOME_UNKNOWN":
            raise ValueError("not ambiguous")
        if remote_query == "confirmed_applied":
            record["state"] = "CONFIRMED_APPLIED"
            return record["state"]
        if record["documented_idempotency"] or renewed_approval:
            record["send_count"] += 1
            record["state"] = "RETRIED"
            return record["state"]
        return "BLOCKED_RECONCILIATION_REQUIRED"


def temporary_project() -> tuple[tempfile.TemporaryDirectory[str], RecoveryHarness]:
    temp = tempfile.TemporaryDirectory()
    return temp, RecoveryHarness(Path(temp.name) / "project")
