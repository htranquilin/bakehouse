//! Binary serialization of a row window for `tauri::ipc::Response`.
//!
//! Layout (little-endian), decoded by src/lib/binary.ts — keep in sync:
//!   u32 startRow, u32 rowCount, u16 colCount
//!   then rowCount * colCount cells: u8 kind, u32 byteLen, byteLen utf8 bytes

use super::buffer::ResultSetBuffer;

pub fn serialize_window(buf: &ResultSetBuffer, start: u32, count: u32) -> Vec<u8> {
    let display_len = buf.sort_perm.as_ref().map_or(buf.rows.len(), Vec::len) as u32;
    let end = (start + count).min(display_len);
    let start = start.min(end);
    let col_count = buf.columns.len() as u16;

    let mut out = Vec::with_capacity(1024 + (end - start) as usize * col_count as usize * 16);
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&(end - start).to_le_bytes());
    out.extend_from_slice(&col_count.to_le_bytes());

    for display_row in start..end {
        let Some(phys) = buf.physical_row(display_row) else { break };
        for cell in &buf.rows[phys] {
            out.push(cell.kind as u8);
            let bytes = cell.display.as_bytes();
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(bytes);
        }
    }
    out
}
