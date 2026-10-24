# S3Dive

A cross-platform desktop app (Tauri v2) that puts a visual interface on top of
[rclone](https://rclone.org): configure S3 buckets, run sync/copy/two-way jobs between a
bucket and a local folder, and mount buckets as ordinary folders — with the option to
bring those mounts back automatically after a reboot.

S3Dive is a **front-end for rclone**. It does not reimplement S3: it builds rclone
command lines, supervises the processes, and shows you what happened.

## What it does

- **Buckets** — create, edit, test and remove S3 remotes (AWS and S3-compatible
  endpoints such as MinIO, Cloudflare R2, Wasabi). Connection test lists the remote root.
- **Sync jobs** — one local folder, one bucket path, three semantics:
  - `Copy` — additive, never deletes.
  - `Mirror (sync)` — destination becomes identical; extra destination files are deleted.
    The app asks for confirmation before every mirror run.
  - `Two-way (bisync)` — changes flow both ways and deletions propagate too; the first
    successful run establishes the baseline, and *Re-sync baseline* re-records it later.
  Include/exclude globs, optional dry-run, and live rclone log output per job.
- **Mounts** — expose a bucket path as a local folder with `rclone mount`, in a supervised
  child process. Choose the VFS cache mode, read-only, `--allow-other` (Unix only).
- **Auto-mount at login** — register S3Dive as a login item; every mount flagged
  *auto-mount* is started when the app starts. Quitting the app unmounts them.
- **Tray icon** — closing the window hides S3Dive to the system tray, so mounts keep
  running. Use *Quit S3Dive* in the tray menu to exit; quitting unmounts everything.

## Requirements

| | Requirement |
| --- | --- |
| All platforms | [rclone](https://rclone.org/downloads/) on `PATH` (or set an explicit path in Settings). Developed against rclone 1.75. |
| macOS | [macFUSE](https://macfuse.io/) — only for mounts. Approve the kernel extension in *System Settings → Privacy & Security*. |
| Linux | `fuse3` — only for mounts (`sudo apt install fuse3`). `/dev/fuse` must exist. |
| Windows | [WinFsp](https://winfsp.dev/rel/) — only for mounts (`winget install WinFsp.WinFsp`). |

S3Dive detects the mount prerequisite at startup and tells you what is missing instead of
failing at mount time. Sync and copy jobs work with rclone alone.

## Using it

1. **Buckets → Add bucket.** Enter a name, the access key ID and the secret. For
   non-AWS endpoints set *S3-compatible* plus the endpoint URL and keep path-style
   addressing on. Hit *Test connection*.
2. **Jobs → New job.** Pick the bucket, the folder inside the bucket (browse it), the
   local folder, and the semantics. Run it and watch the log.
3. **Mounts → New mount.** Pick an **existing, empty** local directory as the mount point.
   Toggle *Auto-mount when S3Dive starts*, then enable *Start S3Dive at login*.
4. **Settings.** Override the rclone binary, set a bandwidth limit (`--bwlimit`) and extra
   flags, one per line, applied to every sync, copy, bisync and mount.

## How auto-mount survives a reboot

Two independent pieces, both visible in the Mounts tab:

1. `tauri-plugin-autostart` registers S3Dive with the OS login items (LaunchAgent on
   macOS, `~/.config/autostart` on Linux, `Run` registry key on Windows).
2. On startup S3Dive starts every mount whose *auto-mount* flag is set.

If a mount fails at startup (missing FUSE, missing mount point, dead credentials) the app
still opens and reports the failure on that mount's card — one broken mount never blocks
the others, and never blocks the UI.

## Credentials and safety

- S3Dive never touches your own `~/.config/rclone/rclone.conf`. It keeps a private
  `rclone.conf` and passes `--config <that file>` to every rclone invocation.
- The secret access key is written **verbatim** to S3Dive's private `rclone.conf`:
  rclone's s3 backend reads `secret_access_key` as plaintext and never reveals an obscured
  value, so an obscured secret makes every request fail with `SignatureDoesNotMatch`. The
  secret never enters process arguments, S3Dive's state file, the logs, or any value
  returned to the UI.
- `rclone.conf` and `state.json` are written atomically with mode `0600` on Unix.
- That config file is a credential: anyone who can read it can read your S3 secrets. The
  Settings tab shows where it lives.
- `Mirror` deletes files that exist only at the destination. It always needs an explicit
  confirmation, and *Dry run* is one toggle away.
- S3Dive refuses to mount over a non-empty directory, over a directory that overlaps any
  job's local folder (that would make rclone read its own output), or over `/`.

## Development

Prerequisites: Node 20+, pnpm, Rust stable, and the
[Tauri v2 system dependencies](https://v2.tauri.app/start/prerequisites/) for your OS.
On Debian/Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev librsvg2-dev libayatana-appindicator3-dev pkg-config
```

Then:

```bash
pnpm install
pnpm app:dev      # tauri dev: Vite on :1420 + the Rust shell
pnpm app:build    # bundles for the current platform
```

Checks:

```bash
pnpm typecheck                                  # tsc --noEmit
pnpm build                                      # tsc + vite production build
cargo test --manifest-path src-tauri/core/Cargo.toml
cargo clippy --manifest-path src-tauri/core/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml   # needs the system deps above
```

Icons are reproducible: `python3 scripts/make-icon.py assets/app-icon.png && pnpm tauri icon assets/app-icon.png`.

### Building without root (Ubuntu/Debian, when `sudo` is not available)

`cargo check`/`cargo build` for the shell need the Tauri dev headers. If you cannot install
packages system-wide, extract them into a sysroot instead — no root required:

```bash
SYSROOT=/tmp/tauri-sysroot
PKGS="libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev libgtk-3-dev \
libglib2.0-dev libpango1.0-dev libcairo2-dev libgdk-pixbuf-2.0-dev libdbus-1-dev libxdo-dev \
librsvg2-dev libayatana-appindicator3-dev libssl-dev libatk1.0-dev libharfbuzz-dev \
libfontconfig1-dev libfreetype-dev libx11-dev libxcomposite-dev libxdamage-dev libxext-dev \
libxfixes-dev libxrandr-dev libxi-dev libxcursor-dev libenchant-2-dev libepoxy-dev \
libwayland-dev libxkbcommon-dev"

mkdir -p "$SYSROOT" /tmp/debs
apt-cache depends --recurse --no-recommends --no-suggests --no-conflicts --no-breaks \
  --no-replaces --no-enhances $PKGS \
  | grep -E '^[A-Za-z0-9][A-Za-z0-9.+-]*(:[a-z0-9]+)?$' | sed -E 's/:.*$//' | sort -u \
  | comm -12 - <(apt-cache pkgnames | sort -u) > /tmp/deps.txt
(cd /tmp/debs && apt-get download $(cat /tmp/deps.txt))
for d in /tmp/debs/*.deb; do dpkg -x "$d" "$SYSROOT"; done
find "$SYSROOT" -name '*.pc' -exec sed -i "s|/usr|$SYSROOT/usr|g" {} +

# every build:
export PKG_CONFIG_PATH="$SYSROOT/usr/lib/x86_64-linux-gnu/pkgconfig:$SYSROOT/usr/share/pkgconfig:$SYSROOT/usr/lib/pkgconfig"
export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1 PKG_CONFIG_ALLOW_SYSTEM_LIBS=1
cargo check --manifest-path src-tauri/Cargo.toml
```

This produces `-I$SYSROOT/...` flags, so host headers are never mixed with sysroot headers.
Without those variables the `*-sys` crates fail with `failed to run custom build command`.
It is enough for `cargo check`/`cargo clippy`; producing installers still needs a real
toolchain install (and a display for `tauri dev`).

## Project layout

```
src/                     React + TypeScript UI (no framework routing, one store)
  api.ts                 typed wrappers over the Rust commands
  store.tsx              bootstrap, event subscriptions, single source of truth
  views/                 Buckets, Jobs, Mounts, Settings
  components/            shared UI + the bucket folder browser
src-tauri/
  src/                   Tauri shell: commands, job/mount supervision, process registry
  core/                  Tauri-free logic crate: model, validation, rclone arg builders,
                         rclone.conf editing, atomic store, platform probes — unit tested
  tauri.conf.json        window, bundle, CSP
  capabilities/          what the window is allowed to call
odd/tasks/s3dive-v1.md   the feature spec and task log this build followed
```

Why a separate `core` crate: the risky logic (which flags rclone gets, how the config file
is edited, how state is persisted, how paths are validated) is testable with `cargo test`
and no GUI, no display server and no rclone binary.

## Versioning

The release version lives in `package.json`. `src-tauri/tauri.conf.json` points at it
(`"version": "../package.json"`), so the Tauri bundler and the running app report the same
number. `src-tauri/Cargo.toml` repeats it for the crate metadata, and a test fails when the
three drift apart.

```bash
pnpm version 0.3.0 --no-git-tag-version   # bumps package.json
```

Then set the same number in `src-tauri/Cargo.toml` (`cargo test --manifest-path
src-tauri/Cargo.toml` tells you when you forget). The version is shown in the sidebar, under
the rclone version.

## Where your data lives

| OS | Folder |
| --- | --- |
| Linux | `~/.config/dev.s3dive.app/` |
| macOS | `~/Library/Application Support/dev.s3dive.app/` |
| Windows | `%APPDATA%\dev.s3dive.app\` |

Contains `state.json` (remotes without secrets, jobs, mounts, settings) and `rclone.conf`.
Settings → *Reveal config folder* opens it.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `rclone was not found` | Install rclone, or set the binary path in Settings and press *Re-check*. |
| `Access denied` on a bucket | The key is valid but lacks `s3:ListBucket`/`s3:GetObject` on that bucket. |
| `Signature mismatch` | The signing secret does not belong to that access key ID, or the region/endpoint pair is wrong — re-enter the secret on the bucket. If it appears after upgrading rclone, check that `secret_access_key` in S3Dive's `rclone.conf` is plaintext: rclone's s3 backend does not reveal obscured values. |
| Mount button disabled / "macFUSE is not installed" | Install the mount prerequisite for your OS and restart S3Dive. |
| "That bucket is still used by a job or a mount" | Buckets referenced by jobs/mounts cannot be deleted. Renaming one keeps every job and mount on the same bucket. |
| `state.json is not readable` | S3Dive keeps the unreadable file as `state.json.corrupt-<timestamp>` and starts empty instead of overwriting it. |
| `mount point must be empty` | Point the mount at an existing empty directory. |
| Mount dies immediately | Open the mount's *Log*; rclone's last stderr line is kept on the card. |
| Linux: the window is a mess of corrupted horizontal bands | WebKitGTK's DMA-BUF renderer failing on your graphics driver. S3Dive already disables it at startup; if the bands persist, launch with `WEBKIT_DISABLE_COMPOSITING_MODE=1` as well. |

## Status and limits (v1)

- Jobs run **when you press Run**. There is no scheduler and no background daemon; the app
  only auto-starts mounts.
- Quitting S3Dive terminates its child processes: running jobs and mounts are killed
  (SIGTERM, then SIGKILL after 3s), so nothing keeps deleting files after the window closes.
- A mount cannot be edited while it is mounted — unmount it first. Its mount point is
  refused when non-empty, nested inside another mount point, or overlapping a job folder.
- Two-way jobs can be re-baselined from the job card (*Re-sync baseline*) when the two sides
  drift apart; a baseline only counts once a run actually succeeded.
- Mounts are children of the app: a crash can leave a stale FUSE mount that needs
  `fusermount -u` / a Windows remount.
- `--allow-other` is hidden on Windows (WinFsp has no equivalent).
- One rclone binary per app instance; profiles/remotes from your global rclone config are
  not imported.
