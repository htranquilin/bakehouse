//! CSV export (fully Rust-side) and TSV clipboard text.

use super::buffer::ResultSetBuffer;
use super::cell::CellKind;
use crate::error::{AppError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CsvOptions {
    pub separator: String,
    pub quote: String,
    pub include_header: bool,
    /// "utf-8" | "utf-8-bom" | "windows-1252"
    pub encoding: String,
    pub null_as: String,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            separator: ",".into(),
            quote: "\"".into(),
            include_header: true,
            encoding: "utf-8".into(),
            null_as: String::new(),
        }
    }
}

pub fn export_csv(buf: &ResultSetBuffer, path: &std::path::Path, opts: &CsvOptions) -> Result<u64> {
    let sep = *opts.separator.as_bytes().first().unwrap_or(&b',');
    let quote = *opts.quote.as_bytes().first().unwrap_or(&b'"');

    let mut writer = csv::WriterBuilder::new()
        .delimiter(sep)
        .quote(quote)
        .from_writer(Vec::new());

    if opts.include_header {
        writer
            .write_record(buf.columns.iter().map(|c| c.name.as_str()))
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }

    let display_len = buf.sort_perm.as_ref().map_or(buf.rows.len(), Vec::len) as u32;
    let mut written = 0u64;
    for display_row in 0..display_len {
        let Some(phys) = buf.physical_row(display_row) else { continue };
        writer
            .write_record(buf.rows[phys].iter().map(|c| match c.kind {
                CellKind::Null => opts.null_as.as_str(),
                _ => c.display.as_str(),
            }))
            .map_err(|e| AppError::Internal(e.to_string()))?;
        written += 1;
    }

    let utf8 = writer.into_inner().map_err(|e| AppError::Internal(e.to_string()))?;
    let bytes: Vec<u8> = match opts.encoding.as_str() {
        "utf-8-bom" => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(&utf8);
            v
        }
        "windows-1252" => {
            let (encoded, _, _) =
                encoding_rs::WINDOWS_1252.encode(std::str::from_utf8(&utf8).unwrap_or(""));
            encoded.into_owned()
        }
        _ => utf8,
    };
    std::fs::write(path, bytes)?;
    Ok(written)
}

/// Inclusive cell rectangle in DISPLAY coordinates (sort order respected).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyRect {
    pub row_start: u32,
    pub row_end: u32,
    pub col_start: u32,
    pub col_end: u32,
}

/// TSV for the clipboard (rect = None copies everything). Tabs/newlines inside
/// values are replaced with spaces (clipboard TSV has no quoting convention).
pub fn to_tsv(buf: &ResultSetBuffer, rect: Option<CopyRect>, include_header: bool) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let display_len = buf.sort_perm.as_ref().map_or(buf.rows.len(), Vec::len) as u32;
    let col_len = buf.columns.len() as u32;
    let rect = rect.unwrap_or(CopyRect {
        row_start: 0,
        row_end: display_len.saturating_sub(1),
        col_start: 0,
        col_end: col_len.saturating_sub(1),
    });
    let row_end = rect.row_end.min(display_len.saturating_sub(1));
    let col_end = rect.col_end.min(col_len.saturating_sub(1));
    let cols = rect.col_start..=col_end;

    let mut out = String::new();
    if include_header {
        out.push_str(
            &cols
                .clone()
                .filter_map(|c| buf.columns.get(c as usize))
                .map(|c| clean(&c.name))
                .collect::<Vec<_>>()
                .join("\t"),
        );
        out.push('\n');
    }
    for display_row in rect.row_start..=row_end {
        let Some(phys) = buf.physical_row(display_row) else { continue };
        let line = cols
            .clone()
            .filter_map(|c| buf.rows[phys].get(c as usize))
            .map(|c| match c.kind {
                CellKind::Null => "NULL".to_string(),
                _ => clean(&c.display),
            })
            .collect::<Vec<_>>()
            .join("\t");
        out.push_str(&line);
        out.push('\n');
    }
    // A single cell copies as the bare value, no trailing newline.
    if rect.row_start == row_end && rect.col_start == col_end && !include_header {
        out.pop();
    }
    out
}
