use super::cell::Cell;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColMeta {
    pub name: String,
    pub sql_type: String,
}

pub struct ResultSetBuffer {
    pub columns: Vec<ColMeta>,
    pub rows: Vec<Vec<Cell>>,
    /// True when the row cap stopped buffering; the remainder was drained
    /// (counted but not stored).
    pub truncated: bool,
    /// Rows seen on the wire (>= rows.len() when truncated).
    pub total_seen: u64,
    /// Row display order after a client-side sort (indices into `rows`).
    pub sort_perm: Option<Vec<u32>>,
}

impl ResultSetBuffer {
    pub fn new(columns: Vec<ColMeta>) -> Self {
        Self { columns, rows: Vec::new(), truncated: false, total_seen: 0, sort_perm: None }
    }

    /// Map a display row index through the active sort permutation.
    pub fn physical_row(&self, display_row: u32) -> Option<usize> {
        match &self.sort_perm {
            Some(perm) => perm.get(display_row as usize).map(|&i| i as usize),
            None => {
                let i = display_row as usize;
                (i < self.rows.len()).then_some(i)
            }
        }
    }
}
