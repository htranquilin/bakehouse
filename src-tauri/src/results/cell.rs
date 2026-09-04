//! A single result cell, kept typed in Rust; display formatting happens once,
//! at ingest, but NULL-ness and broad type class survive for the UI and export.

use serde::Serialize;
use tiberius::ColumnData;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CellKind {
    Null = 0,
    Text = 1,
    Number = 2,
    Bool = 3,
    Binary = 4,
    DateTime = 5,
    Guid = 6,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub kind: CellKind,
    /// Display string (empty for NULL). Binary is rendered as 0x-prefixed hex,
    /// truncated at ingest to keep buffers sane; full value fetch re-queries.
    pub display: String,
}

const BINARY_PREVIEW_BYTES: usize = 64;

impl Cell {
    pub fn null() -> Self {
        Self { kind: CellKind::Null, display: String::new() }
    }

    pub fn from_column_data(data: &ColumnData<'static>) -> Self {
        fn some<T: ToString>(kind: CellKind, v: &Option<T>) -> Cell {
            match v {
                Some(v) => Cell { kind, display: v.to_string() },
                None => Cell::null(),
            }
        }
        use CellKind::*;
        match data {
            ColumnData::U8(v) => some(Number, v),
            ColumnData::I16(v) => some(Number, v),
            ColumnData::I32(v) => some(Number, v),
            ColumnData::I64(v) => some(Number, v),
            ColumnData::F32(v) => some(Number, v),
            ColumnData::F64(v) => some(Number, v),
            ColumnData::Numeric(v) => some(Number, v),
            ColumnData::Bit(v) => match v {
                Some(b) => Cell { kind: Bool, display: if *b { "1".into() } else { "0".into() } },
                None => Cell::null(),
            },
            ColumnData::String(v) => match v {
                Some(s) => Cell { kind: Text, display: s.to_string() },
                None => Cell::null(),
            },
            ColumnData::Guid(v) => some(Guid, v),
            ColumnData::Xml(v) => match v {
                Some(x) => Cell { kind: Text, display: x.to_string() },
                None => Cell::null(),
            },
            ColumnData::Binary(v) => match v {
                Some(bytes) => {
                    let preview: String = bytes
                        .iter()
                        .take(BINARY_PREVIEW_BYTES)
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    let ellipsis = if bytes.len() > BINARY_PREVIEW_BYTES { "…" } else { "" };
                    Cell { kind: Binary, display: format!("0x{preview}{ellipsis}") }
                }
                None => Cell::null(),
            },
            // Temporal types: use tiberius' chrono conversions for display.
            other => Self::temporal(other),
        }
    }

    fn temporal(data: &ColumnData<'static>) -> Self {
        use tiberius::FromSql;
        if let Ok(Some(dt)) = chrono::NaiveDateTime::from_sql(data) {
            return Cell { kind: CellKind::DateTime, display: dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string() };
        }
        if let Ok(Some(d)) = chrono::NaiveDate::from_sql(data) {
            return Cell { kind: CellKind::DateTime, display: d.format("%Y-%m-%d").to_string() };
        }
        if let Ok(Some(t)) = chrono::NaiveTime::from_sql(data) {
            return Cell { kind: CellKind::DateTime, display: t.format("%H:%M:%S%.3f").to_string() };
        }
        if let Ok(Some(dto)) = chrono::DateTime::<chrono::FixedOffset>::from_sql(data) {
            return Cell { kind: CellKind::DateTime, display: dto.format("%Y-%m-%d %H:%M:%S%.3f %:z").to_string() };
        }
        // Whatever it was, it was NULL or unformattable.
        Cell::null()
    }
}
