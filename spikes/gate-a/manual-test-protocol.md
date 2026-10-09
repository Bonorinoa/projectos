# Gate A manual test protocol

Use this protocol unchanged on every entry in `platform-matrix.json`. Evidence IDs should be `<target-id>/<criterion-id>/<timestamp>`.

## 0. Preflight

- Verify the machine matches the frozen OS, architecture, desktop, display server, WebView, and package format.
- Verify it began without developer packages; preserve the package-manager transaction that installs build prerequisites.
- Record `npm --version`, `node --version`, `cargo --version`, `rustc --version`, and Tauri CLI output.
- Build Linux on the oldest Linux target first. Record package SHA-256 hashes.

## 1. Install, launch, and native surface

- Install each target package through its normal user-facing path. Launch from the desktop environment, not a development terminal.
- Verify signing and notarization with the platform-native inspection tools. Unsigned local success is not a substitute.
- Confirm the app menu is native and exposes the expected edit commands.
- Open the file picker with keyboard and pointer. Select each hostile fixture.
- Drag/drop each hostile fixture onto the indicated zone.
- Repeat launch in the target's default and no-GPU profiles.

## 2. Editing and input

- In the editor, type and revise Markdown plus LaTeX source.
- Exercise copy and paste through keyboard shortcuts, native menu items, and buttons.
- Use at least Japanese, Chinese, Korean, Arabic, and a dead-key/compose sequence. Capture composition events and final text.
- Load `extracted-pdf-text.txt`, select the expected phrase, and copy it. Separately open the hostile `.pdf`; confirm PDF bytes are withheld and no action runs.

## 3. Hostile-document containment

- Load every file under `fixtures/hostile/`.
- Confirm source remains visible as inert selectable text.
- Use a network monitor to confirm zero remote requests.
- Confirm no new window, external navigation, form submission, script execution, shell/process, filesystem, credential, or privileged IPC occurs.
- Attempt every URL and active-content payload present in the fixtures. Preserve console/CSP output.

## 4. Responsiveness and accessibility

- Scroll the 10,000-row list from start to end repeatedly in both GPU modes; record frame stability, hangs, and memory.
- At 100% and 200% zoom, verify no control becomes unreachable or obscured and no page-level horizontal scroll appears.
- Navigate the whole interface using keyboard only. Confirm visible focus and logical order.
- Inspect the WebView accessibility tree with VoiceOver on macOS and Orca/Accerciser on Linux. Record landmark, heading, label, description, role, state, and live-region output.

## 5. IPC cancellation and recovery

- Start the five-second IPC probe and cancel near the beginning, middle, and end.
- Confirm the Rust command reports cancellation and the UI remains responsive.
- Start another request with the same ID while active through developer instrumentation; confirm rejection.
- Close the window during an active request, relaunch, and confirm no orphan process or stuck state remains.

## 6. Result classification

- `pass`: executed on the exact frozen target with a linked artifact demonstrating the expected result.
- `fail`: executed and did not meet the expected result; record severity and whether a bounded workaround exists.
- `untested`: not executed, wrong target/version, missing artifact, or only source/static evidence exists.

Do not average results across targets. Gate A is open if any required cell is untested and fails if any critical defect lacks a bounded workaround.
