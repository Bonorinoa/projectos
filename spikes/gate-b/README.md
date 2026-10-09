# Gate B reproducible spike

This directory contains the narrow shared adapter/corpus used to evaluate LadybugDB and HelixDB as in-process graph engines for ProjectOS. It is a feasibility harness, not production ProjectOS code.

## Run

```sh
python3 -m venv spikes/gate-b/.venv
spikes/gate-b/.venv/bin/python -m pip install -r spikes/gate-b/requirements.lock
PYTHONPATH=spikes/gate-b/src \
  spikes/gate-b/.venv/bin/python spikes/gate-b/src/run_gate_b.py
```

The runner recreates only `spikes/gate-b/work/` and writes `evidence/gates/gate-b/runtime-results.json`.

## What it tests

- embedded open with no listening TCP port;
- transactional node/edge mutation and deletion;
- exact one/two-hop lineage and impact traversal;
- stable-ID mapping to a disposable FTS5 companion index;
- concurrent readers plus one controlled writer;
- abrupt exit, reopen, and idempotent operation-journal replay;
- canonical deterministic export/import;
- copy-on-write migration with validation before atomic swap;
- a small deterministic multi-project memory probe.

The adapters intentionally expose the same domain-level operations and canonical JSONL representation. They do not hide unsupported product requirements: Rust/Tauri and clean target packaging remain separate blocking tests.
