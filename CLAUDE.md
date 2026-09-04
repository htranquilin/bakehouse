# Bakehouse

A SQL Server workbench for macOS (Tauri 2: Rust backend + React/TS webview) that runs its own
SQL Server instances in Linux containers via a bundled copy of Apple's `container` runtime.

## Plan & findings
- Implementation plan: `~/.claude/plans/plan-my-new-macos-steady-marshmallow.md` (milestones M0–M7).
- **Read `spike/findings.md` before touching `runtime/`** — it records verified CLI flags, JSON
  shapes, and the two hard constraints below.

## Hard constraints (verified, do not "fix" these)
- SQL Server images are amd64-only → containers run `--rosetta --arch amd64`. Image is pinned:
  `mcr.microsoft.com/mssql/server:2022-CU26-ubuntu-22.04@sha256:b1dc5a74fa12…` (full digest in findings).
- **Connect to SQL Server via the container's IP (`inspect` → `status.networks[0].ipv4Address`),
  never published ports** — apple/container's port forwarder resets TDS prelogin.
- Runtime binaries in `src-tauri/resources/container-runtime/` are Apple-pre-signed with the
  virtualization entitlement. Bundle them UNMODIFIED (no re-signing); keep the `bin/` +
  `libexec/container/plugins/` relative layout intact.
- launchd labels are hardcoded `com.apple.container.*` — before starting/stopping the runtime,
  verify a running apiserver's binary path belongs to our bundle (a user may have their own install).
- Copy files in/out of containers with `container exec -i … cat` (there is no `container cp`).

## Commands
- `npm run tauri dev` — run the app (requires `npm run fetch-runtime` once for a populated
  `src-tauri/resources/container-runtime/`).
- Headless verification (each boots a real SQL Server container; ~2 min):
  `cargo run --features smoke --bin smoke|sqlsmoke|baksmoke -- "$PWD/src-tauri/resources" <scratch-dir>`
  (runtime lifecycle / SQL layer / .bak+metadata). Run from `src-tauri/`, wrap in
  `script -q log.txt …` if piping (println! is block-buffered when piped).
  ⚠️ Smoke binaries must never touch the macOS Keychain — unsigned rebuilds change code
  identity and a hidden auth dialog hangs the process. `SessionManager::new` takes an
  injectable password source for exactly this.
- `npm run sync-brand` — re-sync `visual-identity/tokens.css` + rebuild icons (uses sharp; do NOT
  use Homebrew — it belongs to a different user account on this machine).
- `cargo check` in `src-tauri/` for fast Rust iteration.

## Conventions
- Brand: `visual-identity/` is canonical; `src/styles/tokens.css` is a synced copy — never edit it
  directly. All UI colors/sizes come from `--bh-*` tokens. Ember = live/running only; gold = in
  progress (only color allowed to animate); state pills always pair color with a glyph.
- Fonts are bundled via fontsource packages; token font stacks are remapped in `src/styles/fonts.css`.
- Rust: commands are thin wrappers in `src-tauri/src/commands/`; long work is spawned and reports
  via typed events (`src-tauri/src/events.rs`, mirrored in `src/lib/events.ts`).
- Frontend state: zustand stores per domain; result-set rows never enter zustand or cross IPC as
  JSON (binary windows via `results_window`).
- SQL sessions must set SSMS-matching SET options on connect (see plan) or view metadata breaks.
