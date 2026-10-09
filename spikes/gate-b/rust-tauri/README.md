# Helix Rust/Tauri embedding probe

This pins Helix source `c72e64f0c7e480f3d125f071659d5e3ad7b8da15` with its
`embedded` feature. The published `helix-db = 3.0.0` registry archive has no
embedded feature, despite current docs describing one. The pinned source
requires its official Rust 1.97.1 toolchain for a parent-module alias which
fails with Rust 1.90. The lockfile pins the upstream SlateDB fork and all
registry dependencies. No upstream source was patched.

With Rust 1.97.1 installed, from this folder:

```sh
cargo build --locked --features desktop
./target/debug/projectos-graph-embedding --contract
./target/debug/projectos-graph-embedding
```

The CLI runs the same engine code exposed by the Tauri `graph_probe` command:
six checks for exact one/two-hop traversals, update, delete, disk reopening and
read-only write rejection. All graph storage is in a private temporary folder.
The command accepts no external path, URL, query or credentials. The app button
returns the contract receipt through real Tauri IPC. There is no HTTP server or
sidecar database. A successful build alone does not establish UI IPC behavior;
the GUI observation is recorded separately.

After initial dependencies are cached, reproduce hashed evidence offline:

```sh
PROJECTOS_CARGO_HOME=/private/tmp/projectos-gate-a-cargo PROJECTOS_RUST_TOOLCHAIN_HOME=/private/tmp/projectos-gate-a-rustup node evidence.mjs
```

These environment variables select the existing isolated toolchain and can be
omitted when Cargo/Rust are installed normally. The runner records build,
contract output, dynamic linkage, exact source/binary hashes and dependency
license metadata. The inventory is not a license approval. Clean-machine,
universal and Linux packages, signing, full Rust recovery/export adapter,
concurrency and a production-scale corpus remain open. Helix is not selected
as the product engine; Ladybug remains eligible.
