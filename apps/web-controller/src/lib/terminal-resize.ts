export const MIN_REMOTE_TERMINAL_ROWS = 8;
export const MIN_REMOTE_TERMINAL_COLS = 20;

export interface TerminalGridSize {
  rows: number;
  cols: number;
}

export function shouldSendTerminalResize(
  next: TerminalGridSize,
  last: TerminalGridSize,
): boolean {
  if (next.rows < MIN_REMOTE_TERMINAL_ROWS) return false;
  if (next.cols < MIN_REMOTE_TERMINAL_COLS) return false;
  return next.rows !== last.rows || next.cols !== last.cols;
}
