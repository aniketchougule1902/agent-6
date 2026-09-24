type Candidate = { symbol: string; quality: number; checked_ms: number; phase: string; blockers: string[] };

export function rankedCandidates<T extends Candidate>(rows: T[], now: number): T[] {
  return rows.filter(row => Number.isFinite(row.quality) && row.quality >= 0.90 && row.quality <= 1
    && row.checked_ms > 0 && now >= row.checked_ms && now - row.checked_ms <= 180_000
    && row.phase === "candidate: confirm live flow" && row.blockers.length === 0)
    .sort((a, b) => b.quality - a.quality || a.symbol.localeCompare(b.symbol));
}
