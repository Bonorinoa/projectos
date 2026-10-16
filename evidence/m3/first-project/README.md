# First permanent project — After Dark Phoenix (retained 2026-10-15)

The M3 checkpoint (`docs/CHECKPOINT-2026-10-15.md`) names After Dark Phoenix as ProjectOS's first
permanent Software project. It had no remote. Before the local checkout was deleted it was folded
in here, unchanged, so the checkpoint stays reconstructable from GitHub alone.

| Path | What it is |
|---|---|
| `afterdarkphoenix.bundle` | `git bundle --all` of the project. One commit, `6ff1b97f2b2672f739d91ddbedb863fd21c0ba39` (branch `main`, "Bootstrap After Dark Phoenix project"). |
| `proposals/AfterDarkPhoenix-review.patch` | The durable proposal: 1,305-word "Heat After Sunset" article, outline status, research-blocker audit. **Never applied.** Its SHA-256 matches the exported-file hash recorded in `../first-project-integrity.json`. |
| `proposals/AfterDarkPhoenix-review-initial-national.patch`, `…-ledger-initial.patch` | Intermediate exports, retained per the checkpoint, not deleted. |
| `sidecar-projectos/` | The project's **git-ignored** `.projectos/` runtime metadata (manifest, Software receipts, graph snapshot). Deliberately renamed from `.projectos` so that neither ProjectOS nor git treats this evidence folder as a live project. |

## SHA-256

```
88498852ea7837fb8e9b34fb5b604ff71e658e1c4c969cf39f19d5199a7dab59  afterdarkphoenix.bundle
2cff69a7af39902b054ea5acfb9dcbb4a7a6b05f1e149c1ecea51479e75ccd9c  proposals/AfterDarkPhoenix-review.patch
6ea1f92e9ce3c4d7f713781479a6c86b62e4d5dffd018318c078632484a3e3b2  proposals/AfterDarkPhoenix-review-initial-national.patch
49ce774917afed944cba7e421f6ae9fcabc2140ce09964a6136a6fd7d2c7d65d  proposals/AfterDarkPhoenix-review-ledger-initial.patch
0e4725d25564c20a6edafdc95f5a917a24ed2f6b12a01a3147bb0a998cc3c7ce  sidecar-projectos/project.toml
e40aa899ce3ceed703f45d40b41744d4193606cc9817d466ec4b860c4dc258b5  sidecar-projectos/software-receipts.toml
2f05ffa8f9d53b21a1a5355965029f8bead59918e40dabfba29d2d085404e672  sidecar-projectos/graph/snapshot.toml
```

## Restore

```sh
git clone evidence/m3/first-project/afterdarkphoenix.bundle ~/somewhere/AfterDarkPhoenix
cd ~/somewhere/AfterDarkPhoenix
git rev-parse HEAD     # must print 6ff1b97f2b2672f739d91ddbedb863fd21c0ba39
mkdir -p .projectos/graph
cp <repo>/evidence/m3/first-project/sidecar-projectos/*.toml .projectos/
cp <repo>/evidence/m3/first-project/sidecar-projectos/graph/snapshot.toml .projectos/graph/
git apply --check --whitespace=error <repo>/evidence/m3/first-project/proposals/AfterDarkPhoenix-review.patch
```

`git apply --check` passed against the bundle on 2026-10-15. **Do not apply the proposal
automatically.** Applying it is an owner decision (see the checkpoint). The sidecar's
`project_uuid` is the project's identity, so restore it before you reopen the folder in ProjectOS.
Native run IDs and staging worktrees were session-owned and are gone. On resume, create a fresh
managed worktree, as the checkpoint describes.
