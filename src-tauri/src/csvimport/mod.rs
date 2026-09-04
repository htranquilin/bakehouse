//! CSV → table import (SSMS "Import Flat File", but multi-file).
//! Inspection samples each file to sniff the delimiter, detect a header row,
//! and infer column types; import creates the table and loads rows in batched
//! multi-row INSERTs with progress events.

use crate::error::{AppError, Result};
use crate::events;
use crate::sql::conn::SqlConn;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::Emitter;

const SAMPLE_ROWS: usize = 1000;
const PREVIEW_ROWS: usize = 5;
const INSERT_BATCH: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvColumn {
    pub name: String,
    pub sql_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvFileInfo {
    pub path: String,
    pub file_name: String,
    pub suggested_table: String,
    pub delimiter: String,
    pub has_header: bool,
    pub columns: Vec<CsvColumn>,
    pub sample_rows: Vec<Vec<String>>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSpec {
    pub path: String,
    pub table: String,
    pub delimiter: String,
    pub has_header: bool,
    pub columns: Vec<CsvColumn>,
}

pub fn scan_dir(dir: &Path) -> Result<Vec<String>> {
    let mut files: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|x| x.to_string_lossy().eq_ignore_ascii_case("csv"))
                    .unwrap_or(false)
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    files.sort();
    Ok(files)
}

fn read_decoded(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        // Not UTF-8: vendor exports are usually Windows-1252.
        Err(_) => Ok(encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()),
    }
}

fn sniff_delimiter(text: &str) -> u8 {
    let mut best = (b',', 0usize);
    for cand in [b',', b';', b'\t', b'|'] {
        // Count on the first few lines; require consistency across lines.
        let counts: Vec<usize> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(5)
            .map(|l| l.bytes().filter(|&b| b == cand).count())
            .collect();
        if counts.is_empty() {
            continue;
        }
        let min = *counts.iter().min().unwrap();
        if min > 0 && min > best.1 {
            best = (cand, min);
        }
    }
    best.0
}

fn sanitize_ident(raw: &str, fallback: &str) -> String {
    let mut s: String = raw
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        return fallback.to_string();
    }
    if s.chars().next().unwrap().is_ascii_digit() {
        format!("_{s}")
    } else {
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ColKind {
    Empty,
    Int,
    BigInt,
    Float,
    Date,
    DateTime,
    Text,
}

fn classify(value: &str) -> ColKind {
    let v = value.trim();
    if v.is_empty() {
        return ColKind::Empty;
    }
    if v.parse::<i64>().is_ok() {
        return if v.parse::<i32>().is_ok() { ColKind::Int } else { ColKind::BigInt };
    }
    if v.parse::<f64>().is_ok() && !v.eq_ignore_ascii_case("nan") && !v.eq_ignore_ascii_case("inf") {
        return ColKind::Float;
    }
    if chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_ok() {
        return ColKind::Date;
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M:%S%.f"] {
        if chrono::NaiveDateTime::parse_from_str(v, fmt).is_ok() {
            return ColKind::DateTime;
        }
    }
    ColKind::Text
}

fn merge(a: ColKind, b: ColKind) -> ColKind {
    use ColKind::*;
    match (a, b) {
        (Empty, x) | (x, Empty) => x,
        (x, y) if x == y => x,
        (Int, BigInt) | (BigInt, Int) => BigInt,
        (Int, Float) | (Float, Int) | (BigInt, Float) | (Float, BigInt) => Float,
        (Date, DateTime) | (DateTime, Date) => DateTime,
        _ => Text,
    }
}

fn sql_type_for(kind: ColKind, max_len: usize) -> String {
    match kind {
        ColKind::Int => "INT".into(),
        ColKind::BigInt => "BIGINT".into(),
        ColKind::Float => "FLOAT".into(),
        ColKind::Date => "DATE".into(),
        ColKind::DateTime => "DATETIME2".into(),
        ColKind::Empty | ColKind::Text => {
            let n = [50usize, 100, 255, 1000, 4000].iter().find(|&&n| max_len <= n).copied();
            match n {
                Some(n) => format!("NVARCHAR({n})"),
                None => "NVARCHAR(MAX)".into(),
            }
        }
    }
}

fn kind_of_sql_type(t: &str) -> ColKind {
    match t {
        "INT" => ColKind::Int,
        "BIGINT" => ColKind::BigInt,
        "FLOAT" => ColKind::Float,
        "DATE" => ColKind::Date,
        "DATETIME2" => ColKind::DateTime,
        _ => ColKind::Text,
    }
}

pub fn inspect_file(path: &Path) -> Result<CsvFileInfo> {
    inspect_file_with(path, None, None)
}

/// Re-inspection with user overrides (wizard delimiter/header toggles must
/// re-infer columns, not keep stale ones).
pub fn inspect_file_with(
    path: &Path,
    delimiter_override: Option<u8>,
    header_override: Option<bool>,
) -> Result<CsvFileInfo> {
    let text = read_decoded(path)?;
    if text.trim().is_empty() {
        return Err(AppError::runtime(
            format!("{}: file is empty", path.display()),
            None,
        ));
    }
    let delimiter = delimiter_override.unwrap_or_else(|| sniff_delimiter(&text));
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());

    let mut rows: Vec<Vec<String>> = vec![];
    for rec in reader.records().take(SAMPLE_ROWS + 1) {
        let rec = rec.map_err(|e| AppError::runtime(format!("{}: {e}", path.display()), None))?;
        rows.push(rec.iter().map(|s| s.to_string()).collect());
    }
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    if col_count == 0 {
        return Err(AppError::runtime(format!("{}: no columns found", path.display()), None));
    }

    // Header heuristic: every first-row cell is non-empty text, and at least
    // one later row has a cell that is NOT plain text (number/date), or the
    // first row's cells are all distinct.
    let first_all_text = rows[0].iter().all(|c| classify(c) == ColKind::Text);
    let data_has_typed = rows.iter().skip(1).any(|r| {
        r.iter().any(|c| !matches!(classify(c), ColKind::Text | ColKind::Empty))
    });
    let distinct = {
        let mut seen = std::collections::HashSet::new();
        rows[0].iter().all(|c| seen.insert(c.trim().to_lowercase()))
    };
    let has_header = header_override
        .unwrap_or(first_all_text && (data_has_typed || distinct) && rows.len() > 1);

    let data_rows: &[Vec<String>] = if has_header { &rows[1..] } else { &rows };

    let mut columns = Vec::with_capacity(col_count);
    for i in 0..col_count {
        let raw_name = if has_header {
            rows[0].get(i).cloned().unwrap_or_default()
        } else {
            String::new()
        };
        let mut kind = ColKind::Empty;
        let mut max_len = 1usize;
        for r in data_rows {
            let v = r.get(i).map(String::as_str).unwrap_or("");
            kind = merge(kind, classify(v));
            max_len = max_len.max(v.chars().count());
        }
        let name = sanitize_ident(&raw_name, &format!("column{}", i + 1));
        columns.push(CsvColumn { name, sql_type: sql_type_for(kind, max_len) });
    }
    // Dedupe column names.
    let mut seen: std::collections::HashMap<String, u32> = Default::default();
    for c in &mut columns {
        let n = seen.entry(c.name.to_lowercase()).or_insert(0);
        *n += 1;
        if *n > 1 {
            c.name = format!("{}_{}", c.name, n);
        }
    }

    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(CsvFileInfo {
        path: path.to_string_lossy().into_owned(),
        file_name: path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        suggested_table: sanitize_ident(&stem, "imported"),
        delimiter: (delimiter as char).to_string(),
        has_header,
        columns,
        sample_rows: data_rows.iter().take(PREVIEW_ROWS).cloned().collect(),
        size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    })
}

fn quote_ident(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

fn literal(value: &str, kind: ColKind, file: &str, row: usize, col: &str) -> Result<String> {
    let v = value.trim();
    if v.is_empty() {
        // Empty text stays an empty string; empty typed cells become NULL.
        return Ok(if kind == ColKind::Text { "N''".into() } else { "NULL".into() });
    }
    let bad = |want: &str| {
        AppError::runtime(
            format!("{file}, data row {row}, column [{col}]: '{v}' is not a valid {want}"),
            Some("Re-run the import with 'Import all columns as text' if this column is mixed.".into()),
        )
    };
    Ok(match kind {
        ColKind::Int | ColKind::BigInt => {
            v.parse::<i64>().map_err(|_| bad("integer"))?.to_string()
        }
        ColKind::Float => {
            let f: f64 = v.parse().map_err(|_| bad("number"))?;
            if !f.is_finite() {
                return Err(bad("number"));
            }
            f.to_string()
        }
        ColKind::Date | ColKind::DateTime => {
            if classify(v) != ColKind::Date && classify(v) != ColKind::DateTime {
                return Err(bad("date"));
            }
            format!("'{}'", v.replace('\'', "''"))
        }
        ColKind::Empty | ColKind::Text => format!("N'{}'", v.replace('\'', "''")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tmp(content: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("bh-csv-{}.csv", uuid::Uuid::new_v4().simple()));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn infers_types_and_header() {
        let p = write_tmp("id,name,amount,when\n1,Ada,10.5,2024-01-02\n2,Grace,3,2024-02-03\n");
        let info = inspect_file(&p).unwrap();
        assert!(info.has_header);
        assert_eq!(info.delimiter, ",");
        let types: Vec<&str> = info.columns.iter().map(|c| c.sql_type.as_str()).collect();
        assert_eq!(types, vec!["INT", "NVARCHAR(50)", "FLOAT", "DATE"]);
        assert_eq!(info.columns[1].name, "name");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn semicolon_no_header() {
        let p = write_tmp("1;2;3\n4;5;6\n");
        let info = inspect_file(&p).unwrap();
        assert_eq!(info.delimiter, ";");
        assert!(!info.has_header);
        assert_eq!(info.columns[0].name, "column1");
        assert_eq!(info.columns[0].sql_type, "INT");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn mixed_column_becomes_text_and_empty_is_nullable() {
        let p = write_tmp("a,b\n1,x\ntwo,\n");
        let info = inspect_file(&p).unwrap();
        assert!(info.columns[0].sql_type.starts_with("NVARCHAR"));
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn table_name_from_file_name() {
        let p = std::env::temp_dir().join("2024 sales export!.csv");
        std::fs::write(&p, "a\n1\n").unwrap();
        let info = inspect_file(&p).unwrap();
        assert_eq!(info.suggested_table, "_2024_sales_export");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn literals_escape_and_null() {
        assert_eq!(literal("it's", ColKind::Text, "f", 1, "c").unwrap(), "N'it''s'");
        assert_eq!(literal("", ColKind::Int, "f", 1, "c").unwrap(), "NULL");
        assert_eq!(literal("", ColKind::Text, "f", 1, "c").unwrap(), "N''");
        assert!(literal("abc", ColKind::Int, "f", 1, "c").is_err());
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_import(
    app: &tauri::AppHandle,
    conn: &mut SqlConn,
    database: &str,
    schema: &str,
    specs: &[ImportSpec],
    replace: bool,
    all_text: bool,
    job_id: &str,
) -> Result<Vec<String>> {
    conn.exec_simple(&format!("USE {}", quote_ident(database))).await?;
    let mut summary = vec![];

    for (file_index, spec) in specs.iter().enumerate() {
        let emit = |rows_done: u64, stage: &str| {
            let _ = app.emit(
                events::IMPORT_PROGRESS,
                serde_json::json!({
                    "jobId": job_id, "fileIndex": file_index, "totalFiles": specs.len(),
                    "fileName": spec.table, "rowsDone": rows_done, "stage": stage
                }),
            );
        };
        emit(0, "creating-table");

        let columns: Vec<CsvColumn> = if all_text {
            spec.columns
                .iter()
                .map(|c| CsvColumn { name: c.name.clone(), sql_type: "NVARCHAR(MAX)".into() })
                .collect()
        } else {
            spec.columns.clone()
        };
        let kinds: Vec<ColKind> = columns.iter().map(|c| kind_of_sql_type(&c.sql_type)).collect();

        let fq = format!("{}.{}", quote_ident(schema), quote_ident(&spec.table));
        if replace {
            conn.exec_simple(&format!("DROP TABLE IF EXISTS {fq}")).await?;
        }
        let col_defs: Vec<String> = columns
            .iter()
            .map(|c| format!("{} {} NULL", quote_ident(&c.name), c.sql_type))
            .collect();
        conn.exec_simple(&format!("CREATE TABLE {fq} (\n  {}\n)", col_defs.join(",\n  ")))
            .await
            .map_err(|e| {
                AppError::runtime(
                    format!("cannot create table {fq} for {}: {e}", spec.table),
                    Some("Does the table already exist? Enable 'Replace existing tables' to overwrite.".into()),
                )
            })?;

        // Stream the file in insert batches.
        let text = read_decoded(Path::new(&spec.path))?;
        let delimiter = *spec.delimiter.as_bytes().first().unwrap_or(&b',');
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(spec.has_header)
            .flexible(true)
            .from_reader(text.as_bytes());

        let col_list = columns.iter().map(|c| quote_ident(&c.name)).collect::<Vec<_>>().join(", ");
        let mut batch: Vec<String> = vec![];
        let mut rows_done: u64 = 0;
        emit(0, "inserting");

        for (row_idx, rec) in reader.records().enumerate() {
            let rec = rec.map_err(|e| AppError::runtime(format!("{}: {e}", spec.path), None))?;
            let mut values = Vec::with_capacity(columns.len());
            for (i, kind) in kinds.iter().enumerate() {
                let v = rec.get(i).unwrap_or("");
                values.push(literal(v, *kind, &spec.path, row_idx + 1, &columns[i].name)?);
            }
            batch.push(format!("({})", values.join(", ")));
            if batch.len() >= INSERT_BATCH {
                conn.exec_simple(&format!("INSERT INTO {fq} ({col_list}) VALUES {}", batch.join(",")))
                    .await?;
                rows_done += batch.len() as u64;
                batch.clear();
                emit(rows_done, "inserting");
            }
        }
        if !batch.is_empty() {
            conn.exec_simple(&format!("INSERT INTO {fq} ({col_list}) VALUES {}", batch.join(",")))
                .await?;
            rows_done += batch.len() as u64;
        }
        emit(rows_done, "file-done");
        summary.push(format!("{fq}: {rows_done} rows"));
    }
    Ok(summary)
}
