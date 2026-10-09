from __future__ import annotations

import argparse
import importlib.metadata
import json
import os
import platform
import resource
import shutil
import subprocess
import sys
import threading
import time
import traceback
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, Callable

from common import (
    CompanionFts,
    Edge,
    Node,
    append_journal,
    canonical_bytes,
    canonical_hash,
    load_corpus,
    migrate_v1_to_v2,
    parse_canonical,
    read_journal,
)
from helix_adapter import HelixAdapter
from ladybug_adapter import LadybugAdapter


ROOT = Path(__file__).resolve().parents[3]
SPIKE = ROOT / "spikes" / "gate-b"
FIXTURE = SPIKE / "fixtures" / "corpus.json"
WORK = SPIKE / "work"
EVIDENCE = ROOT / "evidence" / "gates" / "gate-b"


def adapter_for(engine: str, path: Path):
    if engine == "ladybug":
        return LadybugAdapter(path)
    if engine == "helix":
        return HelixAdapter(path)
    raise ValueError(f"unknown engine {engine}")


def result(name: str, status: str, detail: str, **metrics: Any) -> dict[str, Any]:
    row: dict[str, Any] = {"name": name, "status": status, "detail": detail}
    if metrics:
        row["metrics"] = metrics
    return row


def guarded(name: str, fn: Callable[[], dict[str, Any]]) -> dict[str, Any]:
    started = time.monotonic()
    try:
        row = fn()
    except Exception as exc:
        row = result(name, "fail", f"{type(exc).__name__}: {exc}", traceback=traceback.format_exc())
    row.setdefault("elapsed_seconds", round(time.monotonic() - started, 6))
    return row


def test_mutation_and_traversal(engine: str, root: Path) -> dict[str, Any]:
    nodes, edges = load_corpus(FIXTURE)
    adapter = adapter_for(engine, root / "mutation")
    try:
        adapter.transactional_seed(nodes, edges)
        adapter.upsert_node(Node("revision:2", "ArtifactRevision", "Revision two updated", "Updated", 1))
        adapter.upsert_edge(Edge("edge:support", "SUPPORTS", "evidence:vendor", "claim:embedded", 2))
        actual_nodes, actual_edges = adapter.export()
        if len(actual_nodes) != 7 or len(actual_edges) != 7:
            raise AssertionError(f"upsert duplicated records: {len(actual_nodes)} nodes/{len(actual_edges)} edges")
        revision = next(node for node in actual_nodes if node.stable_id == "revision:2")
        support = next(edge for edge in actual_edges if edge.stable_id == "edge:support")
        if revision.name != "Revision two updated" or support.position != 2:
            raise AssertionError("upsert did not update existing node/edge")
        checks = {
            "one_hop_lineage": adapter.traverse(
                "revision:2", direction="out", kind="DERIVED_FROM", hops=1
            ),
            "two_hop_lineage": adapter.traverse(
                "revision:2", direction="out", kind="DERIVED_FROM", hops=2
            ),
            "two_hop_impact": adapter.traverse(
                "artifact:brief", direction="in", kind="DERIVED_FROM", hops=2
            ),
        }
        expected = {
            "one_hop_lineage": ["revision:1"],
            "two_hop_lineage": ["artifact:brief"],
            "two_hop_impact": ["revision:2"],
        }
        if checks != expected:
            raise AssertionError(f"incorrect traversal: {checks!r}")
        adapter.delete_edge("edge:support")
        adapter.delete_node("asset:orphan")
        final_nodes, final_edges = adapter.export()
        if len(final_nodes) != 6 or len(final_edges) != 6:
            raise AssertionError(f"delete mismatch: {len(final_nodes)} nodes/{len(final_edges)} edges")
        return result(
            "transactional_mutation_and_traversal",
            "pass",
            "node/edge insert, update, delete, one/two-hop lineage, and reverse impact matched fixture",
            traversals=checks,
            post_delete_nodes=len(final_nodes),
            post_delete_edges=len(final_edges),
        )
    finally:
        adapter.close()


def test_export_import(engine: str, root: Path) -> dict[str, Any]:
    nodes, edges = load_corpus(FIXTURE)
    source = adapter_for(engine, root / "export-source")
    try:
        source.transactional_seed(nodes, edges)
        first_nodes, first_edges = source.export()
        payload_1 = canonical_bytes(first_nodes, first_edges)
        second_nodes, second_edges = source.export()
        payload_2 = canonical_bytes(second_nodes, second_edges)
        if payload_1 != payload_2:
            raise AssertionError("two exports of unchanged state differ")
    finally:
        source.close()
    imported_nodes, imported_edges, _ = parse_canonical(payload_1)
    target = adapter_for(engine, root / "import-target")
    try:
        target.transactional_seed(imported_nodes, imported_edges)
        round_nodes, round_edges = target.export()
        payload_3 = canonical_bytes(round_nodes, round_edges)
    finally:
        target.close()
    if payload_3 != payload_1:
        raise AssertionError("export/import/export bytes differ")
    return result(
        "deterministic_export_import",
        "pass",
        "two unchanged exports and an import round-trip produced byte-identical canonical JSONL",
        sha256=canonical_hash(payload_1),
        bytes=len(payload_1),
    )


def test_companion_fts(engine: str, root: Path) -> dict[str, Any]:
    nodes, edges = load_corpus(FIXTURE)
    adapter = adapter_for(engine, root / "fts-graph")
    index = CompanionFts(root / "fts.sqlite")
    try:
        adapter.transactional_seed(nodes, edges)
        exported_nodes, _ = adapter.export()
        index.rebuild(exported_nodes)
        stable_ids = index.search("embedded")
        graph_ids = {node.stable_id for node in exported_nodes}
        if not stable_ids or not set(stable_ids).issubset(graph_ids):
            raise AssertionError(f"FTS returned invalid graph mapping: {stable_ids}")
        return result(
            "companion_full_text_mapping",
            "pass",
            "disposable SQLite FTS5 results mapped only through durable graph stable IDs",
            query="embedded",
            stable_ids=stable_ids,
        )
    finally:
        index.close()
        adapter.close()


def test_concurrency(engine: str, root: Path) -> dict[str, Any]:
    nodes, edges = load_corpus(FIXTURE)
    adapter = adapter_for(engine, root / "concurrency")
    adapter.transactional_seed(nodes, edges)
    writer_lock = threading.Lock()

    def reader() -> int:
        if engine == "ladybug":
            connection = adapter.new_connection()
            try:
                count = 0
                for _ in range(25):
                    rows = connection.execute("MATCH (e:Entity) RETURN count(e)").get_all()
                    count += int(rows[0][0])
                return count
            finally:
                connection.close()
        count = 0
        for _ in range(25):
            count += len(
                adapter.traverse("revision:2", direction="out", kind="DERIVED_FROM", hops=2)
            )
        return count

    def writer() -> int:
        for idx in range(20):
            with writer_lock:
                adapter.upsert_node(
                    Node("concurrency:writer", "Artifact", f"Writer {idx}", "controlled write", 1)
                )
        return 20

    try:
        with ThreadPoolExecutor(max_workers=5) as pool:
            futures = [pool.submit(reader) for _ in range(4)] + [pool.submit(writer)]
            outcomes = [future.result(timeout=60) for future in futures]
        exported_nodes, _ = adapter.export()
        writer_nodes = [node for node in exported_nodes if node.stable_id == "concurrency:writer"]
        if len(writer_nodes) != 1 or writer_nodes[0].name != "Writer 19":
            raise AssertionError("controlled writer lost or duplicated its final value")
        return result(
            "concurrent_read_controlled_write",
            "pass",
            "four concurrent readers completed while one serialized writer performed 20 upserts",
            reader_accumulators=outcomes[:4],
            writes=outcomes[4],
        )
    finally:
        adapter.close()


def apply_journal(adapter: Any, records: list[dict[str, Any]]) -> None:
    for record in records:
        if record["op"] == "upsert_node":
            adapter.upsert_node(Node(**record["node"]))
        elif record["op"] == "upsert_edge":
            adapter.upsert_edge(Edge(**record["edge"]))
        else:
            raise ValueError(f"unknown journal operation {record['op']}")


def crash_child(engine: str, database: Path, journal: Path) -> None:
    adapter = adapter_for(engine, database)
    node_a = Node("crash:a", "Artifact", "Crash A", "committed before abrupt exit", 1)
    node_b = Node("crash:b", "ArtifactRevision", "Crash B", "journal replay target", 1)
    edge = Edge("crash:edge", "DERIVED_FROM", "crash:b", "crash:a", 1)
    for node in (node_a, node_b):
        adapter.upsert_node(node)
        append_journal(journal, {"op": "upsert_node", "node": node.__dict__})
    adapter.upsert_edge(edge)
    append_journal(journal, {"op": "upsert_edge", "edge": edge.__dict__})
    os._exit(79)


def test_crash_reopen_replay(engine: str, root: Path) -> dict[str, Any]:
    database = root / "crash-live"
    journal = root / "journal.jsonl"
    child = subprocess.run(
        [sys.executable, str(Path(__file__).resolve()), "--crash-child", engine, str(database), str(journal)],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if child.returncode != 79:
        raise AssertionError(
            f"crash child exited {child.returncode}, expected 79; stderr={child.stderr[-1000:]}"
        )
    reopened = adapter_for(engine, database)
    try:
        reopened_nodes, reopened_edges = reopened.export()
        reopened_payload = canonical_bytes(reopened_nodes, reopened_edges)
    finally:
        reopened.close()
    records = read_journal(journal)
    replay = adapter_for(engine, root / "crash-replay")
    try:
        apply_journal(replay, records)
        apply_journal(replay, records)
        replay_nodes, replay_edges = replay.export()
        replay_payload = canonical_bytes(replay_nodes, replay_edges)
    finally:
        replay.close()
    if reopened_payload != replay_payload:
        raise AssertionError("reopened store and idempotently replayed journal differ")
    return result(
        "crash_reopen_and_journal_replay",
        "pass",
        "abrupt process exit after committed writes reopened cleanly; replaying the fsynced operation journal twice was idempotent",
        child_exit=child.returncode,
        journal_records=len(records),
        sha256=canonical_hash(reopened_payload),
    )


def test_copy_on_write_migration(engine: str, root: Path) -> dict[str, Any]:
    nodes, edges = load_corpus(FIXTURE)
    active = root / "migration-active"
    candidate = root / "migration-candidate"
    backup = root / "migration-backup"
    source = adapter_for(engine, active)
    try:
        source.transactional_seed(nodes, edges)
        source_nodes, source_edges = source.export()
        v1 = canonical_bytes(source_nodes, source_edges, export_version=1)
    finally:
        source.close()
    v2 = migrate_v1_to_v2(v1)
    v2_nodes, v2_edges, _ = parse_canonical(v2)
    staged = adapter_for(engine, candidate)
    try:
        staged.transactional_seed(v2_nodes, v2_edges)
        check_nodes, check_edges = staged.export()
        checked = canonical_bytes(check_nodes, check_edges, export_version=2)
    finally:
        staged.close()
    if checked != v2:
        raise AssertionError("candidate migration failed validation before swap")
    os.replace(active, backup)
    os.replace(candidate, active)
    reopened = adapter_for(engine, active)
    try:
        final_nodes, final_edges = reopened.export()
        final = canonical_bytes(final_nodes, final_edges, export_version=2)
    finally:
        reopened.close()
    if final != v2 or any(node.schema_version != 2 for node in final_nodes):
        raise AssertionError("post-swap migrated store failed validation")
    return result(
        "copy_on_write_migration",
        "pass",
        "snapshot was migrated in a separate store, validated byte-for-byte, atomically swapped, and reopened",
        source_sha256=canonical_hash(v1),
        migrated_sha256=canonical_hash(v2),
        backup_preserved=backup.exists(),
    )


def synthetic_corpus(projects: int = 3, artifacts_per_project: int = 100) -> tuple[list[Node], list[Edge]]:
    nodes: list[Node] = []
    edges: list[Edge] = []
    for project_idx in range(projects):
        project_id = f"memory:project:{project_idx:02d}"
        nodes.append(Node(project_id, "Project", f"Project {project_idx}", "memory corpus", 1))
        for artifact_idx in range(artifacts_per_project):
            artifact_id = f"memory:artifact:{project_idx:02d}:{artifact_idx:04d}"
            revision_id = f"memory:revision:{project_idx:02d}:{artifact_idx:04d}"
            nodes.extend(
                [
                    Node(artifact_id, "Artifact", f"Artifact {artifact_idx}", "bounded memory fixture", 1),
                    Node(revision_id, "ArtifactRevision", f"Revision {artifact_idx}", "lineage", 1),
                ]
            )
            edges.extend(
                [
                    Edge(
                        f"memory:contains:{project_idx:02d}:{artifact_idx:04d}",
                        "CONTAINS",
                        project_id,
                        artifact_id,
                        artifact_idx,
                    ),
                    Edge(
                        f"memory:revision:{project_idx:02d}:{artifact_idx:04d}",
                        "REVISION_OF",
                        revision_id,
                        artifact_id,
                        1,
                    ),
                ]
            )
    return nodes, edges


def max_rss_bytes() -> int:
    value = int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return value if sys.platform == "darwin" else value * 1024


def directory_size(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    return sum(item.stat().st_size for item in path.rglob("*") if item.is_file())


def memory_child(engine: str, database: Path) -> None:
    nodes, edges = synthetic_corpus()
    before = max_rss_bytes()
    adapter = adapter_for(engine, database)
    opened = max_rss_bytes()
    started = time.monotonic()
    adapter.transactional_seed(nodes, edges)
    adapter.checkpoint()
    exported_nodes, exported_edges = adapter.export()
    elapsed = time.monotonic() - started
    peak = max_rss_bytes()
    adapter.close()
    print(
        json.dumps(
            {
                "engine": engine,
                "nodes": len(exported_nodes),
                "edges": len(exported_edges),
                "rss_before_bytes": before,
                "rss_after_open_bytes": opened,
                "peak_rss_bytes": peak,
                "peak_delta_bytes": max(0, peak - before),
                "store_bytes": directory_size(database),
                "elapsed_seconds": round(elapsed, 6),
            },
            sort_keys=True,
        )
    )


def test_memory_probe(engine: str, root: Path) -> dict[str, Any]:
    child = subprocess.run(
        [sys.executable, str(Path(__file__).resolve()), "--memory-child", engine, str(root / "memory")],
        capture_output=True,
        text=True,
        timeout=240,
    )
    if child.returncode != 0:
        raise AssertionError(f"memory probe exited {child.returncode}: {child.stderr[-2000:]}")
    metrics = json.loads(child.stdout.strip().splitlines()[-1])
    threshold = 768 * 1024 * 1024
    if metrics["peak_rss_bytes"] > threshold:
        raise AssertionError(f"peak RSS {metrics['peak_rss_bytes']} exceeds spike ceiling {threshold}")
    return result(
        "bounded_memory_spike_corpus",
        "pass",
        "3-project synthetic corpus remained below the spike-only 768 MiB peak RSS ceiling; production scale is not established",
        **metrics,
        threshold_bytes=threshold,
    )


def hold_open(engine: str, database: Path) -> None:
    adapter = adapter_for(engine, database)
    print(os.getpid(), flush=True)
    try:
        time.sleep(30)
    finally:
        adapter.close()


def test_no_listening_port(engine: str, root: Path) -> dict[str, Any]:
    lsof = shutil.which("lsof")
    if lsof is None:
        return result("no_listening_port", "untested", "lsof is unavailable on this host")
    child = subprocess.Popen(
        [sys.executable, str(Path(__file__).resolve()), "--hold-open", engine, str(root / "port-check")],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        assert child.stdout is not None
        pid_line = child.stdout.readline().strip()
        if not pid_line:
            stderr = child.stderr.read() if child.stderr else ""
            raise AssertionError(f"embedded child did not report a pid: {stderr[-1000:]}")
        probe = subprocess.run(
            [lsof, "-nP", "-a", "-p", pid_line, "-iTCP", "-sTCP:LISTEN"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        if probe.stdout.strip():
            raise AssertionError(f"embedded process opened a listening TCP socket: {probe.stdout}")
        return result(
            "no_listening_port",
            "pass",
            "live embedded process had no listening TCP sockets according to lsof",
            pid=int(pid_line),
            lsof_exit=probe.returncode,
        )
    finally:
        child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=5)


def engine_version(engine: str) -> dict[str, str]:
    if engine == "ladybug":
        return {"python_binding": importlib.metadata.version("ladybug")}
    return {
        "python_sdk": importlib.metadata.version("helix-db"),
        "embedded_runtime": importlib.metadata.version("helix-db-embedded"),
    }


def run_engine(engine: str) -> dict[str, Any]:
    root = WORK / engine
    root.mkdir(parents=True, exist_ok=True)
    tests = [
        guarded("transactional_mutation_and_traversal", lambda: test_mutation_and_traversal(engine, root)),
        guarded("deterministic_export_import", lambda: test_export_import(engine, root)),
        guarded("companion_full_text_mapping", lambda: test_companion_fts(engine, root)),
        guarded("concurrent_read_controlled_write", lambda: test_concurrency(engine, root)),
        guarded("crash_reopen_and_journal_replay", lambda: test_crash_reopen_replay(engine, root)),
        guarded("copy_on_write_migration", lambda: test_copy_on_write_migration(engine, root)),
        guarded("bounded_memory_spike_corpus", lambda: test_memory_probe(engine, root)),
        guarded("no_listening_port", lambda: test_no_listening_port(engine, root)),
    ]
    return {
        "engine": engine,
        "versions": engine_version(engine),
        "status": "pass" if all(test["status"] == "pass" for test in tests) else "fail",
        "tests": tests,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--crash-child", nargs=3, metavar=("ENGINE", "DATABASE", "JOURNAL"))
    parser.add_argument("--memory-child", nargs=2, metavar=("ENGINE", "DATABASE"))
    parser.add_argument("--hold-open", nargs=2, metavar=("ENGINE", "DATABASE"))
    args = parser.parse_args()
    if args.crash_child:
        engine, database, journal = args.crash_child
        crash_child(engine, Path(database), Path(journal))
        return 79
    if args.memory_child:
        engine, database = args.memory_child
        memory_child(engine, Path(database))
        return 0
    if args.hold_open:
        engine, database = args.hold_open
        hold_open(engine, Path(database))
        return 0

    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    report = {
        "gate": "M0 Gate B",
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "rust_toolchain_present": shutil.which("cargo") is not None,
        },
        "fixture": str(FIXTURE.relative_to(ROOT)),
        "engines": [run_engine("ladybug"), run_engine("helix")],
    }
    output = EVIDENCE / "runtime-results.json"
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if all(engine["status"] == "pass" for engine in report["engines"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
