# Fix review findings — code quality and architecture review of S3Dive

Objective: fix the defects and architectural gaps found in the code-quality and
architecture review of S3Dive, in priority order (P0 → P3).
Branch: `fix/review-findings` (work unit 1). Delivery strategy:
`chain_strategy=feature-branch-chain` — one branch per work unit, stacked on the previous
one; PR creation stays the user's decision. Test runner: `cargo test --manifest-path
src-tauri/core/Cargo.toml` (headless), `cargo test --manifest-path src-tauri/Cargo.toml`
(shell, needs the Tauri sysroot), `pnpm typecheck` / `pnpm build` (frontend).

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
- S10 (accepted, user decision 2026-10-23): jobs and mounts must reference a remote by its
  `id` instead of by its mutable name, so renaming a bucket cannot silently retarget a
  transfer. The persisted `state.json` format changes, so the state file needs a migration and
  a new schema version. A job or mount whose remote no longer exists must be reported, not
  silently re-pointed.

## Tasks

| ID | Spec | Route | Commit | Status |
| --- | --- | --- | --- | --- |
| T1 | S2 | inline | 5c7665e, eeef3a0 | done: core identity API (`plan_remote_update` / `apply_remote_update`) + `ensure_ids` repair on load + 7 core tests |
| T2 | S2 | inline | eeef3a0 | done: `save_remote` / `delete_remote` wiring + 3 shell tests |
| T3 | S3 | inline | 3d68e5f | done: core flag tokenizer (`split_flag_line`) + `validate::extra_flags` + `base_args` + 10 core tests |
| T4 | S3 | inline | 3d68e5f | done: `save_settings` calls the core validation, Settings hint and README wording fixed |
| T5 | S4 | inline | pending | pending: keep the pid in the registry until the child is dead |
| T6 | S5 | inline | pending | pending: `rclone.conf` and `state.json` stay consistent |
| T7 | S6 | inline | pending | pending: unique temporary file per write |
| T8 | S7 | inline | pending | pending: reject control characters in the secret |
| T9 | S8 | inline | pending | pending: labels associated + modal focus trap |
| T10 | S9 | inline | pending | pending: P3 cleanups |
| T11 | S10 | inline | pending | pending: `remoteId` on jobs and mounts, schema v2 + migration in the core |
| T12 | S10 | inline | pending | pending: commands and rclone arg builders resolve the remote by id |
| T13 | S10 | inline | pending | pending: the UI sends and shows `remoteId` instead of the name |

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

L7 (user decisions, 2026-10-23):

- Delivery: "Rama por feature encadenada" — `chain_strategy=feature-branch-chain`. One branch
  per work unit, stacked on the previous one. Slice boundaries: `main` → `fix/review-findings`
  (work unit 1, remote identity) → `fix/extra-flags` (work unit 2) → `fix/process-lifecycle`
  (work unit 3) → `fix/remote-references` (work unit 4, the S10 migration) →
  `fix/validation-and-a11y` (work unit 5) → `fix/cleanup` (work unit 6).
- Migration: "Sí, migrar ahora" — S10 is accepted. Jobs and mounts reference a remote by id;
  `state.json` gets a new schema version and a migration. T11-T13 replace the blocked task.

L8 (work unit 1, delivered): 625 authored lines in two commits (`5c7665e` rustfmt,
`eeef3a0` the fix). Past the ~400-line slice, so the chain strategy applies from the next
commit.

L9 (design for work units 3 and 4, decided while work unit 2 was in flight):

- T5 (S4): `RunEntry.cancel` and `MountEntry.cancel` become `Option<oneshot::Sender<()>>`.
  `cancel`/`stop` take the sender out of the entry but leave the entry in the registry, so
  the pid stays killable until the supervisor sees the child exit and removes it. The
  supervisor already removes the entry when the child is gone, so nothing else changes.
- T6 (S5): one helper writes the config file and the state together. The config is written
  first; when the state write fails, the previous config content is written back and the
  error says so if that rollback also fails. Applied to `save_remote` and `delete_remote`.
- T7 (S6): `store::temp_path` gets a per-write counter (an `AtomicU64` plus the pid)
  instead of the pid alone, so two concurrent writes never share a temporary file.
- T11-T13 (S10): `Job` and `Mount` carry `remote_id` instead of the remote name. The
  pre-v2 `remote` name stays as a read-only `#[serde(default, rename = "remote",
  skip_serializing)]` field, used only to resolve the id when an old state file is loaded;
  `STATE_VERSION` goes to 2 and `load_state` performs the migration. A reference whose
  remote no longer exists keeps an empty `remote_id` and is reported, never re-pointed.
  `transfer_args` and `mount_args` take the remote name as an argument, because the job no
  longer carries it. `apply_remote_update` loses the rename rewrite: with ids, a rename
  cannot move a job.

L10 (work unit 2 — extra flags, RED → GREEN, delivered in `3d68e5f`):

- Delegated the core half to a bounded writer (the tokenizer, the validation API and the
  core tests); the parent did the shell wiring, the Settings hint and the README. The
  writer's brief fixed the exact API signatures, so the two halves met without a
  negotiation round.
- RED: the argv carried one element, `["--config", ..., "--transfers 8", "copy", ...]`;
  `bandwidth_limit_and_global_flags_apply_to_every_run` failed with the left/right
  difference, and the new test failed with 64 passed, 2 failed.
- GREEN: core 70 unit + 5 + 6 integration tests, shell 3 tests, all green; clippy
  `-D warnings` clean on both crates; `cargo fmt --check` clean; `pnpm build` green.
- End-to-end proof against real rclone 1.75.2: the argv is now
  `["--config", CONF, "--transfers", "8", "sync", ...]` and rclone parses it, failing only
  for the expected missing config section instead of `unknown command`.
- The writer's differential check compared eight argv scenarios against the base commit:
  the only differences are the two intended ones.
- Parent hardening after the handoff: `base_args` now drops a config flag that reaches it
  through a hand-edited state file, together with its value, so the app config path cannot
  be redirected. The writer had flagged that only `save_settings` gated it.

L11 (delivery): 911 authored lines across three commits (`main` -> `fix/review-findings` ->
`fix/extra-flags`). The chain strategy applies: the next work unit starts on
`fix/process-lifecycle`, stacked on `fix/extra-flags`.
