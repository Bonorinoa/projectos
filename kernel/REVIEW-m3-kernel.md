# M3 Kernel Review — reviewer note

Scope: `kernel/src/document.rs` (native `DocumentStore`) and
`kernel/src/proposals.rs` (`apply`/`Decision`/`ProposalKind`/`RevisionLog`),
reviewed against `docs/EDITOR-PROVIDER-IPC.md` and the recorded decisions
(recorded decisions D-M2-c: **no new crate dependencies**; **hashing stays
FNV-1a-64**, documented and non-cryptographic). This note accompanies the
reviewer-owned suite `kernel/tests/m3_adversarial.rs`. No existing source or
test file was modified.

## Result summary

The M3 claims held on every property **except one** (metadata scoping of the
`objects/` subtree). Full run of the reviewer suite: **17 of 18 tests pass;
1 fails deliberately** (`metadata_refuses_the_objects_subtree`). The failing
assertion is the concrete evidence behind Finding 1 below. All other suites
(`document`, `proposal_editor`, `m2_acceptance`) remain green and were not
modified or weakened.

## Findings

### 1. `write_metadata`/`read_metadata` do NOT refuse the `objects/` subtree — a real scope deviation

`kernel/src/document.rs` lines 294-301 (`metadata_path`):

```rust
fn metadata_path(rel: &str) -> Result<String> {
    parts(rel, true)?;
    if rel.starts_with(".projectos") || Path::new(rel).extension().and_then(|e| e.to_str()) != Some("toml") {
        return Err(...);
    }
    Ok(format!(".projectos/{rel}"))
}
```

`parts(rel, true)` accepts any relative component that does not start with a
`.` (the `.projectos` prefix is injected afterwards), and the only guard left
is "must end in `.toml`". Therefore a name such as `objects/probe.toml` is
accepted and expands to `.projectos/objects/probe.toml`. `write_metadata` will
create it (`create_at` with `mkdirat` for the `objects` dir) and
`read_metadata` will read it. The M3 claim — "write_metadata/read_metadata
... refuse draft paths, **objects/**, escapes, and symlinked targets" — is
false for TOML-named entries under `objects/`.

Impact assessment: **contained, LOW–MED severity.** It is not an escape — the
path stays inside `.projectos`, parent traversal is still rejected, and the
content-addressed blobs are 16-hex names with **no** `.toml` extension, so a
metadata write can never directly overwrite or forge a revision blob
(`read_revision_blob` also re-verifies `stable_hash(content) == hash` on
read). The concrete harm is scope pollution of a directory that is supposed to
hold only content-addressed objects, plus a violation of the stated contract.
Test: `metadata_refuses_the_objects_subtree` asserts refusal and **fails**
because the store permits it. Kept as the review's evidence; the fix is to
reject any `rel` whose first or any component is `objects` (or otherwise
restrict metadata writes to a whitelist of the `.projectos` sub-trees).

### 2. The `libc` crate was added — deviation from the recorded decision

D-M2-c says **no new crate dependencies**. `kernel/Cargo.toml` now lists
`libc = "0.2"` (line 15). This is the only new build dependency the M3 diff
introduced and it was not re-approved against the decision. Flag as a
deviation; either re-open the decision or remove the dependency.

Assessment of the unchecked `unsafe` syscall code that drove it
(`open_at`, `parent`, `replace`, `walk` in `document.rs`):

- `openat(..., O_CLOEXEC | O_NOFOLLOW)`: the name is a live NUL-terminated
  `CString` borrowed for the duration of the call; the directory fd is a valid,
  live `File::as_raw_fd`; on `fd < 0` nothing is owned, on success
  `File::from_raw_fd` takes unique ownership. Sound.
- `mkdirat` / `renameat` / `unlinkat`: same `CString` + live-dirfd discipline;
  `renameat`/`unlinkat` use the caller-provided `parent` fd (resolved
  `O_NOFOLLOW`), never a re-opened path. The temp sibling is created
  `O_EXCL` and unlinked on error only when it was created; on the success path
  `renameat` consumes it. Sound.
- `fdopendir`: on `NULL` the fd is explicitly `close`d (POSIX does not take
  ownership on failure); on success ownership transfers to the `DIR*` guarded
  by the `Stream` RAII `closedir`. The `d_name`/`CStr` read of a NUL-terminated
  dir entry is valid. The parent `DIR` is kept open across recursion (bounded
  by `depth <= 32`), which is fine and avoids fd pressure.

No pointer-safety defect was found; the unsafe is disciplined and localized to
one module. The concern is governance (an unapproved dependency), not the
soundness of the calls — but an audit gate should record the approval.

### 3. Judgment on the changed assertion in `kernel/tests/m2_acceptance.rs`

The implementer changed one assertion in `a1_substack_golden_path_offline`
(line 98) from comparing `rev.after_hash` to `proposal.after_hash` to comparing
it to `stable_hash(new_text)` (the freshly read new document).

**Verdict: the change is a correction, not a weakening — it does not weaken
the original M2 property; it is the only way to assert the actual documented
property.**

Reasoning: in `proposals.rs` the proposal's `after_hash` is
`stable_hash(proposed.as_bytes())` — the hash of the *replacement snippet*
only (line 91). The revision's `after_hash`, however, is
`stable_hash(new_text)` where `new_text` is the **whole** rebuilt document
(`text[..byte_start] + proposed + text[byte_end..]`, line 235). For a
subsection edit `revision.after_hash != proposal.after_hash` by construction
(confirmed by `proposal_editor.rs::subsection_revision_hashes_complete_document_and_preserves_before_image`,
which asserts `assert_ne!(rev.after_hash, p.after_hash)`). So the original
assertion (`assert_eq!(rev.after_hash, proposal.after_hash)`) could never have
passed against this implementation — the implementer corrected it to assert the
contract property named in `docs/EDITOR-PROVIDER-IPC.md` (line 38 of this note,
paraphrasing: "a replacement acceptance changes the selected bytes and the
revision `after_hash` equals `stable_hash(whole new document)`"). The reviewer
suite independently re-proves this in
`replacement_after_hash_is_the_hash_of_the_whole_new_document`.

### 4. Observation (not a contract violation; not tested)

`Proposal::new` derives `id` from `"{rel_path}\x1F{original}\x1F{proposed}"`
(no `kind`), so two proposals identical except for `kind`
(Replacement vs Annotation) share the same `id` and the same `after_hash`.
`apply`'s integrity recheck therefore does not bind `kind`. Because `proposed`
and the anchor's `original` are fully validated and the chosen `proposed` bytes
are the only thing ever written, this is not an exploitable path; it is noted
for anyone relying on a proposal `id` to distinguish kinds downstream.

## Per-file provenance (m3_adversarial.rs → claim attacked)

| Test | Claim attacked |
|---|---|
| `open_refuses_a_symlinked_root` | every descendant via `openat(O_NOFOLLOW)`; symlinked root refused |
| `root_swapped_for_symlink_fails_closed_and_touches_nothing_outside` | pinned store: root renamed+recreated; all ops fail closed, outside untouched |
| `subdirectory_swapped_for_symlink_is_refused_for_read` | directory-swap → refusals on read/create; outside untouched |
| `lexical_rejections_cover_...` | `..`, absolute, `./`, `a/../b`, `.projectos/x`, `a//b`, trailing, `C:/x` |
| `extensions_are_case_insensitive_...` | `.md`/`.markdown`/`.txt` only for drafts |
| `size_bounds_refuse_over_limit_...` | reads/saves over 1 MiB refused; exact limit legal |
| `invalid_utf8_document_is_refused_...` | invalid UTF-8 refused, no partial write |
| `failed_saves_leave_original_bytes_and_never_leak_a_temp_sibling` | no partial write; atomic write (no half-written draft / no temp) |
| `save_with_wrong_expected_hash_is_refused_and_bytes_untouched` | stale-write: wrong `expectedHash` refused, bytes untouched |
| `metadata_refuses_the_objects_subtree` | **FAILS** — `objects/` not refused (Finding 1) |
| `metadata_paths_are_scoped_and_escape_and_symlinked_targets_are_refused` | `.projectos`-relative only; drafts/escapes/symlinked targets refused |
| `replacement_after_hash_is_the_hash_of_the_whole_new_document` | replacement changes bytes; `after_hash == stable_hash(whole new doc)` |
| `annotation_acceptance_keeps_draft_unchanged_and_requires_a_fresh_document` | annotation leaves bytes unchanged, `accepted-annotation` receipt, no revision; stale → fail before mutation |
| `rejection_mutates_nothing_and_records_a_rejected_receipt` | reject mutates nothing + records `rejected` |
| `malformed_proposals_are_refused_before_mutation_...` | malformed proposal/anchor fail before mutation (Accept & Reject) |
| `before_images_are_retained_deduped_and_only_the_exact_hash_is_verified` | retained before-image content-addressed & retrievable |
| `corrupt_before_image_at_the_exact_hash_fails_before_any_mutation` | corrupt before-image fails BEFORE mutation |
| `an_accepted_replacement_cannot_be_reapplied_to_a_now_stale_document` | no double-apply; reapply of a consumed proposal is refused |

Deterministic-only; every test uses `tempfile::tempdir`; no timing dependence.

## Untested / not deterministically testable

- **TOCTOU in the final compare/rename window of `DocumentStore::save`.** The
  module doc (document.rs:5-7) explicitly documents this is *not* a lock
  against an uncooperative writer editing in the final compare/rename window. It
  cannot be made deterministic, so it is documented, not asserted.
- **Crash mid-write / disk-full partial states.** The atomic tmp+`renameat`
  design is verified behaviorally (no temp remnants) but true crash injection is
  out of scope for an offline unit suite.
- **Raw pointer soundness of the `libc` syscalls** is assessed by code review
  (Finding 2), not provable at runtime.
- **Cross-platform path semantics** (Windows separators/`C:` accept/reject) are
  exercised with explicit rejection inputs, but the suite runs on macOS only.

## Governance deviations checklist

- [x] `libc` added to `kernel/Cargo.toml` — **deviation** from D-M2-c "no new
  crate dependencies" (Finding 2). Unsafe is sound but the dependency was not
  re-approved.
- [x] Hashing stays FNV-1a-64 (`provider::stable_hash`) — **conforms**.
- [x] m2_acceptance assertion change — **conforms** (correction, not a
  weakening; Finding 3).
- [x] metadata `objects/` scope — **violated** (Finding 1), the one red test.

## Resolution (lead, applied after review)

Finding 1 was fixed in `kernel/src/document.rs::metadata_path`: any path carrying
an `objects` component is now refused, so `write_metadata`/`read_metadata` can no
longer reach `.projectos/objects/`. `metadata_refuses_the_objects_subtree` now
passes and the reviewer suite is **18/18 green** (kernel total **121/121**, 0
warnings). Three M3 `collapsible_if` clippy lints in `document.rs` were also
cleared — M3 files are clippy-clean; 11 pre-existing clippy warnings in M1/M2
files (`worker.rs`, `selection.rs`, `registration.rs`, `journal.rs`, and three
test files) remain and are recorded as a separate cleanup, not part of M3.

Finding 2 (the `libc` dependency, D-M2-c deviation) is carried to
`docs/adr-kernel-native-fs-syscalls.md` as a **Proposed** ADR awaiting owner
approval; Finding 3 (the `m2_acceptance.rs` assertion) stands as a confirmed
correction, not a weakening. Finding 4 remains an untested observation.