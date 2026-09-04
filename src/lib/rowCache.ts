// Non-reactive window cache between the Glide grid and the Rust result buffer.
// getCellContent reads synchronously; misses trigger a coalesced fetch of the
// surrounding window, and arrivals invalidate the grid via onWindowLoaded.

import { decodeWindow, type DecodedCell } from "./binary";
import { resultsWindow } from "./ipc";

const WINDOW_SIZE = 500;
const MAX_CACHED_WINDOWS = 40;

export class RowWindowCache {
  private windows = new Map<number, DecodedCell[][]>(); // windowIndex -> rows
  private lru: number[] = [];
  private pending = new Set<number>();

  constructor(
    private resultSetId: number,
    public onWindowLoaded: () => void,
  ) {}

  /** Synchronous read; undefined means "loading" (a fetch has been kicked off). */
  get(row: number, col: number): DecodedCell | undefined {
    const w = Math.floor(row / WINDOW_SIZE);
    const win = this.windows.get(w);
    if (win) {
      this.touch(w);
      return win[row - w * WINDOW_SIZE]?.[col];
    }
    void this.fetch(w);
    return undefined;
  }

  /** Drop everything (sort changed, re-run). */
  clear() {
    this.windows.clear();
    this.lru = [];
    this.pending.clear();
  }

  private touch(w: number) {
    const i = this.lru.indexOf(w);
    if (i >= 0) this.lru.splice(i, 1);
    this.lru.push(w);
  }

  private async fetch(w: number) {
    if (this.pending.has(w) || this.windows.has(w)) return;
    this.pending.add(w);
    try {
      const buf = await resultsWindow(this.resultSetId, w * WINDOW_SIZE, WINDOW_SIZE);
      const decoded = decodeWindow(buf);
      this.windows.set(w, decoded.rows);
      this.touch(w);
      while (this.lru.length > MAX_CACHED_WINDOWS) {
        const evict = this.lru.shift();
        if (evict !== undefined) this.windows.delete(evict);
      }
      this.onWindowLoaded();
    } catch {
      // released result set or transient error; the grid will retry on scroll
    } finally {
      this.pending.delete(w);
    }
  }
}
