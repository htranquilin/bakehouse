// Decoder for the binary row-window format produced by
// src-tauri/src/results/window.rs — keep the layout in sync:
//   u32 startRow, u32 rowCount, u16 colCount   (little-endian)
//   then rowCount*colCount cells: u8 kind, u32 byteLen, byteLen utf8 bytes

export const CELL_KIND = {
  null: 0,
  text: 1,
  number: 2,
  bool: 3,
  binary: 4,
  dateTime: 5,
  guid: 6,
} as const;

export interface DecodedCell {
  kind: number;
  display: string;
}

export interface DecodedWindow {
  startRow: number;
  rows: DecodedCell[][];
}

const utf8 = new TextDecoder();

export function decodeWindow(buf: ArrayBuffer): DecodedWindow {
  const view = new DataView(buf);
  const bytes = new Uint8Array(buf);
  const startRow = view.getUint32(0, true);
  const rowCount = view.getUint32(4, true);
  const colCount = view.getUint16(8, true);
  let off = 10;

  const rows: DecodedCell[][] = new Array(rowCount);
  for (let r = 0; r < rowCount; r++) {
    const row: DecodedCell[] = new Array(colCount);
    for (let c = 0; c < colCount; c++) {
      const kind = view.getUint8(off);
      const len = view.getUint32(off + 1, true);
      off += 5;
      row[c] = { kind, display: len ? utf8.decode(bytes.subarray(off, off + len)) : "" };
      off += len;
    }
    rows[r] = row;
  }
  return { startRow, rows };
}
