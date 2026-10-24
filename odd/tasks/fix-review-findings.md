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
| T5 | S4 | inline | 397937d | done: `cancel`/`stop` take the sender, the entry stays in the registry until the child is dead + 4 shell tests |
| T6 | S5 | inline | 397937d | done: `commit_remote_change` writes the config and the state together and restores the config when the state write fails + 1 shell test |
| T7 | S6 | inline | 397937d | done: every write gets its own temporary file + 2 core tests |
| T8 | S7 | inline | 8b37719 | done: `validate::secret_access_key` refuses control characters in the secret + 2 tests |
| T9 | S8 | inline | 8b37719 | done: `useDialogFocus` traps and restores the focus, every field has an associated label, the Direction group is a fieldset with real radios |
| T10 | S9 | inline | dbecbbc | done: the store updates a finished job in place, `platform` and `source` are typed enums, one lockfile, dead API removed |
| T11 | S10 | inline | c793707 | done: `remoteId` on jobs and mounts, schema v2 + load-time migration in the core + 3 migration tests |
| T12 | S10 | inline | c793707 | done: commands, arg builders and the shell resolve the remote by id |
| T13 | S10 | inline | c793707 | done: the UI picks the bucket by id, shows the resolved name and reports dangling references |

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

L12 (work unit 3 — process lifecycle, done inline because it is the highest-risk unit and
the design was already fixed in L9):

- T5: `RunEntry.cancel` and `MountEntry.cancel` became `Option<oneshot::Sender<()>>`. `cancel`
  and `stop` take the sender out of the entry and leave the entry in the registry; the
  supervisor removes it when the child is really gone. The pid therefore stays killable.
  Tests: `a_cancelled_run_stays_killable`, `a_stopped_mount_stays_killable`.
- T6: `commit_remote_change` writes the config and the state together. The config is written
  first; when the state write fails, the previous config content is written back and the
  error says so when even that fails. Test: `a_failed_state_write_restores_the_config`
  (the state path is turned into a directory, so the atomic rename cannot land).
- T7: `store::temp_path` adds a per-write counter to the pid. Tests: `temporary_names_never_repeat`
  and `concurrent_writes_to_the_same_file_both_land`.
- Evidence: core 72 unit + 5 + 6 integration tests, shell 8 tests, all green; clippy
  `-D warnings` clean on both crates; `cargo fmt --check` clean; `pnpm build` green.
- Correction made during the unit: the first version of the rollback test created a directory
  where `state.json` already existed as a file, so the test failed on its own setup, not on
  the behaviour. Fixed by removing the file first.

L13 (work unit 4 — remote references by id, delivered in `c793707`):

- The model change: `Job` and `Mount` carry `remoteId`; the pre-v2 name is read as
  `legacy_remote` (`#[serde(default, rename = "remote", skip_serializing)]`) and is never
  written back. `STATE_VERSION` is 2.
- The migration lives in `AppState::migrate_references`, called by `load_state`. It resolves
  the old names into ids and leaves a reference whose bucket is gone with an empty id, so it
  is reported by `AppState::dangling_references` (surfaced by `bootstrap` and a banner in the
  UI) and never re-pointed at another bucket.
- `transfer_args` and `mount_args` take the remote name as an argument now: the job carries
  the id, and the command layer resolves it before building the command line.
- `apply_remote_update` lost the rename rewrite. With ids, a rename changes the name and the
  jobs follow the bucket, because they still point at its id.
- New core tests: `an_old_state_file_names_its_remotes_and_is_migrated`,
  `an_old_state_file_with_a_missing_remote_is_reported_not_repointed`,
  `the_saved_state_references_a_remote_by_id`.
- End-to-end proof with a scratch program against the real crate: a schema v1 file loads as
  v2 with the ids resolved, a rename keeps the job on the same bucket, and the saved file
  carries `remoteId` and no `remote`.
- Evidence: core 75 unit + 5 + 6 integration tests, shell 8 tests, all green; clippy
  `-D warnings` clean on both crates; `cargo fmt --check` clean; `pnpm build` green.
- Defect caught while reviewing the UI diff, invisible to the type checker because both are
  strings: the "New job" and "New mount" buttons still passed the bucket NAME where the id
  was expected, which would have made every new job point at a bucket that does not exist.
  Fixed before the commit.
- Correction made during the unit: the first `useState<string[]>([])` in `store.tsx` was
  written with a stray brace (`useState<string[]>([]});`), which is a parse error in TSX.
  Rewritten as `useState<Array<string>>([])`.

L14 (work unit 5 — validation and accessibility, in flight):

- T11: `validate::secret_access_key` refuses an empty secret and any control character in
  it. The check runs on the secret the user typed, before the config file is written, so a
  newline can no longer end the section and start another one. A secret read back from the
  file is not re-validated: it is already in the file, and refusing it would lock the user
  out of the bucket.
  Tests: `a_secret_cannot_break_the_config_format` (core) and
  `a_secret_with_a_newline_cannot_inject_a_config_section` (shell, asserts that neither the
  section nor the bucket reached the disk).
- T12: `useDialogFocus` in `src/components/ui.tsx` keeps Tab inside the dialog, moves the
  focus to the dialog when it opens and gives it back to the control that opened it. Both
  `Modal` and `ConfirmDialog` use it, so `aria-modal="true"` is now true of both.
- The label wiring (every `Field` gets an `htmlFor` and its control an `id`) was delegated
  to a bounded writer with the exact id scheme, while the parent did the focus trap and the
  validation.
- The writer flagged the one `Field` it could not wire: "Direction" wraps two buttons, not
  a form control, so a `htmlFor` would point at nothing. The parent replaced it with a
  `fieldset` and two real radio inputs, which gives the group a name and the arrow keys for
  free. All 20 `Field` usages are now associated (verified: 20 `htmlFor`, no orphan).

L15 (work unit 6 — P3 cleanups, planned):

- `job:finished` updates the job in the store from the payload it already carries, instead
  of re-running `bootstrap`, which re-read the state file and spawned `rclone version`.
- `platform` and `RcloneInfo.source` become typed enums on the wire instead of debug
  strings, and the TypeScript side stops widening `source` with `| string`.
- `src-tauri/Cargo.toml` becomes the workspace root with `core` as a member, so there is one
  lockfile instead of two and the core's standalone test run stops creating its own.
- `Remote::spec` and `AppState::find_remote` are removed (both dead after the id change) and
  `JobKind::requires_direction` gets a caller: a bisync job always stores the default
  direction.

L16 (work unit 6 — P3 cleanups, delivered in `dbecbbc`):

- `job:finished` updates the job in the store from the payload it carries. The backend
  writes the summary to the state file before it announces the end of the run, so the
  store no longer re-runs `bootstrap` (which re-read the file and spawned `rclone version`).
- `platform` and the rclone `source` are typed enums on the wire. The TypeScript side lost
  the `| string` that made the `source` union meaningless.
- `src-tauri` is now the workspace root with `core` as a member: one lockfile covers both
  crates and the standalone core test run no longer creates a second one.
- `Remote::spec` and `AppState::find_remote` are removed (dead), and
  `JobKind::requires_direction` has a caller.

L17 (delivery summary): seven commits, one per work unit, each on its own branch stacked on
the previous one, per the chain strategy the user chose:

| Branch | Commit | Unit |
| --- | --- | --- |
| `fix/review-findings` | `5c7665e`, `eeef3a0` | remote identity |
| `fix/extra-flags` | `3d68e5f` | extra rclone flags |
| `fix/process-lifecycle` | `397937d` | process lifecycle |
| `fix/remote-references` | `c793707` | buckets referenced by id |
| `fix/validation-and-a11y` | `8b37719` | secret validation + accessibility |
| `fix/cleanup` | `dbecbbc` | P3 cleanups |

Total: 2137 authored lines, 7 commits, 10 files touched outside the UI, 5 UI files.
Final state of the checks: core 76 unit + 5 + 6 integration tests, shell 9 tests, all green;
clippy `-D warnings` clean on both crates; `cargo fmt --check` clean; `pnpm typecheck` and
`pnpm build` green.

L18 (known gap, not fixed): the frontend still has no test runner. The label association and
the focus trap are verified by typecheck, build and static inspection only. Adding vitest
means a new dependency and a new toolchain, which is a decision for the user.

L19 (independent verification and the native review):

- The assess over `main..HEAD` returned tier `medium` with `reviewDue: false`. The change
  touches a Task Size high-risk item (a persisted-format migration), so the assess was run
  again with `escalate` on item 1, which raised the tier to `high` and added an independent
  verifier to the plan.
- The independent verifier found ONE blocker (B1): `validate::extra_flags` only looked at
  the first token of a line, so `--transfers 8 --config /etc/rclone.conf` was accepted with
  a green save and no explanation, and the flag was dropped later when the command line was
  built. S3 says the config flag must be refused with an explanatory message. One
  correction batch fixed it (the validation now scans every token), with a RED test first
  (2 failed, 5 passed) and GREEN after. The re-verification confirmed B1 fixed and the
  safety property intact, including the quoted-value shape (advisory A4) that is now
  refused at validation. Commit `e105b09`.
- The same correction promoted the verifier's probes to permanent regression tests: the
  state migration (6 tests in `core/tests/state_migration.rs`), the config and state
  agreement (3 tests), the rename (1 test) and the kill path (2 tests that spawn a real
  child). The verifier's other findings were advisories: the SIGKILL escalation thread
  dies with the app (pre-existing, rclone handles SIGTERM), the microsecond window between
  the spawn and the pid registration (pre-existing), a secret read back from the config is
  not re-validated (documented in L14), a name that does not match exactly is reported
  instead of guessed (the safe behaviour S10 asks for), and the missing frontend test
  runner (L18).
- Native review: the lineage `review-685bfcd291740861` was created over `main..HEAD`
  (28 paths, 2865 lines, tier high, lenses risk/resilience/readability/reliability) and is
  in the `reviewing` state. The capture route refuses the provider-issued collect bindings
  in every documented shape: the group with serialized strings, the group with the
  `{collectBinding: ...}` objects, a single slot with the object and a single slot with the
  string. The refusals are `collectBindings must be the complete distinct current reviewer
  group in exact provider order` and `collectBinding is missing or stale for current
  STATUS`, while the binding was copied verbatim from the current STATUS. Every attempt
  reported `mutation_performed: false`, so nothing was mutated and no authority was
  consumed; the lineage stays in `reviewing` and can be resumed.
- The reviewer models are configured (`~/.pi/gentle-ai/models.json` holds the four lenses
  plus `review-refuter` and `review-validator`), so this is not the relay configuration
  cause recorded in L11.
- The native review is an outside view and never replaces the tier's verification. That
  verification is done: the independent verifier, the correction and its re-verification,
  the full test suite and the spot check all passed.
