# Fix review findings — code quality and architecture review of S3Dive

Objective: fix the defects and architectural gaps found in the code-quality and
architecture review of S3Dive, in priority order (P0 → P3).
Branch: `fix/review-findings` (from `main`).
Test runner: `cargo test --manifest-path src-tauri/core/Cargo.toml` (headless),
`cargo test --manifest-path src-tauri/Cargo.toml` (shell, needs the Tauri sysroot),
`pnpm typecheck` / `pnpm build` (frontend). Delivery strategy: work-unit commits on the
feature branch; PR creation stays the user's decision.

## Specs

- S1: The user's request, verbatim: "quiero arreglar estos puntos, cual es el siguiente paso
  recomendado?" — scope is every finding of the review of "the code quality and arquitecture
  of the project", in the priority order the review gave (P0, P1, P2, P3).
- S2: A remote must have an identity. Every `Remote` persisted in `state.json` carries a
  non-empty, unique `id`. Creating a second bucket must never replace the first one in the
  state, and must never repoint an existing job or mount at a different bucket. The duplicate
  name guard must keep working and refuse "A bucket named '{}' already exists.". `delete_remote`
  and `test_remote` must act on the bucket whose id they were given. A state file written by the
  buggy build (one remote with an empty id) must be repaired on load, not duplicated.
- S3: The Settings feature "Extra flags" must produce working rclone invocations. The example the
  UI teaches, "--transfers 8", must reach rclone as two arguments, never as one; rclone 1.75.2
  rejects the single-argument form with `unknown flag`. The app owns the config path, so
  "--config" and "--config=..." must be refused with an explanatory message. The validation
  message "'{flag}' is not an rclone flag. Extra flags must start with '-' (for example --transfers 8)."
  must stay accurate. The README sentence "extra flags applied to every rclone call" must match
  what the code actually does.
- S4: A cancelled or stopped child must stay killable until it is really dead. The promise
  "Quitting S3Dive terminates its child processes: running jobs and mounts are killed (SIGTERM,
  then SIGKILL after 3s), so nothing keeps deleting files after the window closes." must hold
  when the user cancels a run and then quits the app.
- S5: The app-owned `rclone.conf` and `state.json` must not disagree. If the state write fails
  after the config was written, the app must not keep a bucket whose credentials are missing, nor
  leave a credential with no bucket. A failed save must leave both files as they were.
- S6: Two concurrent writes to the same file must not share a temporary file. The temporary name
  of `write_private_file` must be unique per write.
- S7: The secret access key must be rejected when it contains control characters, the same way
  `access_key_id`, `region` and `endpoint` already are. A secret containing a newline must not be
  able to create extra sections in `rclone.conf`.
- S8: Every form field must have an associated label: `Field` accepts `htmlFor` and every caller
  must pass it, so clicking the label focuses the control and screen readers announce it. Modals
  must trap focus and restore it on close; `aria-modal="true"` must be true.
- S9: P3 cleanups: the store must refresh a finished job without re-running the whole
  `bootstrap` command (which re-reads the state file and runs `rclone version`); `platform` and
  `RcloneInfo.source` must cross the IPC boundary as typed values instead of debug strings; the
  redundant `src-tauri/core/Cargo.lock` must go; dead API (`Remote::spec`,
  `JobKind::requires_direction`) must be removed or used.
- S10 (assumption, needs the user's decision): jobs and mounts should reference a remote by its
  `id` instead of by its mutable name, so renaming a bucket cannot silently retarget a transfer.
  This changes the persisted `state.json` format and therefore needs the user's decision; it is
  not started until the decision is taken.

## Tasks

| ID | Spec | Route | Commit | Status |
| --- | --- | --- | --- | --- |
| T1 | S2 | inline | pending | done: core identity API (`plan_remote_update` / `apply_remote_update`) + `ensure_ids` repair on load + 7 core tests |
| T2 | S2 | inline | pending | done: `save_remote` / `delete_remote` wiring + 3 shell tests |
| T3 | S3 | inline | pending | pending: core flag tokenizer + `base_args` + core tests |
| T4 | S3 | inline | pending | pending: Settings validation, UI hint, README wording |
| T5 | S4 | inline | pending | pending: keep the pid in the registry until the child is dead |
| T6 | S5 | inline | pending | pending: `rclone.conf` and `state.json` stay consistent |
| T7 | S6 | inline | pending | pending: unique temporary file per write |
| T8 | S7 | inline | pending | pending: reject control characters in the secret |
| T9 | S8 | inline | pending | pending: labels associated + modal focus trap |
| T10 | S9 | inline | pending | pending: P3 cleanups |
| T11 | S10 | inline | n/a | blocked: waits on the user's decision about the `state.json` schema migration |

## Log

L1 (verbatim request):

> quiero arreglar estos puntos, cual es el siguiente paso recomendado?

L2 (review context): the review that produced the findings was requested with "review the code
quality and arquitecture of the project". Both P0 defects were reproduced before the fix, not
read: a scratch program against the real `s3dive-core` crate showed the second bucket replacing
the first and repointing a job at it, and real rclone 1.75.2 rejected the single-argument
"--transfers 8" with exit code 2 and `unknown command "/home/me/Pictures"`.

L3 (classification): large. The resume test fails: 10 work units across the core, the shell and
the UI, plus one deferred schema decision; the requirements would lose fidelity if compacted.
Tracking lives in this document, mirrored to Engram under `odd/fix-review-findings/tasks`.

L4 (decision): T1 and T2 land as one work unit before the first commit, because the shell test
harness for `save_remote` depends on the core API that T1 introduces. The core owns the pure
state transitions (`plan` / `apply`) so they stay testable without a GUI, a display server or
rclone, which is the split the README already justifies.

L5 (work unit 1 — remote identity, RED → GREEN):

- RED, core: `a_legacy_record_without_an_id_is_repaired_on_load` failed with "the empty id
  survived" (64 passed, 1 failed).
- RED, shell: `saving_two_buckets_keeps_both` failed with "the second bucket replaced the
  first — left: 1, right: 2". This is the defect reproduced at the layer where it lived.
- GREEN: `s3dive-core` gained `remotes::plan_remote_update` (identity) and
  `remotes::apply_remote_update` (uniqueness + rename rewrites); `AppState::ensure_ids`
  repairs a state file written by the buggy build; `save_remote` and `delete_remote` now
  delegate to them through `save_remote_inner` / `delete_remote_inner`.
- Evidence: core 65 unit + 6 integration tests, shell 3 tests, all green; clippy `-D warnings`
  clean on both crates; `cargo fmt --check` clean.
- Side effect: the shell crate gained its first tests and `tempfile` as a dev-dependency, so
  the command layer is now testable without a webview.

L6 (decision, deferred): jobs and mounts still reference a remote by name, so a rename has to
rewrite them. S10 changes that to an id reference and therefore changes the persisted format;
it waits for the user's decision (T11).
