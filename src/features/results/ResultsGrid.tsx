import {
  CompactSelection,
  DataEditor,
  GridCellKind,
  type CellClickedEventArgs,
  type DataEditorRef,
  type GridCell,
  type GridColumn,
  type GridSelection,
  type Item,
} from "@glideapps/glide-data-grid";
import "@glideapps/glide-data-grid/dist/index.css";
import { useCallback, useMemo, useRef, useState } from "react";
import { CELL_KIND } from "../../lib/binary";
import * as ipc from "../../lib/ipc";
import { RowWindowCache } from "../../lib/rowCache";
import type { ResultSetMeta } from "../../stores/resultsStore";

const EMPTY_SELECTION: GridSelection = {
  columns: CompactSelection.empty(),
  rows: CompactSelection.empty(),
};

interface CopyMenu {
  x: number;
  y: number;
  cell: [number, number];
}

export function ResultsGrid({ meta }: { meta: ResultSetMeta }) {
  const gridRef = useRef<DataEditorRef>(null);
  const [sort, setSort] = useState<{ col: number; desc: boolean } | null>(null);
  const [cellViewer, setCellViewer] = useState<{ display: string } | null>(null);
  const [selection, setSelection] = useState<GridSelection>(EMPTY_SELECTION);
  const [menu, setMenu] = useState<CopyMenu | null>(null);

  // Cheap full re-render on window arrival; Glide only repaints visible cells.
  const [, forceRender] = useState(0);
  const cache = useMemo(
    () => new RowWindowCache(meta.resultSetId, () => forceRender((n) => n + 1)),
    [meta.resultSetId],
  );

  const columns = useMemo<GridColumn[]>(
    () =>
      meta.columns.map((c) => ({
        title: c.name || "(no name)",
        id: c.name,
        width: Math.min(320, Math.max(90, c.name.length * 9 + 40)),
      })),
    [meta.columns],
  );
  const [colWidths, setColWidths] = useState<Record<number, number>>({});
  const sizedColumns = useMemo(
    () => columns.map((c, i) => (colWidths[i] ? { ...c, width: colWidths[i] } : c)),
    [columns, colWidths],
  );

  const getCellContent = useCallback(
    ([col, row]: Item): GridCell => {
      const cell = cache.get(row, col);
      if (!cell) {
        return { kind: GridCellKind.Loading, allowOverlay: false };
      }
      if (cell.kind === CELL_KIND.null) {
        return {
          kind: GridCellKind.Text,
          data: "",
          displayData: "NULL",
          allowOverlay: false,
          themeOverride: { textDark: cssVar("--bh-text-faint") },
          contentAlign: "center",
        };
      }
      const isNumber = cell.kind === CELL_KIND.number;
      return {
        kind: GridCellKind.Text,
        data: cell.display,
        displayData: cell.display.length > 260 ? `${cell.display.slice(0, 260)}…` : cell.display,
        allowOverlay: false,
        contentAlign: isNumber ? "right" : "left",
      };
    },
    [cache],
  );

  const onHeaderClicked = useCallback(
    (col: number) => {
      const next =
        sort?.col === col ? (sort.desc ? null : { col, desc: true }) : { col, desc: false };
      setSort(next);
      void ipc
        .resultsSort(meta.resultSetId, next ? next.col : null, next?.desc ?? false)
        .then(() => {
          cache.clear();
          forceRender((n) => n + 1);
        });
    },
    [sort, meta.resultSetId, cache],
  );

  const onCellActivated = useCallback(
    ([col, row]: Item) => {
      void ipc.resultsCell(meta.resultSetId, row, col).then((c) => setCellViewer({ display: c.display }));
    },
    [meta.resultSetId],
  );

  const onCellContextMenu = useCallback(([col, row]: Item, event: CellClickedEventArgs) => {
    event.preventDefault();
    setMenu({
      x: event.bounds.x + event.localEventX,
      y: event.bounds.y + event.localEventY,
      cell: [col, row],
    });
  }, []);

  // Rect of the current drag-selection, if it spans more than one cell.
  const selectionRect = useMemo(() => {
    const r = selection.current?.range;
    if (!r || (r.width <= 1 && r.height <= 1)) return null;
    return { rowStart: r.y, rowEnd: r.y + r.height - 1, colStart: r.x, colEnd: r.x + r.width - 1 };
  }, [selection]);

  const lastRow = Math.max(0, Math.min(meta.rowCount, rowCapDisplay(meta)) - 1);
  const lastCol = Math.max(0, meta.columns.length - 1);
  const copyRect = useCallback(
    (rect: ipc.CopyRect, withHeader = false) => {
      void ipc.resultsCopyTsv(meta.resultSetId, rect, withHeader);
      setMenu(null);
    },
    [meta.resultSetId],
  );

  return (
    <div className="results-grid">
      <DataEditor
        ref={gridRef}
        columns={sizedColumns}
        rows={Math.min(meta.rowCount, rowCapDisplay(meta))}
        getCellContent={getCellContent}
        onHeaderClicked={onHeaderClicked}
        onCellActivated={onCellActivated}
        onCellContextMenu={onCellContextMenu}
        gridSelection={selection}
        onGridSelectionChange={setSelection}
        onColumnResize={(_c, w, i) => setColWidths((s) => ({ ...s, [i]: w }))}
        smoothScrollX
        smoothScrollY
        rowHeight={26}
        headerHeight={30}
        getCellsForSelection={true}
        width="100%"
        height="100%"
        theme={glideTheme()}
      />
      {menu && (
        <div className="context-menu-overlay" onClick={() => setMenu(null)} onContextMenu={(e) => { e.preventDefault(); setMenu(null); }}>
          <div className="context-menu" style={{ left: menu.x, top: menu.y }} onClick={(e) => e.stopPropagation()}>
            {selectionRect && (
              <>
                <button className="context-menu-item" onClick={() => copyRect(selectionRect)}>
                  Copy selection
                </button>
                <button className="context-menu-item" onClick={() => copyRect(selectionRect, true)}>
                  Copy selection with headers
                </button>
              </>
            )}
            <button
              className="context-menu-item"
              onClick={() =>
                copyRect({ rowStart: menu.cell[1], rowEnd: menu.cell[1], colStart: menu.cell[0], colEnd: menu.cell[0] })
              }
            >
              Copy cell
            </button>
            <button
              className="context-menu-item"
              onClick={() => copyRect({ rowStart: menu.cell[1], rowEnd: menu.cell[1], colStart: 0, colEnd: lastCol })}
            >
              Copy row
            </button>
            <button
              className="context-menu-item"
              onClick={() =>
                copyRect({ rowStart: 0, rowEnd: lastRow, colStart: menu.cell[0], colEnd: menu.cell[0] }, true)
              }
            >
              Copy column
            </button>
            <button
              className="context-menu-item"
              onClick={() => {
                setMenu(null);
                void ipc
                  .resultsCell(meta.resultSetId, menu.cell[1], menu.cell[0])
                  .then((c) => setCellViewer({ display: c.display }));
              }}
            >
              View cell…
            </button>
          </div>
        </div>
      )}
      {cellViewer && (
        <div className="modal-overlay" onClick={() => setCellViewer(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>Cell value</h2>
            <pre className="modal-logs">{cellViewer.display || "(empty)"}</pre>
            <button className="btn-primary" onClick={() => setCellViewer(null)}>
              Close
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** Grid shows at most what the Rust buffer holds (cap), even if more streamed by. */
function rowCapDisplay(meta: ResultSetMeta): number {
  return meta.truncated ? 10000 : Number.MAX_SAFE_INTEGER;
}

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function glideTheme() {
  return {
    accentColor: cssVar("--bh-accent"),
    accentLight: cssVar("--bh-syn-selection"),
    textDark: cssVar("--bh-text"),
    textMedium: cssVar("--bh-text-muted"),
    textLight: cssVar("--bh-text-faint"),
    textHeader: cssVar("--bh-text-muted"),
    bgCell: cssVar("--bh-surface-sunken"),
    bgCellMedium: cssVar("--bh-surface"),
    bgHeader: cssVar("--bh-surface"),
    bgHeaderHasFocus: cssVar("--bh-surface-raised"),
    bgHeaderHovered: cssVar("--bh-surface-raised"),
    borderColor: cssVar("--bh-border"),
    fontFamily: "JetBrains Mono, monospace",
    baseFontStyle: "13px",
    headerFontStyle: "600 12px",
  };
}
