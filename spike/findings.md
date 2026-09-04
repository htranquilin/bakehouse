# M0 Runtime Spike — Findings (2026-08-31)

**Verdict: GO.** SQL Server 2022 runs under apple/container with Rosetta, relocated binaries work
unmodified, teardown is fully clean. Two design adjustments required (see below).

## Environment
- Apple M5, 24 GB RAM, macOS 26.6.2 (25G83). Rosetta installed. No system `container` install.
- No codesigning identities on this machine.

## Runtime: apple/container 1.3.1

### Distribution & signing — better than planned
- Release ships as a signed+notarized `.pkg`. `pkgutil --expand-full` extracts the payload:
  `bin/{container,container-apiserver}` + `libexec/container/plugins/{5 plugins}/`.
- **Binaries are pre-signed by Apple with `com.apple.security.virtualization` already on
  `container-runtime-linux`.** Signatures are path-independent and survive relocation.
  → **Bundle unmodified = no re-signing needed at all.** `sign-runtime.sh` becomes unnecessary
  unless we ever patch the binaries.
- Payload ≈ 422 MB (7 Swift static binaries ~60 MB each). The `k8s` plugin (61 MB) is droppable.
- Pinned artifact: `container-1.3.1-installer-signed.pkg` from GitHub releases.

### Relocation — works
```
container system start --app-root <dir> --install-root <payload-dir> --log-root <dir> \
  --enable-kernel-install --timeout 60
```
- Started clean on first try; kernel auto-installed with `--enable-kernel-install` (no prompt).
- `container system status` confirms custom appRoot/installRoot/logRoot.
- App-root contents: `apiserver/ containers/ content/ kernels/ networks/ plugin-state/ snapshots/ volumes/`.
- Env-var equivalents exist: `CONTAINER_APP_ROOT`, `CONTAINER_INSTALL_ROOT`, `CONTAINER_LOG_ROOT`.

### ⚠️ Design adjustment 1: no launchd prefix namespacing
- launchd labels are **hardcoded** `com.apple.container.*` (`--prefix` exists only on `system stop`;
  `CONTAINER_DEBUG_LAUNCHD_LABEL` untested). A bundled copy shares label space with any
  user-installed `container`.
- Mitigation: before `system start`/`system stop`, check the running apiserver's binary path
  (`ps` shows full paths; also `launchctl print gui/$UID/com.apple.container.apiserver`). If an
  apiserver is running from a path outside our bundle → surface "another container installation is
  active" instead of hijacking/stopping it.

## SQL Server under Rosetta — works
- Image: `mcr.microsoft.com/mssql/server:2022-CU26-ubuntu-22.04`
  **registry digest `sha256:ba4c8329f48fb8f02e1416be6a930ebfd71268caee78aa985f3af4315e457c89`** ← v1 pin.
  ⚠️ The digest shown by `container image list`/`inspect` (`b1dc5a74…`) is a locally-computed
  index digest, NOT the registry digest — pulling by it 404s. Pin the `docker-content-digest`
  from MCR's manifest HEAD; match local presence by reference string, not digest.
- Pull: ~1.57 GB, fast. Run:
  ```
  container run -d --rm --rosetta --arch amd64 -m 4096M \
    -l com.bakehouse.managed=true -e ACCEPT_EULA=Y -e MSSQL_SA_PASSWORD=… \
    -e MSSQL_MEMORY_LIMIT_MB=3072 --name <id> <image@digest>
  ```
- **Cold start to "ready for client connections": ~8 s** (far below the 120 s budget).
  No AVX/memory-mapping crashes. VM RSS ≈ 2.6 GB with 4 GB cap. Edition: Developer (default).
- TDS login + queries + CREATE DATABASE verified (python-tds from host).

### ⚠️ Design adjustment 2: published ports are broken for TDS — connect via container IP
- `-p 14330:1433`: TCP handshake succeeds but the connection is **reset during TDS prelogin**,
  consistently (apple/container's TCP forwarder issue). Retested warm: same failure.
- Direct container IP (`192.168.64.2:1433`) works perfectly.
- → Bakehouse connects to `<container-ip>:1433`; IP read from `container inspect`:
  `[0].status.networks[0].ipv4Address` (CIDR-suffixed). **Drop the 14330–14399 port-allocation
  scheme entirely.** Consequence: external tools can't reach instances via localhost ports in v1
  (compose export still covers portability).

## .bak round-trip — works
- Copy-in: `cat file | container exec -i <id> sh -c 'cat > /var/opt/mssql/backup/x.bak'`
  (no `container cp` subcommand exists). Files land owned `mssql:mssql` since exec runs as the
  container user — the docker-cp uid problem largely disappears; keep
  `container exec --user root <id> chown -R 10001:0 /var/opt/mssql/backup` as belt-and-braces (verified working).
- Copy-out: `container exec <id> cat /path > host-file` (binary-safe, verified).
- RESTORE HEADERONLY / FILELISTONLY / RESTORE DATABASE … WITH MOVE …, RECOVERY, STATS — all verified;
  restored data read back correctly.
- BACKUP DATABASE … WITH INIT, COMPRESSION — verified.

## Teardown — fully clean
- Crash state (no cleanup): 4 runtime processes + 1 Virtualization.framework VM process persist;
  launchd shows `com.apple.container.{apiserver, container-core-images, machine-apiserver,
  container-network-vmnet.default, container-runtime-linux.<container-id>}`.
  Per-container launchd label + our `com.bakehouse.managed=true` label = leftovers enumerable.
- Recovery: `container stop <id>` (with `--rm` the container vanishes) + `container system stop`
  → **zero processes, zero launchd entries**. `system stop` also stops containers itself (5 s timeout).

## JSON shapes for runtime/apple.rs
- `container list [--all] --format json` → array of `{configuration:{id, image:{reference,
  descriptor.digest}, labels, rosetta, platform:{architecture,os}, initProcess:{environment,…}},
  status?}`.
- `container inspect <id>` → same array shape; `status: {state: "running", startedDate,
  networks: [{ipv4Address: "192.168.64.2/24", hostname, …}]}`.
- ⚠️ `MSSQL_SA_PASSWORD` is visible in inspect env output — document; Keychain stays source of truth.

## Version/tag notes
- Latest 2022 CU: `2022-CU26-ubuntu-22.04` (pinned above). Latest 2025: CU8 (CU1+ safe under Rosetta).
- SQL `SELECT @@VERSION` confirms: "SQL Server 2022 (RTM-CU26) … 16.0.4265.3 (X64)".
