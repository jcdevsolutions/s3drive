# S3Dive v1 — Tauri v2 visual interface for rclone

Cross-platform (macOS, Windows, Linux) desktop app for configuring S3 remotes, running
sync/copy/bisync jobs between buckets and local folders, and mounting buckets with
auto-mount at OS login.

## Specs

- S1: "a desktop app that runs on Mac, Windows and Linux using Tauri(v2)" — one codebase,
  Tauri v2 shell, buildable on all three targets.
- S2: "acts as a visual interface for rclone" — the app drives the user's installed
  `rclone` binary; it does not reimplement the S3 protocol.
- S3: "so the user can configure s3 buckets" — create/edit/delete/test S3 remotes, written
  to an app-owned, rclone-compatible `rclone.conf`.
- S4: "select the folders they want to sync" — pick a local folder and a remote
  prefix, then run `sync`, `copy`, or `bisync` with live log output.
- S5: "the option to auto mount on os reboot" — per-mount auto-start flag; the app
  registers itself at OS login and starts flagged mounts on launch.
- S6 (from L2, decision): v1 exposes mirror (`sync`), download (`copy`) and two-way
  (`bisync`) semantics.
- S7 (from L2, decision): S3 credentials live in an app-owned `rclone.conf`; secrets are
  obscured by rclone itself, never stored in app state, and the file is mode 0600.
- S8 (from L2, decision): FUSE / WinFsp is detected at runtime; missing dependencies
  block mounting with an actionable message and guided install instructions.

## Tasks

| ID | Spec | Route | Commit | Status |
| --- | --- | --- | --- | --- |
| T1 | S1 | scaffold Vite+React+TS + Tauri v2 shell, icons, capabilities | n/a (no git repo) | done: `pnpm build` green, icons generated |
| T2 | S2, S7 | `s3dive-core` crate: model, rclone arg builders, parsers, store | n/a | done: 50 tests, clippy `-D warnings` clean |
| T3 | S2, S3, S4, S5, S8 | Tauri command layer, process supervision, events | n/a | done; shell compile check blocked, see L4 |
| T4 | S3 | Frontend shell, settings, remotes (buckets) screens | n/a | done: typecheck + build green |
| T5 | S4 | Jobs screen: folder pickers, run, live log | n/a | done |
| T6 | S5, S8 | Mounts screen, FUSE guidance, autostart wiring | n/a | done |
| T7 | all | Verification: cargo test/clippy, tsc, vite build, docs | n/a | done except the shell `cargo check` (L4) |

## Log

L1 (verbatim request):

> I want to create a a desktop app that runs on Mac, Windows and Linux using Tauri(v2) that acts as a visual interface for rclone, so the user can configure s3 buckets and select the folders they want to sync, also the option to auto mount on os reboot

L2 (clarification answers):

- Sync semantics: "Mirror, download, and two-way bisync" → S6.
- Credentials: "App-owned rclone.conf" → S7.
- Mount deps: "Detect and guide" → S8.
- Toolchain: "Yes, install rustup and build".

L3 (environment): rustc 1.99.1 installed via rustup. `libwebkit2gtk-4.1-dev`, `libxdo-dev`,
`libssl-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev` are absent and `sudo` requires
a password, so `cargo build` of the Tauri shell cannot run here. Mitigation: all logic lives
in the Tauri-free `s3dive-core` crate, which is unit-tested headlessly; the shell is verified
by frontend build plus review.

L4 (verification evidence):

- `cargo test --manifest-path src-tauri/core/Cargo.toml`: 50 passed, 0 failed.
- `cargo clippy --manifest-path src-tauri/core/Cargo.toml --all-targets -- -D warnings`: clean.
- `pnpm build` (tsc --noEmit + vite build): clean.
- `rustfmt --check src-tauri/src/*.rs`: clean, so every shell source parses.
- `cargo check` on the `s3dive` shell crate: attempted, failed inside `libdbus-sys` and
  `webkit2gtk-sys` build scripts (missing system dev libraries from L3) before reaching this
  crate's own code. Not a code failure, but the command layer is not compiler-verified here.

L6 (compile verification, after L4): the shell was compiler-verified after all, without root.

- A dev-package sysroot was built under `/tmp/tauri-sysroot` with `apt-get download` +
  `dpkg -x` (650 packages, recursive closure), and its `.pc` files rewritten to emit sysroot
  paths. The recipe is now in the README.
- `cargo check` in `src-tauri` exits 0 with zero warnings for both `src/lib.rs` and
  `src/main.rs`. Negative control: unsetting `PKG_CONFIG_PATH` fails in `glib-sys`, so the
  sysroot is genuinely what makes it compile.
- `cargo clippy` on the shell could not run in this environment (clippy-driver reports
  `can't find crate for std` while compiling the build script); the core crate's clippy run is
  clean and is the one that gates the code with real logic.

L7 (bugs found by review and by probing real rclone, all fixed):

- `rclone` retried an unreachable endpoint for 60s with no output, so `captured()` gained a 45s
  timeout plus a pid kill; otherwise "Test connection" and the bucket browser could hang forever.
- `tokio::select!` cannot mutate the child in the cancel arm while `child.wait()` holds the
  borrow; both supervisors now kill by pid inside the arm and await the child after.
- Job start could orphan an rclone child if persisting the run failed, and both supervisors
  had check-then-act races that allowed duplicate children. The job claim is now an atomic
  `HashSet` insert before spawn, and the mount entry is inserted under the same lock guard
  used for the duplicate check; every failure path releases its reservation.
- `find_in_path` accepted non-executable files on Unix; it now checks the executable bit.
- Verified against real rclone 1.75.2: `obscure -` (stdin), `copy`/`sync` flag sets,
  `lsjson --dirs-only` payload shape, `bisync` presence, and every mount flag used.

L8 (independent verification pass, read-only verifier; defects found and fixed):

1. Running jobs outlived app quit: only mounts were killed, so a mirror could keep deleting
   with no UI. `jobs::kill_all` now runs on exit next to `mounts::kill_all`, and
   `kill_process` escalates SIGTERM to SIGKILL after 3s on Unix.
2. Mount safety checks were bypassable with `..` and symlinks. `normalize` now resolves `..`
   lexically, `comparable` canonicalizes existing paths, and `validate_mount` rejects nested
   or duplicate mount points. Root spellings (`//`, `/./`, `/..`) are refused.
3. The credential temp file was created 0666 and chmodded after writing. It now opens with
   `OpenOptionsExt::mode(0o600)` from the start.
4. `--resync` was one-shot and keyed off `lastRun`, which was written before the spawn: a
   failed first run skipped the baseline forever. A baseline now counts only on success, and
   `--resync` is gated in the core so it can never reach `sync`/`copy`.
5. Editing a mounted mount left the child on old settings; `save_mount` now refuses while it
   is mounted.
6. Renaming a bucket broke every job/mount reference; the rename now rewrites them in the
   same mutation.
7. An unreadable `state.json` was silently replaced on the next save (and an unreadable
   `rclone.conf` would be clobbered too). Corrupt state is now quarantined to
   `state.json.corrupt-<ts>`, and `read_conf` refuses instead of returning an empty string.
8. Spawn-then-persist ordering: `persist_and_spawn` now spawns first and kills the child if
   the state write fails, removing both the orphan window and the stuck "running" summary.
9. `default_flags` must start with `-`, so a stray token cannot inject an rclone subcommand.

Tests added with these fixes: overlap after `..`, duplicate/nested mount points, root
spellings, temp-file mode, state quarantine, and `--resync` never reaching `sync`/`copy`.

L9 (RDD): `gentle-ai review mode status` is on (global). Native assess returned
`nativeRisk: unassessable` because the project is not a git repository and every file is
untracked, so no candidate can be bound. Plan returned: writer self-verification, no extra
verifier. An independent read-only verifier was run anyway; its findings are L8.

L10 (delivery strategy and review outcome):

- Version control: the repo existed but had zero commits. Work units were committed on
  `feat/s3dive-v1` from `master` (`chore: ignore build outputs and dependencies`):
  `ad8ed42` core crate, `ac7fe8f` shell, `3de0159` UI, `9fb25a5` docs, `593c5b6` Cargo.lock.
  Push, PR and merge stay with the user.
- Slice boundary: 8248 authored changes across 5 commits (lock files excluded), far above the
  ~400-line review slice because this is a greenfield app. The honest boundary is by
  component, and the review-relevant order is core (risky logic, tested) -> shell (process
  and credential handling) -> UI -> docs.
- Native assess on `master..HEAD` (committed range): `nativeRisk: high`, `reviewDue: true`
  (`high_risk`), candidate `base-diff` consumed=false.
- Native START was refused: `lens_context_budget_exceeded` in preflight, no authority was
  created (`mutation_outcome: not_started`), so nothing has to be abandoned or repaired. The
  native continuation is to split into smaller reviewable candidates. Each per-commit slice
  is the natural candidate; which slice to review first is the user's decision.

L5 (decisions taken during the build, not stated in the request):

- Jobs carry an explicit direction (push/pull) so a bucket can restore into a local folder;
  `bisync` ignores direction.
- Secrets are passed to `rclone obscure` on stdin, so they never appear in process arguments,
  and editing a bucket without re-entering the secret keeps the stored one.
- Mounts are supervised children, never `--daemon`, so exit reasons reach the UI and quitting
  the app can kill them explicitly.
- Mount points are refused when non-empty, when they overlap a job's local folder, when they
  are the filesystem root, or when another mount already uses them.
- Job and mount processes are terminated by pid inside the cancellation branch, because the
  awaited `wait()` future holds the child borrow for the whole `tokio::select!`.

L11 (native review of core+shell, blocked on host config): lineage
`review-df5280ed33aaa2aa` was created over the core+shell tree (73 paths, 4430 lines, tier
high, lenses risk/resilience/reliability) in a linked worktree `.review/core-shell`
(`master..ac7fe8f`). Capturing the lens artifacts is refused by the Pi host relay four times
with `pi-host-relay-transport-failure` / `reviewer-config-invalid`: "no model is configured
for review-risk". Nothing was mutated (`prepared_reviewers: 0`, `submitted_reviewers: 0`), no
authority was consumed, and the lineage stays `reviewing`.

Root cause and the fix attempted: the refusal is `review-host-relay.ts:616` when
`request.selection` is empty; the selection comes from the agent model routing config, not
from the reviewer agent frontmatter (`model:` there was tried and reverted, managed-asset
hashes verified byte-identical). Two stores were then populated with the user's chosen
routing (deepseek-v4-pro for review-risk, deepseek-flash for the others):
`~/.pi/agent/subagents.json` (`model_profiles`) and `~/.pi/gentle-ai/profiles.json`
(`kind: gentle-pi.agent_model_profiles`, `active: default`). The running session still
refuses, so the relay reads that config at session start: a fresh Pi session is required
before the lineage can be resumed.

L12 (native review of core+shell: APPROVED):
- Lineage `review-df5280ed33aaa2aa` over `master..ac7fe8f` (73 paths, 4430 lines, tier high),
  lenses risk (deepseek-v4-pro), resilience and reliability (deepseek-flash), plus the
  provider refuter. 3 reviewers prepared/submitted, revision `a77710ad`.
- Outcome: `approved`, then the exact acknowledgement continuation burned the authority
  (`burn_evidence: gentle-ai.review-acknowledged/v1`). Delivery is ordinary repository policy.
- Advisory (non-blocking, no correction offered, deliberate follow-ups rather than a reason to
  re-review this candidate): R1-001 CRITICAL refuted (`src-tauri/src/commands.rs:96-105`);
  informational R1-002 (same), R1-003 (`context.rs:96-112`), R1-004 (`core/src/validate.rs:174-180`),
  R1-005 (`src/jobs.rs:293-307`), R3-001 (`core/src/validate.rs:152-164`),
  R3-002 (`runner.rs:169-227`), R3-003 (`commands.rs:271-321`), R3-004 (`mounts.rs:128-157`),
  R3-005 SUGGESTION (`core/src/rclone.rs:354-360`), R4-lock-poison (`context.rs:105`),
  R4-mount-admission-after-spawn (`mounts.rs:131-168`).
- Unblocking detail for future sessions: the host relay resolves reviewer models from
  `~/.pi/gentle-ai/models.json`, a FLAT agent->{model,thinking} map read per call (not
  `subagents.json` `model_profiles`, not `profiles.json` `active`). Role slots use the fixed
  keys `review-refuter` and `review-validator`, so those need entries too. Selection format is
  `provider/id`, e.g. `deepseek/deepseek-v4-pro`.

L13 (post-delivery fix: corrupted Linux window):
- User report: `tauri dev` showed the window as corrupted horizontal bands. Reproduced live
  with `xwd` on the GTK window; the rest of the desktop rendered fine, so the React/CSS layer
  and the core were never implicated.
- Root cause: WebKitGTK 2.52.6 DMA-BUF renderer on X11 with the NVIDIA GT 730 running nouveau
  (Mesa 25.2.8). Relaunching with `WEBKIT_DISABLE_DMABUF_RENDERER=1` rendered the UI pixel
  perfect, which isolated the fault to the webview.
- Fix commit `4b45bd2`: `apply_linux_webkit_defaults()` in `src-tauri/src/lib.rs` defaults the
  variable before the webview exists (Linux only, external value respected, `=0` opts back in),
  plus a Troubleshooting row in `README.md`.
- Evidence: `cargo check` and `cargo clippy -D warnings` on the shell clean; `tauri dev` launched
  with `env -u WEBKIT_DISABLE_DMABUF_RENDERER` still rendered correctly, proving the binary
  applies the default itself. No automated test: the failure is a driver-level rasterization
  defect, not observable through the app's public interface.
