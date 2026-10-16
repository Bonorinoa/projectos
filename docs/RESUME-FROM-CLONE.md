# Resume from a fresh clone (freeze of 2026-10-15)

ProjectOS was frozen on GitHub at tag `checkpoint-2026-10-15`, and the repository was **archived**
(read-only). The local working copy was then deleted. This page covers what a clean machine needs
that the clone does not contain. Read `docs/CHECKPOINT-2026-10-15.md` first. It is the
authoritative state; this page only adds recovery steps.

## 0. Unfreeze

```sh
gh repo unarchive Bonorinoa/projectos      # only when work actually resumes
git clone https://github.com/Bonorinoa/projectos.git && cd projectos
git checkout checkpoint-2026-10-15         # or main, if nothing landed after the freeze
```

## 1. Not in git, and how to regenerate it

| Missing | Size at freeze | Recovery |
|---|---|---|
| `app/src-tauri/target`, `kernel/target`, `spikes/*/target` | ~7.7 GB | Rebuilt by `cargo` (toolchain 1.97.1, `--locked`). |
| `spikes/gate-b/.venv`, `spikes/gate-a/node_modules` | ~180 MB | Recreated by the spike runners or their READMEs. |
| `spikes/gate-b/work/`, `spikes/gate-c2/probe-wasm/work/` | ~12 MB | Recreated by each runner (see its README). |
| `spikes/gate-c/build/` (incl. `build/runtime/node`) | 108 MB | See §2. The suite only writes beneath `build/` and `evidence/gates/gate-c/`. |
| Isolated toolchain dirs `/private/tmp/projectos-gate-a-{cargo,rustup}` | — | Referenced by older evidence logs only. Recreate with `CARGO_HOME`/`RUSTUP_HOME` pointing there and `rustup toolchain install 1.97.1`, or use a normal rustup home with `rustup run 1.97.1`. |

Absolute `/Users/bonorinoa/...` paths inside historical evidence logs are a record of where the
evidence was produced. They are not live configuration.

## 2. Gate C pinned Node runtime

`spikes/gate-c/runtime-pin.json` pins Node **v22.22.3 darwin-arm64**, binary SHA-256
`5d9d3872911e2340a43b707962e68143de8a4e8d54628845c0c4f2de1fb7cd5c`. At the freeze, the official
upstream tarball was checked to contain exactly this binary:

```sh
curl -fLO https://nodejs.org/dist/v22.22.3/node-v22.22.3-darwin-arm64.tar.gz
curl -fsL https://nodejs.org/dist/v22.22.3/SHASUMS256.txt | grep darwin-arm64.tar.gz
#   expect 0da7ff74ef8611328c8212f17943368713a2ad953fb7d89a8c8a0eae87c23207
shasum -a 256 node-v22.22.3-darwin-arm64.tar.gz
tar -xzf node-v22.22.3-darwin-arm64.tar.gz node-v22.22.3-darwin-arm64/bin/node
shasum -a 256 node-v22.22.3-darwin-arm64/bin/node    # must equal the pin
mkdir -p spikes/gate-c/build/runtime
cp node-v22.22.3-darwin-arm64/bin/node spikes/gate-c/build/runtime/node
```

Run the Gate C suite with that exact binary (`<pinned-node> spikes/gate-c/run-tests.mjs`). It fails
closed on any other runtime by design. This also resolves the pin's open "upstream release
checksum" provenance item for darwin-arm64.

## 3. Checkpoint verification

```sh
rustup toolchain install 1.97.1
export PATH="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH"   # or set PROJECTOS_CARGO
node scripts/verify-checkpoint.mjs     # exit 0 = kernel, native, node, syntax, diff-check, debug-build
```

At the freeze this passed (exit 0) in a **fresh clone** of `3a32ea4`. Expected side effect: the
script **rewrites** `evidence/m3/{kernel,native,node,debug-build}.txt` and
`evidence/m3/report.json`, so a dirty tree afterwards is normal. Commit those files only when you
mean to refresh the evidence. `node scripts/update-roadmap.mjs` stamps `updatedAt` on every run;
revert timestamp-only churn.

## 4. Other retained pieces

- First permanent project (After Dark Phoenix: git bundle, proposals, sidecar):
  `evidence/m3/first-project/README.md`.
- System Design DOCX builder: `evidence/m3/design/build_doc.py`. It ran from a `/private/tmp`
  scratch dir and reads a **Codex-plugin template** (`reference.docx`, SHA-256
  `13504f6c…1d76`). That template is third-party and is deliberately **not** committed. The
  generated DOCX (`docs/ProjectOS-M3-First-Project-System-Design.docx`) and its audits are the
  durable artifact. Re-running the builder needs that plugin template locally.
- `.impeccable/config.json`: design-linter suppressions that record the locked Synodos Field
  palette/font decision (`design/DECISION-canvas-visual-language.md`). The hook cache is ignored.
