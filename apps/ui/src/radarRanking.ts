type Candidate = {
  symbol: string;
  quality: number;
  checked_ms: number;
  phase: string;
  blockers: string[];
  stability_cycles: number;
  last_qualified_ms: number;
  peak_quality: number;
};

function fresh(row: Candidate, now: number): boolean {
  return row.checked_ms > 0 && now >= row.checked_ms && now - row.checked_ms <= 180_000;
}

function active(row: Candidate, now: number): boolean {
  return Number.isFinite(row.quality) && row.quality >= 0.90 && row.quality <= 1
    && fresh(row, now) && row.stability_cycles >= 2
    && row.phase === "candidate: confirm live flow" && row.blockers.length === 0;
}

export function rankedCandidates<T extends Candidate>(rows: T[], now: number): T[] {
  return rows.filter(row => active(row, now))
    .sort((a, b) => b.quality - a.quality || b.stability_cycles - a.stability_cycles || a.symbol.localeCompare(b.symbol));
}

export function revalidatingCandidates<T extends Candidate>(rows: T[], now: number): T[] {
  return rows.filter(row => !active(row, now)
    && fresh(row, now)
    && row.last_qualified_ms > 0
    && now >= row.last_qualified_ms
    && now - row.last_qualified_ms <= 240_000)
    .sort((a, b) => b.peak_quality - a.peak_quality || b.last_qualified_ms - a.last_qualified_ms || a.symbol.localeCompare(b.symbol));
}
