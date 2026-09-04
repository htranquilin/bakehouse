# Connecting external tools and AI agents to Bakehouse instances

This document is written to be handed to an AI agent (e.g. a Claude skill) or a script
author who wants to talk to SQL Server instances managed by the
[Bakehouse](README.md) macOS app: read data, run queries, create views, execute
generated `.sql` files — directly over TDS, without going through the app's UI.

## The connection manifest

Bakehouse maintains a machine-readable manifest of every instance:

```
~/.bakehouse/connections.json        (file mode 0600 — it contains passwords)
```

Example content:

```json
{
  "$note": "Maintained by Bakehouse. …",
  "updatedAt": "2026-09-02T14:03:11.512Z",
  "instances": [
    {
      "name": "Legacy vendor 2019",
      "id": "bh-4f2a91c07d3e",
      "image": "mcr.microsoft.com/mssql/server:2019-CU32-ubuntu-20.04@sha256:…",
      "state": "running",
      "host": "192.168.64.4",
      "port": 1433,
      "user": "sa",
      "password": "Xy3…",
      "connectionString": "Server=192.168.64.4,1433;User Id=sa;Password=…;TrustServerCertificate=True",
      "sqlcmd": "sqlcmd -S 192.168.64.4,1433 -U sa -P '…' -C"
    },
    {
      "name": "Target test",
      "id": "bh-99ab12cd34ef",
      "image": "mcr.microsoft.com/mssql/server:2022-CU26-ubuntu-22.04@sha256:…",
      "state": "stopped"
    }
  ]
}
```

Field notes:

| Field | Meaning |
| --- | --- |
| `name` | Human-chosen instance name — **the key users will refer to** |
| `id` | Stable internal id (also the container name) |
| `state` | `running`, `stopped`, `starting`, `pulling`, `waitingForSql`, `stopping`, `failed` |
| `host`, `port`, `user`, `password` | Present **only when `state` is `running`** |
| `connectionString` | ADO.NET-style string, ready to use |
| `sqlcmd` | A ready-to-run `sqlcmd` invocation (note the `-C` flag) |

## Rules an agent must follow

1. **Re-read the file immediately before every connection.** The `host` is the
   container's IP on a private VM network and **changes every time an instance starts**.
   Never cache connection parameters across runs.
2. **Instances exist only while the Bakehouse app is open.** If the file is missing,
   or the wanted instance isn't `state: "running"`, stop and tell the user to open
   Bakehouse and start the instance — don't retry in a loop.
3. **Trust the server's self-signed certificate.** The containers use auto-generated
   certificates. Every client needs the equivalent of `TrustServerCertificate=True`:
   - `sqlcmd`: add `-C`
   - Python `pytds`: `cafile=None, validate_host=False`
   - Python `pymssql`: trusts by default
   - Encryption is negotiated TDS 7.4 (login always encrypted); do **not** request
     strict/TDS 8.0 encryption — it is not enabled on these servers.
4. **Treat a refused connection as stale state**, not an error to retry hard: the app
   may have quit or crashed since the file was written. Report it and re-read the file.
5. The credentials are `sa` on a throwaway local dev server — full permissions,
   local-machine reachability only. Still: don't print the password into logs or chat
   output unnecessarily.

## Executing SQL — the `GO` rule (important)

`GO` is **not** T-SQL. It's a client-side batch separator used by SSMS/sqlcmd, and
generated `.sql` files (including Bakehouse's own "Generate scripts" output) use it.

- If you execute a script **through a TDS driver** (pytds, pymssql, tedious, …), you
  must **split the script on lines containing only `GO`** and send each batch as its own
  execution. Sending the whole file as one statement fails.
- `CREATE VIEW`, `CREATE PROCEDURE`, `CREATE FUNCTION`, and `CREATE TRIGGER` must be the
  **first statement in their batch** — which is exactly why the `GO` separators are there.
- `sqlcmd` handles `GO` natively, so `sqlcmd … -i file.sql` needs no splitting.

A sufficient splitter for well-formed generated scripts: split on `^\s*GO\s*;?\s*$`
(case-insensitive, whole line). Don't split on the word GO inside strings or comments if
you process arbitrary hand-written SQL.

## Recipes

### Bash + sqlcmd

```bash
# Pick the instance named "Target test" and run a script file
info=$(python3 -c "
import json,sys
d=json.load(open('$HOME/.bakehouse/connections.json'))
i=[x for x in d['instances'] if x['name']=='Target test']
assert i and i[0]['state']=='running', 'instance not running — open Bakehouse and start it'
print(i[0]['host'], i[0]['password'])")
read -r host pw <<< "$info"
sqlcmd -S "$host,1433" -U sa -P "$pw" -C -d MyDatabase -i views.sql
```

### Python (pure-Python driver, `pip install python-tds`)

```python
import json, re, pathlib
import pytds

def connect(instance_name: str, database: str = "master"):
    manifest = json.loads((pathlib.Path.home() / ".bakehouse/connections.json").read_text())
    inst = next((i for i in manifest["instances"] if i["name"] == instance_name), None)
    if inst is None:
        raise SystemExit(f"No Bakehouse instance named {instance_name!r}")
    if inst.get("state") != "running":
        raise SystemExit(f"{instance_name!r} is {inst.get('state')} — start it in Bakehouse first")
    return pytds.connect(
        server=inst["host"], port=inst["port"],
        user=inst["user"], password=inst["password"],
        database=database, cafile=None, validate_host=False,
        autocommit=True,
    )

def run_script(conn, sql_text: str):
    """Execute a script that may contain GO batch separators."""
    for batch in re.split(r"(?im)^\s*GO\s*;?\s*$", sql_text):
        if batch.strip():
            conn.cursor().execute(batch)

with connect("Legacy vendor 2019", database="vendor_db") as conn:
    cur = conn.cursor()
    cur.execute("SELECT TOP 10 * FROM dbo.customers")
    for row in cur.fetchall():
        print(row)

with connect("Target test", database="target_db") as conn:
    run_script(conn, open("generated_views.sql").read())
```

### Useful queries

```sql
-- What databases exist (skip system dbs)
SELECT name FROM sys.databases WHERE database_id > 4 AND state = 0;

-- All user views with their definitions
SELECT s.name AS [schema], v.name, m.definition
FROM sys.views v
JOIN sys.schemas s ON v.schema_id = s.schema_id
JOIN sys.sql_modules m ON m.object_id = v.object_id;

-- Column inventory for mapping work
SELECT s.name AS [schema], t.name AS [table], c.name AS [column],
       ty.name AS type, c.max_length, c.is_nullable
FROM sys.columns c
JOIN sys.tables t  ON c.object_id = t.object_id
JOIN sys.schemas s ON t.schema_id = s.schema_id
JOIN sys.types ty  ON c.user_type_id = ty.user_type_id
ORDER BY s.name, t.name, c.column_id;
```

## Quick reference

- Manifest: `~/.bakehouse/connections.json` (re-read before every connect)
- Port: always `1433`, at the instance's current `host` IP; reachable from this Mac only
- Auth: SQL auth, `sa` + per-instance generated password
- TLS: trust the self-signed certificate; TDS 7.4 (no strict encryption)
- `GO` in scripts: split client-side, or use `sqlcmd`
- Instance lifecycle: only while the Bakehouse app is open; databases persist between runs
