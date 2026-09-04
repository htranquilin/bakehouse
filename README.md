# Bakehouse

**A macOS workbench for SQL Server that brings its own instances.**

Bakehouse is a native macOS app for people who work with SQL Server data on a Mac —
restoring `.bak` backups, running queries, building views — without a Windows machine,
without Docker Desktop, and without anything left running after you quit.

Pick a SQL Server version, click create, and Bakehouse spins up an isolated instance in a
lightweight VM using Apple's open-source [container](https://github.com/apple/container)
runtime (bundled inside the app). Restore a vendor backup, query it, script your views,
export your results. Quit the app and everything shuts down; your databases persist and
come back on the next launch.

## Features

**Instances**
- Create, start, stop, and delete local SQL Server instances — versions **2017, 2019, 2022, and 2025**, each pinned to an exact image digest
- First-run wizard: license acceptance, Rosetta check, guided first instance
- Nothing runs in the background after quit; a crash sweeper cleans up leftovers on next launch
- Generated `sa` passwords stored in the macOS Keychain
- Export any instance as a `docker-compose.yml` to recreate it on vanilla Docker elsewhere
- Live connection manifest at `~/.bakehouse/connections.json` so external tools and
  scripts can connect to running instances (see [CONNECTING.md](CONNECTING.md))

**Workbench**
- Query editor tabs (CodeMirror) with T-SQL highlighting, schema-aware autocomplete,
  `GO` batch handling, ⌘↵ runs the selection or the whole buffer
- Each tab pins to an instance and database, with a visible spid and open-transaction badge
- Messages pane with `PRINT` output, rows-affected counts, and clickable error line numbers
- Cancelable queries; sessions survive and reconnect automatically
- Virtualized results grid for large result sets: sort, column resize, cell viewer,
  distinct NULL rendering, right-click copy (cell / row / column / selection)
- CSV export with configurable separator, quoting, encoding, and NULL representation;
  copy-as-TSV for pasting into spreadsheets
- Object tree with tables, views, procedures, functions, and columns; filter,
  Select Top 1000, Script as CREATE/ALTER
- Query history and crash-safe autosaved buffers; open/save `.sql` files (⌘O / ⌘S)

**Data in and out**
- **Restore `.bak`** — pick a file, Bakehouse stages it into the container, reads the
  backup sets, plans the file moves, and shows live restore progress. Version-downgrade
  and FILESTREAM backups are refused early with clear explanations
- **Backup to `.bak`** — compressed backup copied out to a path you choose
- **Import CSV files** — SSMS "Import Flat File" style, but for many files at once
  (or a whole folder). Delimiter/header/type inference with per-file table names
  defaulted from file names
- **Generate scripts** — SSMS "Tasks → Generate Scripts" style: pick objects, get a
  single dependency-ordered `.sql` (or one file per object), with optional
  `DROP … IF EXISTS`

## Requirements

- **macOS 26 (Tahoe) or later**, Apple Silicon
- **Rosetta 2** (SQL Server ships only for x86-64; the wizard tells you how to install
  Rosetta if it's missing: `softwareupdate --install-rosetta --agree-to-license`)
- ~4 GB of free RAM per running instance (configurable), and disk space for the SQL
  Server images (~1.5–2 GB per version, downloaded once on first use)

No Docker, no Docker Desktop, no Homebrew packages, no background services.

## Building from source

You need [Rust](https://rustup.rs) (stable) and Node.js 20+.

```sh
git clone <this repo>
cd bakehouse
npm install
npm run fetch-runtime   # downloads Apple's signed container runtime into src-tauri/resources/
npm run sync-brand      # renders the app icon and syncs design tokens
npm run tauri dev       # run it
npm run tauri build     # or build Bakehouse.app
```

`fetch-runtime` downloads the pinned [apple/container](https://github.com/apple/container)
release, verifies its Apple notarization, and stages the binaries **unmodified** — they
ship pre-signed with the virtualization entitlement, so no code-signing setup is needed
for a personal build.

### Tests

```sh
cd src-tauri
cargo test                                   # unit tests (batch splitter, CSV inference, …)
cargo run --features smoke --bin smoke    -- "$PWD/resources" /tmp/bh-smoke   # runtime lifecycle
cargo run --features smoke --bin sqlsmoke -- "$PWD/resources" /tmp/bh-smoke   # SQL layer
cargo run --features smoke --bin baksmoke -- "$PWD/resources" /tmp/bh-smoke   # .bak + metadata
```

The smoke binaries boot real SQL Server containers (a few minutes each, first run
downloads the image). They refuse to run while another Bakehouse/container installation
is active.

## How it works

- Each instance is one Linux container in its own lightweight VM
  (Apple [Containerization](https://github.com/apple/containerization)), running the
  official `mcr.microsoft.com/mssql/server` image for amd64 under **Rosetta 2 for Linux**
- The runtime lives inside the app bundle and uses a private data root
  (`~/Library/Application Support/com.bakehouse.app/container-root`), so it never
  collides with Docker, Colima, or a system `container` install
- Database files live on named volumes that survive stop/start; the app connects over
  TDS (a patched [tiberius](https://github.com/prisma/tiberius) driver, vendored in
  `src-tauri/vendor/tiberius`) straight to the container's IP
- UI is Tauri 2: a Rust backend and a WebView front end. Result rows never cross IPC as
  JSON — the grid fetches binary row windows from Rust buffers

## Good to know

- **Instances run only while Bakehouse is open.** That's a feature — quit means quiet.
  Databases persist on their volumes and return on the next start.
- **Connect from outside the app** via `~/.bakehouse/connections.json`
  ([CONNECTING.md](CONNECTING.md)). Instances are reachable at their container IP on
  port 1433 from the local machine; there are no localhost port mappings.
- SQL Server on Apple Silicon means x86-64 emulation. Microsoft doesn't officially
  support emulated environments — Bakehouse is a development and test tool, not a
  production server.
- The SQL Server images default to **Developer Edition**, which is free for
  development/test use only. You accept Microsoft's license terms in the first-run
  wizard.
- `.bak` limitations: full backups only (no differential/log chains yet), and backups
  containing FILESTREAM or memory-optimized filegroups can't be restored on SQL Server
  for Linux.

## Third-party components

| Component | License | How it's used |
| --- | --- | --- |
| [apple/container](https://github.com/apple/container) | Apache-2.0 | Container runtime, downloaded at build time (not committed) |
| [tiberius](https://github.com/prisma/tiberius) 0.12.3 | MIT/Apache-2.0 | TDS driver, vendored with a small patch (grep "Bakehouse patch") |
| Microsoft SQL Server container images | [Microsoft EULA](https://go.microsoft.com/fwlink/?linkid=857698) | Pulled at runtime after user acceptance |
| Fraunces, JetBrains Mono | SIL OFL 1.1 | Bundled fonts |
| Geist | MIT | Bundled font |
| CodeMirror, Glide Data Grid, React, Tauri | MIT/Apache-2.0 | UI |

Bakehouse is an independent project. It is not affiliated with or endorsed by Microsoft;
"SQL Server" is referenced only to describe compatibility.

## License

[MIT](LICENSE). Third-party components keep their own licenses (see the table above).
