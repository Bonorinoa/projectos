# ProjectOS Gate A desktop-shell spike

This directory is an isolated feasibility rig for Architecture Gate A. It is not the ProjectOS application and does not establish that Gate A passed.

The rig implements one deliberately small Tauri 2.12.0 window with:

- a Rust-created native menu;
- browser-native file selection and drag/drop;
- clipboard and IME event probes;
- Markdown/LaTeX source editing;
- inert, text-node-only display of untrusted Markdown, HTML, LaTeX, and PDF-derived text;
- an explicit Rust navigation allowlist plus a strict CSP with no remote assets;
- a cancellable IPC command with bounded inputs;
- a 10,000-row virtual list;
- keyboard/focus, accessibility-tree, and 200% zoom inspection surfaces;
- no filesystem, network, shell, Git, credential, provider, process, or asset-protocol capability.

## Trust boundary

The frontend is untrusted presentation code. It receives no ambient privileged API. Three custom commands are exposed: read inert shell metadata, start the bounded cancellation probe, and cancel that probe. The Rust navigation handler accepts only the packaged local origin. Documents are displayed with `document.createTextNode`; rich HTML is never executed. This spike intentionally does not solve production Markdown, LaTeX, or PDF rendering.

## Prerequisites

The pinned toolchain baseline is:

- Node.js 22 or compatible;
- Rust 1.90 or newer;
- Tauri CLI 2.12.0 (installed locally by `npm ci` once a reviewed lockfile exists);
- platform prerequisites from the official Tauri documentation;
- on Linux, the exact WebKitGTK package frozen in `platform-matrix.json`.

The 2026-10-08 run used an isolated Rust 1.90 toolchain in `/private/tmp`, with Node 22.22.3. The native macOS arm64 debug app compiled, bundled, and launched. File opening, inert HTML display, named accessibility controls and real IPC cancellation were observed on this developer machine. The full frozen platform matrix remains open.

## Safe static checks

From this directory:

```sh
npm test
npm run verify
```

These require no installed packages and no network. They validate hostile fixtures, the text-only rendering contract, navigation policy, dependency pins, CSP, capability surface, frozen matrix shape, and evidence-report completeness. They are source-level evidence only.

## Native build and interactive run

After installing the prerequisites on a disposable or clean matrix machine:

```sh
npm ci --ignore-scripts
npm run dev
```

After the first dependency resolution has been reviewed, check in the generated `package-lock.json`, then use `npm ci` for every evidence run. Build packages with:

```sh
npm run build
```

Both dependency lockfiles now exist. Record the resolved Cargo graph with `cargo metadata --locked`; do not substitute floating dependencies.

To regenerate source-hashed build/test evidence from the repository root:

```sh
PROJECTOS_CARGO_HOME=/private/tmp/projectos-gate-a-cargo PROJECTOS_RUST_TOOLCHAIN_HOME=/private/tmp/projectos-gate-a-rustup node spikes/gate-a/tests/native-evidence.mjs
```

Omit these optional variables when Rust is installed normally. The native runner does not perform or claim the GUI observations: those are separately retained in `evidence/gates/gate-a/host-ui-observations.json`. It runs three Rust regression tests, hostile-fixture tests, static configuration checks and a debug app bundle build. Duplicate IPC request IDs preserve the original cancellation handle; concurrent probes are capped at sixteen.

## Evidence run

For each target in `platform-matrix.json`:

1. Start from a clean virtual or physical machine without developer packages.
2. Record OS release, kernel, architecture, desktop, session, WebKitGTK/WKWebView, display server, and GPU.
3. Build Linux artifacts on `ubuntu-22.04.5-x86_64-x11` first.
4. Install the produced package as an ordinary user.
5. Execute every step in `manual-test-protocol.md` in both default and no-GPU modes.
6. Save terminal logs, package hashes, screenshots/video, accessibility-inspector exports, and pass/fail observations under `evidence/gates/gate-a/<target-id>/`.
7. Update `evidence/gates/gate-a/report.json` only from those artifacts.

Signing and notarization require explicit release credentials and cannot be inferred from an unsigned local bundle. Linux package installation cannot be inferred from a macOS build.

## Decision rule

Gate A passes only when every required criterion passes on every frozen target. One untested criterion keeps the gate open. A critical accessibility, packaging, rendering, or stability defect without a bounded workaround fails the gate and triggers the documented Electron comparison; the project must not maintain both shells.
