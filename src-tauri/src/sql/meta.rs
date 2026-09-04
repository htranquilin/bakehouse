//! Catalog queries for the object tree, definitions, and completion schema.
//! All queries are fully qualified against [db].sys.* so pooled utility
//! connections never need USE.

use crate::error::Result;
use crate::sql::conn::SqlConn;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjInfo {
    pub schema: String,
    pub name: String,
    pub object_id: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_pk: bool,
}

pub fn quote_ident(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

pub async fn objects(conn: &mut SqlConn, database: &str, kind: &str) -> Result<Vec<ObjInfo>> {
    let type_filter = match kind {
        "table" => "'U'",
        "view" => "'V'",
        "proc" => "'P'",
        "fn" => "'FN','IF','TF'",
        _ => "'U'",
    };
    let db = quote_ident(database);
    let sql = format!(
        "SELECT s.name, o.name, CAST(o.object_id AS bigint)
         FROM {db}.sys.objects o JOIN {db}.sys.schemas s ON o.schema_id = s.schema_id
         WHERE o.type IN ({type_filter}) AND o.is_ms_shipped = 0
         ORDER BY s.name, o.name"
    );
    let rows = conn.query_rows(&sql).await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(ObjInfo {
                schema: r.first()?.display.clone(),
                name: r.get(1)?.display.clone(),
                object_id: r.get(2)?.display.parse().ok()?,
            })
        })
        .collect())
}

pub async fn columns(conn: &mut SqlConn, database: &str, object_id: i64) -> Result<Vec<ColInfo>> {
    let db = quote_ident(database);
    let sql = format!(
        "SELECT c.name,
                CONCAT(t.name,
                  CASE WHEN t.name IN ('varchar','char','varbinary','binary')
                         THEN CONCAT('(', IIF(c.max_length = -1, 'max', CAST(c.max_length AS varchar(10))), ')')
                       WHEN t.name IN ('nvarchar','nchar')
                         THEN CONCAT('(', IIF(c.max_length = -1, 'max', CAST(c.max_length/2 AS varchar(10))), ')')
                       WHEN t.name IN ('decimal','numeric')
                         THEN CONCAT('(', c.precision, ',', c.scale, ')')
                       ELSE '' END),
                c.is_nullable,
                IIF(pk.column_id IS NOT NULL, 1, 0)
         FROM {db}.sys.columns c
         JOIN {db}.sys.types t ON c.user_type_id = t.user_type_id
         LEFT JOIN (
            SELECT ic.object_id, ic.column_id
            FROM {db}.sys.index_columns ic
            JOIN {db}.sys.indexes i ON i.object_id = ic.object_id AND i.index_id = ic.index_id
            WHERE i.is_primary_key = 1
         ) pk ON pk.object_id = c.object_id AND pk.column_id = c.column_id
         WHERE c.object_id = {object_id}
         ORDER BY c.column_id"
    );
    let rows = conn.query_rows(&sql).await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(ColInfo {
                name: r.first()?.display.clone(),
                data_type: r.get(1)?.display.clone(),
                nullable: r.get(2)?.display == "1" || r.get(2)?.display == "true",
                is_pk: r.get(3)?.display == "1",
            })
        })
        .collect())
}

/// Module/view/proc definition via OBJECT_DEFINITION; tables get a
/// catalog-driven CREATE TABLE (columns, types, nullability, identity, PK —
/// FKs/defaults intentionally out of v1 scope). Wraps the body with USE.
pub async fn script_object(
    conn: &mut SqlConn,
    database: &str,
    object_id: i64,
    alter: bool,
) -> Result<String> {
    let scripted = script_object_body(conn, database, object_id, alter).await?;
    Ok(format!(
        "USE {}\nGO\n{}\nGO\n",
        quote_ident(database),
        scripted.body.trim_end()
    ))
}

pub struct ScriptedObject {
    pub object_type: String,
    pub schema: String,
    pub name: String,
    pub body: String,
}

pub async fn script_object_body(
    conn: &mut SqlConn,
    database: &str,
    object_id: i64,
    alter: bool,
) -> Result<ScriptedObject> {
    let db = quote_ident(database);
    // OBJECT_DEFINITION() resolves in the CURRENT database; sys.sql_modules can
    // be db-qualified, which keeps pooled utility connections USE-free.
    let def_rows = conn
        .query_rows(&format!(
            "SELECT o.type, s.name, o.name, ISNULL(m.definition, '')
             FROM {db}.sys.objects o
             JOIN {db}.sys.schemas s ON o.schema_id = s.schema_id
             LEFT JOIN {db}.sys.sql_modules m ON m.object_id = o.object_id
             WHERE o.object_id = {object_id}"
        ))
        .await?;
    let row = def_rows
        .first()
        .ok_or_else(|| crate::AppError::Internal("object not found".into()))?;
    let obj_type = row[0].display.trim().to_string();
    let schema = row[1].display.clone();
    let name = row[2].display.clone();
    let definition = row[3].display.clone();

    if obj_type == "U" {
        // CREATE TABLE from catalog.
        let cols = columns(conn, database, object_id).await?;
        let identity = conn
            .query_rows(&format!(
                "SELECT name FROM {db}.sys.identity_columns WHERE object_id = {object_id}"
            ))
            .await?;
        let identity_col = identity.first().and_then(|r| r.first()).map(|c| c.display.clone());
        let pk_cols: Vec<String> =
            cols.iter().filter(|c| c.is_pk).map(|c| quote_ident(&c.name)).collect();
        let mut lines: Vec<String> = cols
            .iter()
            .map(|c| {
                format!(
                    "    {} {}{}{}",
                    quote_ident(&c.name),
                    c.data_type,
                    if identity_col.as_deref() == Some(c.name.as_str()) { " IDENTITY(1,1)" } else { "" },
                    if c.nullable { " NULL" } else { " NOT NULL" },
                )
            })
            .collect();
        if !pk_cols.is_empty() {
            lines.push(format!("    PRIMARY KEY ({})", pk_cols.join(", ")));
        }
        return Ok(ScriptedObject {
            object_type: obj_type,
            body: format!(
                "CREATE TABLE {}.{} (\n{}\n)",
                quote_ident(&schema),
                quote_ident(&name),
                lines.join(",\n")
            ),
            schema,
            name,
        });
    }

    if definition.is_empty() {
        return Err(crate::AppError::Internal("no definition available (encrypted object?)".into()));
    }
    let definition = if alter {
        rewrite_create_to_alter(&definition)
    } else {
        definition
    };
    Ok(ScriptedObject {
        object_type: obj_type,
        schema,
        name,
        body: definition.trim_end().to_string(),
    })
}

/// SSMS "Generate Scripts": script several objects into one T-SQL file, in
/// dependency order (tables → functions → views → procedures, with
/// sys.sql_expression_dependencies breaking ties among the selected modules).
pub async fn generate_scripts(
    conn: &mut SqlConn,
    database: &str,
    object_ids: &[i64],
    include_drop: bool,
    include_use: bool,
) -> Result<String> {
    if object_ids.is_empty() {
        return Err(crate::AppError::Internal("no objects selected".into()));
    }
    let db = quote_ident(database);
    let id_list = object_ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");

    // Category rank per object (base ordering).
    let rows = conn
        .query_rows(&format!(
            "SELECT CAST(o.object_id AS bigint), RTRIM(o.type)
             FROM {db}.sys.objects o WHERE o.object_id IN ({id_list})"
        ))
        .await?;
    let rank = |t: &str| match t {
        "U" => 0u8,
        "FN" | "IF" | "TF" => 1,
        "V" => 2,
        _ => 3,
    };
    let mut kinds: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    for r in &rows {
        if let (Some(id), Some(t)) = (
            r.first().and_then(|c| c.display.parse::<i64>().ok()),
            r.get(1).map(|c| c.display.clone()),
        ) {
            kinds.insert(id, t);
        }
    }

    // Dependencies among the selected objects only.
    let dep_rows = conn
        .query_rows(&format!(
            "SELECT DISTINCT CAST(d.referencing_id AS bigint), CAST(d.referenced_id AS bigint)
             FROM {db}.sys.sql_expression_dependencies d
             WHERE d.referencing_id IN ({id_list}) AND d.referenced_id IN ({id_list})
               AND d.referencing_id <> d.referenced_id"
        ))
        .await?;
    let mut deps: Vec<(i64, i64)> = vec![]; // (needs, needed-first)
    for r in &dep_rows {
        if let (Some(a), Some(b)) = (
            r.first().and_then(|c| c.display.parse().ok()),
            r.get(1).and_then(|c| c.display.parse().ok()),
        ) {
            deps.push((a, b));
        }
    }

    // Kahn's algorithm, seeded in category-rank order for stability.
    let mut ordered: Vec<i64> = object_ids.to_vec();
    ordered.sort_by_key(|id| rank(kinds.get(id).map(String::as_str).unwrap_or("")));
    let mut result: Vec<i64> = vec![];
    let mut remaining: Vec<i64> = ordered;
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .position(|id| {
                !deps.iter().any(|(a, b)| a == id && remaining.contains(b))
            })
            .unwrap_or(0); // dependency cycle: emit in base order
        result.push(remaining.remove(next));
    }

    let mut out = format!(
        "-- Generated by Bakehouse on {} from database [{}]\nSET ANSI_NULLS ON\nGO\nSET QUOTED_IDENTIFIER ON\nGO\n",
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC"),
        database
    );
    if include_use {
        out = format!("{out}USE {db}\nGO\n");
    }
    for id in result {
        let obj = script_object_body(conn, database, id, false).await?;
        out.push('\n');
        if include_drop {
            let drop_kw = match obj.object_type.as_str() {
                "U" => "TABLE",
                "V" => "VIEW",
                "P" => "PROCEDURE",
                "FN" | "IF" | "TF" => "FUNCTION",
                _ => "OBJECT",
            };
            out.push_str(&format!(
                "DROP {drop_kw} IF EXISTS {}.{}\nGO\n",
                quote_ident(&obj.schema),
                quote_ident(&obj.name)
            ));
        }
        out.push_str(obj.body.trim_end());
        out.push_str("\nGO\n");
    }
    Ok(out)
}

/// Best-effort CREATE→ALTER rewrite on the first CREATE keyword.
fn rewrite_create_to_alter(definition: &str) -> String {
    let lower = definition.to_lowercase();
    if let Some(pos) = lower.find("create") {
        // Only rewrite if it looks like the module header (first non-comment token region).
        format!("{}ALTER{}", &definition[..pos], &definition[pos + 6..])
    } else {
        definition.to_string()
    }
}

/// `{ "schema.table": ["col", ...] }` shaped for CodeMirror schemaCompletionSource.
pub async fn completion_schema(
    conn: &mut SqlConn,
    database: &str,
) -> Result<serde_json::Value> {
    let db = quote_ident(database);
    let rows = conn
        .query_rows(&format!(
            "SELECT s.name, o.name, c.name
             FROM {db}.sys.objects o
             JOIN {db}.sys.schemas s ON o.schema_id = s.schema_id
             JOIN {db}.sys.columns c ON c.object_id = o.object_id
             WHERE o.type IN ('U','V') AND o.is_ms_shipped = 0
             ORDER BY s.name, o.name, c.column_id"
        ))
        .await?;
    let mut map = serde_json::Map::new();
    for r in rows {
        if r.len() < 3 {
            continue;
        }
        let key = if r[0].display == "dbo" {
            r[1].display.clone()
        } else {
            format!("{}.{}", r[0].display, r[1].display)
        };
        map.entry(key)
            .or_insert_with(|| serde_json::Value::Array(vec![]))
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::String(r[2].display.clone()));
    }
    Ok(serde_json::Value::Object(map))
}
