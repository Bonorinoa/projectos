"""Offline fault suite. JSON contract storage, not Gate B engine verification."""
from __future__ import annotations

import copy
import json
import os
import subprocess
import sys
import tempfile
import time
import traceback
import threading
import unittest
from pathlib import Path
from unittest.mock import patch

from recovery_harness import (
    CURRENT_SCHEMA, Conflict, ExternalEffectLedger, FaultPlan, InjectedCrash,
    RecoveryHarness, SecondWriter, atomic_bytes, canonical, digest,
    export_profile, resolve_folder_identity, resolve_git_divergence, restore_profile,
)

BEFORE = b"before\n"
AFTER = b"accepted bytes\n"
BOUNDARIES = [
    "before_prepared", "before_before_object_flush", "after_before_object_flush",
    "before_before_object_rename", "after_before_object_rename",
    "graph_before_fsync", "graph_after_fsync", "graph_before_swap", "graph_after_swap",
    "after_prepared", "before_staged_file_flush", "after_staged_file_flush",
    "before_after_object_flush", "after_after_object_flush", "before_after_object_rename",
    "after_after_object_rename", "before_file_rename", "after_file_rename",
    "before_files_applied", "after_files_applied", "before_committed", "after_committed",
    "before_journal_emission", "before_journal_fsync", "after_journal_fsync",
    "after_journal_emission", "before_snapshot_creation", "snapshot_before_fsync",
    "snapshot_after_fsync", "snapshot_before_swap", "snapshot_after_swap", "after_snapshot_creation",
    "disk_full", "permission_loss",
    "before_staged_file_fsync", "after_staged_file_fsync",
    "before_file_directory_fsync", "after_file_directory_fsync",
    "after_file_apply_validation",
]


class RecoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="projectos-gate-d-")
        self.base = Path(self.temp.name)
        self.h = RecoveryHarness(self.base / "project")

    def tearDown(self):
        self.temp.cleanup()

    def assert_recovery(self, h=None):
        h = h or self.h
        h.recover()
        first = canonical(h.state())
        h.recover()
        self.assertEqual(first, canonical(h.state()), "replay must be idempotent")
        content = (h.root / "artifact.md").read_bytes()
        self.assertIn(content, [BEFORE, AFTER])
        committed = [m for m in h.state()["mutations"].values() if m["state"] == "COMMITTED"]
        if committed:
            self.assertEqual(content, AFTER)
            self.assertEqual(h._object_path(digest(BEFORE)).read_bytes(), BEFORE)
            self.assertEqual(h._object_path(digest(AFTER)).read_bytes(), AFTER)
            self.assertEqual(len(h.state()["revisions"]), 1)
            self.assertEqual(len(h._journal_records()), 1)

    def test_acknowledged_export_restores_bytes_and_unknown_fields(self):
        state = self.h.state()
        state["vendor_extension"] = {"future": [1, 2]}
        self.h._save_state(state)
        self.h.mutate(AFTER)
        payload = self.h.export_transport("full")
        restored = RecoveryHarness.hydrate(self.base / "clone", payload)
        self.assertEqual((restored.root / "artifact.md").read_bytes(), AFTER)
        self.assertEqual(restored.state(), self.h.state())
        self.assertEqual(restored.export_transport("full"), payload)
        self.assertNotIn("credentials", json.dumps(payload))

    def test_metadata_clone_topology_and_truthful_tombstones(self):
        self.h.mutate(AFTER)
        clone = RecoveryHarness.hydrate(self.base / "clone", self.h.export_transport("metadata-only"))
        self.assertEqual(clone.state(), self.h.state())
        self.assertFalse((clone.root / "artifact.md").exists(), "must not fabricate unavailable artifact")
        tombstones = json.loads((clone.recovery / "transport-tombstones.json").read_text())
        self.assertEqual({t["hash"] for t in tombstones}, {digest(BEFORE), digest(AFTER)})

    def test_missing_revision_blob_full_export_discloses_loss(self):
        self.h.mutate(AFTER)
        self.h._object_path(digest(AFTER)).unlink()
        payload = self.h.export_transport("full")
        self.assertEqual(payload["tombstones"], [{"hash": digest(AFTER), "reason": "missing_at_export"}])
        clone = RecoveryHarness.hydrate(self.base / "clone", payload)
        self.assertFalse((clone.root / "artifact.md").exists())

    def test_corrupt_blob_export_rejected(self):
        self.h.mutate(AFTER)
        self.h._object_path(digest(AFTER)).write_bytes(b"corrupt")
        with self.assertRaises(Conflict):
            self.h.export_transport("full")

    def test_transport_hash_tamper_rejected_before_creation(self):
        self.h.mutate(AFTER)
        payload = self.h.export_transport("full")
        payload["state"]["project_id"] = "other"
        with self.assertRaises(Conflict):
            RecoveryHarness.hydrate(self.base / "clone", payload)
        self.assertFalse((self.base / "clone").exists())

    def test_transport_object_tamper_rejected_before_creation(self):
        self.h.mutate(AFTER)
        payload = self.h.export_transport("full")
        payload["objects"][digest(AFTER)] = b"bad".hex()
        payload["export_hash"] = digest(canonical({k: v for k, v in payload.items() if k != "export_hash"}))
        with self.assertRaises(Conflict):
            RecoveryHarness.hydrate(self.base / "clone", payload)
        self.assertFalse((self.base / "clone").exists())

    def test_restore_existing_destination_rejected(self):
        self.h.mutate(AFTER)
        with self.assertRaises(Conflict):
            RecoveryHarness.hydrate(self.h.root, self.h.export_transport("full"))
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), AFTER)

    def test_second_writer_blocked_then_released(self):
        fd = self.h.acquire_writer()
        try:
            with self.assertRaises(SecondWriter):
                self.h.mutate(AFTER)
            with self.assertRaises(SecondWriter):
                self.h.recover()
            with self.assertRaises(SecondWriter):
                self.h.export_transport("full")
        finally:
            self.h.release_writer(fd)
        self.h.mutate(AFTER)

    def test_real_permission_denial_before_graph_replace_preserves_artifact(self):
        graph_dir = self.h.graph_path.parent
        graph_dir.chmod(0o500)
        try:
            with self.assertRaises(PermissionError):
                self.h.mutate(AFTER)
        finally:
            graph_dir.chmod(0o700)
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), BEFORE)
        self.h.recover()

    def test_every_acknowledged_before_after_blob_restorable_to_new_copy(self):
        contents = [BEFORE, AFTER, b"second\n", b"third\n"]
        for index, content in enumerate(contents[1:]):
            self.h.mutate(content, f"m-{index}")
        for index, content in enumerate(contents):
            destination = self.base / f"restored-{index}.md"
            self.h.restore_revision(digest(content), destination)
            self.assertEqual(destination.read_bytes(), content)
        with self.assertRaises(Conflict):
            self.h.restore_revision(digest(AFTER), self.h.root / "artifact.md")

    def test_missing_or_corrupt_blob_restore_keeps_destination_absent(self):
        self.h.mutate(AFTER)
        blob = self.h._object_path(digest(AFTER))
        destination = self.base / "restored.md"
        blob.write_bytes(b"corrupt")
        with self.assertRaises(Conflict):
            self.h.restore_revision(digest(AFTER), destination)
        blob.unlink()
        with self.assertRaises(Conflict):
            self.h.restore_revision(digest(AFTER), destination)
        self.assertFalse(destination.exists())

    def test_restore_no_replace_when_destination_appears_after_check(self):
        self.h.mutate(AFTER)
        destination = self.base / "restored.md"
        original_read = self.h.fs.read
        def create_external(path):
            result = original_read(path)
            if path == self.h._object_path(digest(AFTER)):
                destination.write_bytes(b"external destination")
            return result
        with patch.object(self.h.fs, "read", create_external), self.assertRaises(Conflict):
            self.h.restore_revision(digest(AFTER), destination)
        self.assertEqual(destination.read_bytes(), b"external destination")

    def test_three_acknowledged_mutations_restored_from_journal_after_old_snapshot(self):
        for index, content in enumerate([AFTER, b"second\n", b"third\n"]):
            self.h.mutate(content, f"m-{index}")
        (self.h.snapshots / "m-1.json").unlink()
        (self.h.snapshots / "m-2.json").unlink()
        self.h.graph_path.write_bytes(b"corrupt store")
        actions = self.h.recover()
        self.assertIn("restored_corrupt_graph_from_snapshot", actions)
        self.assertEqual(len(self.h.state()["revisions"]), 3)
        self.assertEqual(len(self.h._journal_records()), 3)
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), b"third\n")
        self.assertTrue(list((self.h.recovery / "quarantine").glob("store-*")))

    def test_corrupt_latest_snapshot_falls_back_and_replays(self):
        self.h.mutate(AFTER, "m-0")
        self.h.mutate(b"second\n", "m-1")
        (self.h.snapshots / "m-1.json").write_bytes(b"bad snapshot")
        self.h.graph_path.write_bytes(b"bad store")
        self.h.recover()
        self.assertEqual(len(self.h.state()["revisions"]), 2)

    def test_snapshot_checksum_tamper_rejected(self):
        self.h.mutate(AFTER)
        path = self.h.snapshots / "mutation-0001.json"
        snapshot = json.loads(path.read_text())
        snapshot["graph"]["lineage_id"] = "tampered"
        atomic_bytes(path, canonical(snapshot))
        self.h.graph_path.write_bytes(b"bad store")
        self.h.journal.unlink()
        with self.assertRaises(Conflict):
            self.h.recover()
        self.assertEqual(self.h.graph_path.read_bytes(), b"bad store")

    def test_old_snapshot_ordering_tamper_does_not_hide_newer_journal(self):
        self.h.mutate(AFTER, "m-0")
        self.h.mutate(b"second\n", "m-1")
        path = self.h.snapshots / "m-0.json"
        snapshot = json.loads(path.read_text())
        snapshot["sequence"] = 999
        atomic_bytes(path, canonical(snapshot))
        self.h.graph_path.write_bytes(b"corrupt store")
        self.h.recover()
        self.assertEqual(len(self.h.state()["revisions"]), 2)

    def test_missing_live_store_hydrates_from_journal(self):
        self.h.mutate(AFTER)
        self.h.graph_path.unlink()
        for snapshot in self.h.snapshots.glob("*.json"):
            snapshot.unlink()
        self.h.recover()
        self.assertEqual(len(self.h.state()["revisions"]), 1)

    def test_structurally_corrupt_graph_hydrates_and_quarantines(self):
        self.h.mutate(AFTER)
        for content in [b"[]\n", b'{"schema_version":3}\n']:
            self.h.graph_path.write_bytes(content)
            self.h.recover()
            self.assertEqual(len(self.h.state()["revisions"]), 1)
            self.assertEqual((self.h.recovery / "quarantine" / f"store-{digest(content)}").read_bytes(), content)

    def test_mutation_identifier_path_escape_rejected(self):
        with self.assertRaises(ValueError):
            self.h.mutate(AFTER, "../../escaped")
        self.assertFalse((self.h.sidecar / "escaped.stage").exists())

    def test_torn_journal_tail_quarantined_before_append(self):
        self.h.mutate(AFTER, "m-0")
        with self.h.journal.open("ab") as handle:
            handle.write(b'{"mutation_id":"torn')
        self.h.mutate(b"second\n", "m-1")
        self.assertEqual(len(self.h._journal_records()), 2)
        self.assertTrue(list((self.h.recovery / "quarantine").glob("journal-tail-*")))

    def test_interior_journal_corruption_preserved_and_rejected(self):
        self.h.mutate(AFTER)
        raw = self.h.journal.read_bytes().replace(b"COMMITTED", b"FORGED___")
        self.h.journal.write_bytes(raw)
        with self.assertRaises(Conflict):
            self.h.recover()
        self.assertEqual(self.h.journal.read_bytes(), raw)

    def test_truncated_only_journal_repaired_from_live_state(self):
        self.h.mutate(AFTER)
        self.h.journal.write_bytes(self.h.journal.read_bytes()[:15])
        self.h.recover()
        self.assertEqual(len(self.h._journal_records()), 1)

    def test_third_hash_conflict_copy_preserved_and_terminal(self):
        with self.assertRaises(InjectedCrash):
            self.h.mutate(AFTER, fault_at="after_prepared")
        external = b"independent editor changed this\n"
        (self.h.root / "artifact.md").write_bytes(external)
        self.h.recover()
        self.h.recover()
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), external)
        copies = list((self.h.sidecar / "conflicts").glob("*.copy"))
        self.assertEqual(len(copies), 1)
        self.assertEqual(copies[0].read_bytes(), external)
        self.assertEqual(self.h.state()["mutations"]["mutation-0001"]["state"], "EXTERNAL_CONFLICT")

    def test_external_change_after_commit_does_not_overwrite(self):
        self.h.mutate(AFTER)
        (self.h.root / "artifact.md").write_bytes(b"external\n")
        self.h.recover()
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), b"external\n")
        event = self.h.observe_external_change(digest(AFTER))
        self.assertEqual(event["actor"], "external/unattributed")
        self.assertNotIn("recoverable_before_image", event)

    def test_stale_base_and_selection_rejected(self):
        for keyword in ["expected_hash", "selection_hash"]:
            with self.assertRaises(Conflict):
                self.h.mutate(AFTER, **{keyword: digest(b"stale")})
        self.assertEqual(self.h.state()["mutations"], {})

    def test_external_edit_immediately_before_apply_preserved(self):
        original = FaultPlan.hit
        def edit(plan, point):
            if point == "before_file_rename":
                (self.h.root / "artifact.md").write_bytes(b"external\n")
            original(plan, point)
        with patch.object(FaultPlan, "hit", edit), self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.h.recover()
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), b"external\n")

    def test_symlink_target_and_case_alias_rejected(self):
        target = self.h.root / "artifact.md"
        target.unlink()
        outside = self.base / "outside.md"
        outside.write_bytes(b"outside")
        target.symlink_to(outside)
        with self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.assertEqual(outside.read_bytes(), b"outside")
        target.unlink()
        (self.h.root / "Artifact.md").write_bytes(BEFORE)
        with self.assertRaises(Conflict):
            self.h.mutate(AFTER)

    def test_symlink_swap_at_known_hook_rejected(self):
        original = FaultPlan.hit
        outside = self.base / "outside.md"
        outside.write_bytes(b"outside")
        def swap(plan, point):
            if point == "before_file_rename":
                target = self.h.root / "artifact.md"
                target.unlink()
                target.symlink_to(outside)
            original(plan, point)
        with patch.object(FaultPlan, "hit", swap), self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.assertEqual(outside.read_bytes(), b"outside")

    def test_symlink_swap_after_final_validation_never_follows_destination(self):
        original = FaultPlan.hit
        outside = self.base / "outside.md"
        outside.write_bytes(b"outside sentinel")
        def swap(plan, point):
            if point == "after_file_apply_validation":
                target = self.h.root / "artifact.md"
                target.unlink()
                target.symlink_to(outside)
            original(plan, point)
        with patch.object(FaultPlan, "hit", swap):
            self.h.mutate(AFTER)
        self.assertEqual(outside.read_bytes(), b"outside sentinel")
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), AFTER)
        self.assertFalse(self.h.fs.exists(self.h._object_path(digest(b"outside sentinel"))))

    def test_active_concurrent_symlink_adversary_no_outside_read_or_write(self):
        outside = self.base / "outside.md"
        outside_bytes = b"private external sentinel\n"
        outside.write_bytes(outside_bytes)
        target = self.h.root / "artifact.md"
        stop = threading.Event()
        started = threading.Event()
        errors = []
        swaps = [0]
        def attack():
            link_stage = self.h.root / ".attacker-link"
            file_stage = self.h.root / ".attacker-ordinary"
            try:
                while not stop.is_set():
                    link_stage.symlink_to(outside)
                    os.replace(link_stage, target)
                    swaps[0] += 1
                    started.set()
                    file_stage.write_bytes(BEFORE)
                    os.replace(file_stage, target)
            except BaseException as exc:
                errors.append(repr(exc))
        adversary = threading.Thread(target=attack, daemon=True)
        adversary.start()
        self.assertTrue(started.wait(2))
        attempts = 0
        try:
            for index in range(30):
                try:
                    self.h.mutate(AFTER, f"race-{index}")
                except (Conflict, FileNotFoundError):
                    pass
                attempts += 1
        finally:
            stop.set()
            adversary.join(3)
        self.assertFalse(adversary.is_alive())
        self.assertEqual(errors, [])
        self.assertGreater(swaps[0], 0)
        self.assertEqual(attempts, 30)
        self.assertEqual(outside.read_bytes(), outside_bytes)
        self.assertFalse(self.h.fs.exists(self.h._object_path(digest(outside_bytes))))

    def test_retained_parent_descriptor_survives_parent_symlink_swap(self):
        directory = self.h.root / "docs"
        directory.mkdir()
        (directory / "artifact.md").write_bytes(BEFORE)
        outside = self.base / "outside"
        outside.mkdir()
        (outside / "artifact.md").write_bytes(b"outside")
        with self.h.fs.parent(directory / "artifact.md") as (parent_fd, name):
            directory.rename(self.h.root / "parked-docs")
            directory.symlink_to(outside, target_is_directory=True)
            self.assertEqual(self.h.fs.read_at(parent_fd, name), BEFORE)
            fd = self.h.fs.open_at(parent_fd, "stage", os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.write(fd, AFTER)
            os.close(fd)
            os.replace("stage", name, src_dir_fd=parent_fd, dst_dir_fd=parent_fd)
        self.assertEqual((outside / "artifact.md").read_bytes(), b"outside")
        self.assertEqual((self.h.root / "parked-docs" / "artifact.md").read_bytes(), AFTER)
        with self.assertRaises(Conflict):
            self.h.fs.read(directory / "artifact.md")

    def test_root_path_replacement_does_not_retarget_opened_project(self):
        original = FaultPlan.hit
        moved = self.base / "pinned-original"
        replacement = self.h.root
        def replace_root(plan, point):
            if point == "after_prepared":
                replacement.rename(moved)
                replacement.mkdir()
                (replacement / "artifact.md").write_bytes(b"replacement sentinel")
            original(plan, point)
        with patch.object(FaultPlan, "hit", replace_root):
            self.h.mutate(AFTER)
        self.assertEqual((moved / "artifact.md").read_bytes(), AFTER)
        self.assertEqual((replacement / "artifact.md").read_bytes(), b"replacement sentinel")
        self.assertFalse((replacement / ".projectos").exists())
        self.assertEqual(self.h.state()["mutations"]["mutation-0001"]["state"], "COMMITTED")
        self.h.recover()
        reopened = RecoveryHarness(moved, create=False)
        self.assertEqual(reopened.state(), self.h.state())

    def test_root_symlink_replacement_does_not_retarget_opened_project(self):
        original = FaultPlan.hit
        moved = self.base / "pinned-original"
        outside = self.base / "outside"
        outside.mkdir()
        (outside / "artifact.md").write_bytes(b"outside")
        def replace_root(plan, point):
            if point == "after_prepared":
                self.h.root.rename(moved)
                self.h.root.symlink_to(outside, target_is_directory=True)
            original(plan, point)
        with patch.object(FaultPlan, "hit", replace_root):
            self.h.mutate(AFTER)
        self.assertEqual((outside / "artifact.md").read_bytes(), b"outside")
        self.assertFalse((outside / ".projectos").exists())
        self.assertEqual((moved / "artifact.md").read_bytes(), AFTER)
        with self.assertRaises(OSError):
            RecoveryHarness(self.h.root, create=False)

    def test_symlinked_sidecar_and_lock_rejected_without_outside_write(self):
        self.h.sidecar.rename(self.h.root / "original-sidecar")
        outside = self.base / "outside"
        outside.mkdir()
        self.h.sidecar.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.assertEqual(list(outside.iterdir()), [])
        self.h.sidecar.unlink()
        (self.h.root / "original-sidecar").rename(self.h.sidecar)
        outside_lock = self.base / "outside-lock"
        outside_lock.write_bytes(b"lock sentinel")
        self.h.lock_path.symlink_to(outside_lock)
        with self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.assertEqual(outside_lock.read_bytes(), b"lock sentinel")

    def test_orphan_stage_cleanup_unlinks_symlink_without_following_it(self):
        with self.assertRaises(InjectedCrash):
            self.h.mutate(AFTER, fault_at="after_staged_file_flush")
        stage = self.h.sidecar / "staging" / "mutation-0001.stage"
        stage.unlink()
        outside = self.base / "outside"
        outside.write_bytes(b"outside sentinel")
        stage.symlink_to(outside)
        actions = self.h.recover()
        self.assertIn("removed_orphan_stage:mutation-0001.stage", actions)
        self.assertFalse(stage.is_symlink())
        self.assertEqual(outside.read_bytes(), b"outside sentinel")

    def test_swapped_staged_source_symlink_rejected_before_install(self):
        outside = self.base / "outside"
        outside.write_bytes(b"outside sentinel")
        original = FaultPlan.hit
        def swap_source(plan, point):
            if point == "before_file_rename":
                stage = self.h.sidecar / "staging" / "mutation-0001.stage"
                stage.unlink()
                stage.symlink_to(outside)
            original(plan, point)
        with patch.object(FaultPlan, "hit", swap_source), self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.h.recover()
        self.assertEqual((self.h.root / "artifact.md").read_bytes(), BEFORE)
        self.assertEqual(outside.read_bytes(), b"outside sentinel")

    def test_hardlink_leaf_rejected_without_reading_or_truncating_outside(self):
        outside = self.base / "outside"
        outside.write_bytes(b"outside sentinel")
        target = self.h.root / "artifact.md"
        target.unlink()
        os.link(outside, target)
        with self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.assertEqual(outside.read_bytes(), b"outside sentinel")

    def test_hardlinked_stage_rejected_before_truncation(self):
        outside = self.base / "outside"
        outside.write_bytes(b"outside sentinel")
        stage = self.h.sidecar / "staging" / "mutation-0001.stage"
        stage.parent.mkdir()
        os.link(outside, stage)
        with self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.assertEqual(outside.read_bytes(), b"outside sentinel")

    def test_fifo_leaf_rejected_without_blocking(self):
        target = self.h.root / "artifact.md"
        target.unlink()
        os.mkfifo(target)
        started = time.perf_counter()
        with self.assertRaises(Conflict):
            self.h.mutate(AFTER)
        self.assertLess(time.perf_counter() - started, 2)

    def test_manifest_identity_mismatch_never_repaired_automatically(self):
        self.h.mutate(AFTER)
        path = self.h.sidecar / "project.toml"
        path.write_text('project_id = "other"\nschema_version = 3\n')
        with self.assertRaises(Conflict):
            self.h.recover()

    def test_folder_move_reopens_and_identity_divergence_requires_decision(self):
        self.h.mutate(AFTER)
        old = self.h.root
        moved = self.base / "moved"
        old.rename(moved)
        reopened = RecoveryHarness(moved, create=False)
        reopened.recover()
        self.assertEqual((moved / "artifact.md").read_bytes(), AFTER)
        identity = {**reopened.state(), "path": str(old)}
        self.assertEqual(resolve_folder_identity(identity, {**identity, "path": str(moved)}), "reconnect")
        for field in ["lineage_id", "checkpoint"]:
            self.assertIn("decision_required", resolve_folder_identity(identity, {**identity, field: "diverged"}))
        self.assertIn("decision_required", resolve_git_divergence({**identity, "journal_head": "a"}, {**identity, "journal_head": "b"}))

    def test_clean_profile_restore_links_preferences_patterns_without_path_bindings(self):
        profile = {"profile_id": "p1", "cross_project_links": [{"from": "p1", "to": "p2"}],
                   "global_preferences": {"theme": "dark"}, "approved_type_patterns": ["software.v1"],
                   "path_bindings": {"p1": "/private/local/path"}, "future_field": {"value": 42}}
        export = export_profile(profile)
        restored = restore_profile(export, self.base / "fresh-app-data")
        self.assertEqual(restored, {k: v for k, v in profile.items() if k != "path_bindings"})
        self.assertNotIn(b"/private/local/path", canonical(export))
        export["state"]["global_preferences"]["theme"] = "tampered"
        with self.assertRaises(Conflict):
            restore_profile(export, self.base / "other-app-data")

    def test_vault_key_loss_preserves_receipt_with_tombstone(self):
        self.h.mutate(AFTER)
        self.h.expire_vault("mutation-0001")
        receipt = self.h.state()["receipts"][0]
        self.assertEqual(receipt["raw_vault"], "unavailable")
        self.assertEqual(receipt["vault_tombstone"], "key_loss")
        self.h.graph_path.write_bytes(b"corrupt")
        self.h.recover()
        self.assertEqual(self.h.state()["receipts"][0]["vault_tombstone"], "key_loss")

    def test_revocation_removes_only_derived_source_chunks(self):
        state = self.h.state()
        state["derived_chunks"] = [{"source_hash": "a", "id": "1"}, {"source_hash": "b", "id": "2"}]
        self.h._save_state(state)
        self.h.revoke_source("a")
        self.assertEqual(self.h.state()["derived_chunks"], [{"source_hash": "b", "id": "2"}])
        self.h.graph_path.write_bytes(b"corrupt")
        self.h.recover()
        self.assertEqual(self.h.state()["derived_chunks"], [{"source_hash": "b", "id": "2"}])

    def test_pack_migration_failure_preserves_live_store_and_restore_point(self):
        original = self.h.graph_path.read_bytes()
        def transform(state):
            state["objects"] = [{"type": "software.task"}]
        with self.assertRaises(InjectedCrash):
            self.h.migrate_pack("firstparty.software", 2, transform, "pack_before_swap")
        self.assertEqual(self.h.graph_path.read_bytes(), original)
        self.assertTrue((self.h.snapshots / "pre-pack-migration.json").exists())
        self.h.migrate_pack("firstparty.software", 2, transform)
        self.assertEqual(self.h.state()["pack_versions"], {"firstparty.software": 2})
        self.h.migrate_pack("firstparty.software", 3, lambda state: state.update(project_id="namespaced-value"))
        self.assertEqual(self.h.state()["project_id"], "project-0001")
        with self.assertRaises(ValueError):
            self.h.migrate_pack("global", 3, transform)

    def test_acknowledged_pack_migration_survives_corrupt_live_graph(self):
        self.h.mutate(AFTER)
        self.h.migrate_pack("firstparty.software", 2, lambda state: state.update(objects=[{"type": "software.task"}]))
        self.h.graph_path.write_bytes(b"corrupt store")
        self.h.recover()
        self.assertEqual(self.h.state()["pack_versions"], {"firstparty.software": 2})
        self.assertEqual(self.h.state()["pack_data"]["firstparty.software"]["objects"], [{"type": "software.task"}])

    def test_pack_transform_failure_cannot_change_other_namespaces_or_global_facts(self):
        self.h.mutate(AFTER)
        state = self.h.state()
        state["pack_data"] = {"firstparty.research": {"objects": ["keep"]}}
        self.h._save_state(state)
        original = self.h.graph_path.read_bytes()
        def bad_transform(data):
            data["partial"] = True
            raise ValueError("fixture migration failed")
        with self.assertRaises(ValueError):
            self.h.migrate_pack("firstparty.software", 2, bad_transform)
        self.assertEqual(original, self.h.graph_path.read_bytes())
        self.h.migrate_pack("firstparty.software", 2, lambda data: data.update(receipts=[], revisions=[]))
        self.assertEqual(self.h.state()["pack_data"]["firstparty.research"], {"objects": ["keep"]})
        self.assertEqual(self.h.state()["receipts"], state["receipts"])
        self.assertEqual(self.h.state()["revisions"], state["revisions"])

    def test_acknowledged_schema_migration_survives_corrupt_live_graph(self):
        state = self.h.state()
        state["schema_version"] = 2
        self.h._save_state(state)
        self.h.mutate(AFTER)
        self.h.migrate(2)
        self.h.graph_path.write_bytes(b"corrupt store")
        self.h.recover()
        self.assertEqual(self.h.state()["schema_version"], 3)

    def test_unknown_external_effect_requires_reconcile_or_approval(self):
        ledger = ExternalEffectLedger()
        record = ledger.begin("attempt-1", False)
        self.assertIsNone(record["idempotency_key"])
        ledger.ambiguous_timeout("attempt-1")
        self.assertEqual(ledger.retry("attempt-1"), "BLOCKED_RECONCILIATION_REQUIRED")
        self.assertEqual(record["send_count"], 1)
        self.assertEqual(ledger.retry("attempt-1", remote_query="confirmed_applied"), "CONFIRMED_APPLIED")
        self.assertEqual(record["send_count"], 1)
        with self.assertRaises(Conflict):
            ledger.begin("attempt-1", False)
        other = ledger.begin("attempt-2", False)
        ledger.ambiguous_timeout("attempt-2")
        self.assertEqual(ledger.retry("attempt-2", renewed_approval=True), "RETRIED")
        self.assertEqual(other["send_count"], 2)

    def test_documented_idempotency_retry_keeps_key(self):
        ledger = ExternalEffectLedger()
        record = ledger.begin("attempt-1", True)
        ledger.ambiguous_timeout("attempt-1")
        self.assertEqual(ledger.retry("attempt-1"), "RETRIED")
        self.assertEqual(record["idempotency_key"], "attempt-1")


def fault_test(point):
    def run(self):
        expected = OSError if point in {"disk_full", "permission_loss"} else InjectedCrash
        with self.assertRaises(expected):
            self.h.mutate(AFTER, fault_at=point)
        self.assert_recovery()
    return run


def process_exit_test(point):
    def run(self):
        script = """import os, sys
from pathlib import Path
from recovery_harness import RecoveryHarness, FaultPlan
original = FaultPlan.hit
def hit(self, point):
    if point == sys.argv[2]: os._exit(91)
    original(self, point)
FaultPlan.hit = hit
RecoveryHarness(Path(sys.argv[1]), create=False).mutate(b'accepted bytes\\n')
"""
        result = subprocess.run([sys.executable, "-c", script, str(self.h.root), point],
                                cwd=Path(__file__).parent, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 91, result.stderr.decode())
        self.assert_recovery()
    return run


def migration_test(schema, point):
    def run(self):
        state = self.h.state()
        state["schema_version"] = schema
        state["future_extension"] = {"kept": True}
        self.h._save_state(state)
        original = self.h.graph_path.read_bytes()
        if point:
            with self.assertRaises(InjectedCrash):
                self.h.migrate(schema, point)
            if point == "after_migration_swap":
                self.assertEqual(self.h.state()["schema_version"], CURRENT_SCHEMA)
            else:
                self.assertEqual(original, self.h.graph_path.read_bytes())
        else:
            self.h.migrate(schema)
            self.assertEqual(self.h.state()["schema_version"], CURRENT_SCHEMA)
        self.assertEqual(self.h.state()["future_extension"], {"kept": True})
    return run


for point in BOUNDARIES:
    setattr(RecoveryTests, f"test_fault_{point}", fault_test(point))
for point in ["after_prepared", "after_staged_file_flush", "after_file_rename", "after_files_applied",
              "after_committed", "before_journal_fsync", "after_journal_emission", "snapshot_after_swap"]:
    setattr(RecoveryTests, f"test_process_exit_{point}", process_exit_test(point))
for schema in [1, 2]:
    for point in [None, "before_migration_snapshot", "after_migration_snapshot", "before_migration_copy",
                  "after_migration_copy", "after_migration_validation", "before_migration_swap", "after_migration_swap"]:
        setattr(RecoveryTests, f"test_migrate_n_minus_{3-schema}_{point or 'success'}", migration_test(schema, point))


class EvidenceResult(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.records = []

    def startTest(self, test):
        self.started = time.perf_counter()
        super().startTest(test)

    def record(self, test, status, err=None):
        self.records.append({"id": test.id(), "status": status,
                             "duration_seconds": round(time.perf_counter() - self.started, 6),
                             "error": "".join(traceback.format_exception(*err)) if err else None})

    def addSuccess(self, test):
        self.record(test, "passed")
        super().addSuccess(test)

    def addFailure(self, test, err):
        self.record(test, "failed", err)
        super().addFailure(test, err)

    def addError(self, test, err):
        self.record(test, "error", err)
        super().addError(test, err)


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(RecoveryTests)
    result = unittest.TextTestRunner(verbosity=2, resultclass=EvidenceResult).run(suite)
    if args.report:
        import platform
        from datetime import datetime, timezone
        sources = {str(path.relative_to(Path(__file__).parents[2])): digest(path.read_bytes())
                   for path in sorted(Path(__file__).parent.glob("*.py"))}
        report = {"gate": "D", "scope": "offline JSON contract model on host filesystem",
                  "gate_status": "partial_unverified", "model_suite_passed": result.wasSuccessful(),
                  "timestamp_utc": datetime.now(timezone.utc).isoformat(),
                  "host": {"system": platform.system(), "release": platform.release(),
                           "machine": platform.machine(), "python": platform.python_version()},
                  "source_sha256": sources, "total": result.testsRun,
                  "passed": sum(item["status"] == "passed" for item in result.records),
                  "failed": len(result.failures), "errors": len(result.errors), "results": result.records,
                  "untested": ["Helix/Ladybug embedded engine integration and WAL/transaction failure",
                               "power loss and hardware/filesystem flush durability", "Linux export/import and installer",
                               "cloud-synced/network filesystem behavior", "case/Unicode alias semantics across filesystems",
                               "atomic compare-and-swap against external regular-file edits",
                               "production profile adapter/journal replay", "durable external-attempt ledger",
                               "retention policies and user-facing recovery UI"]}
        atomic_bytes(args.report, canonical(report))
    sys.exit(0 if result.wasSuccessful() else 1)
