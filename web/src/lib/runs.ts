import type { Run } from "../types";

export function runDurationMs(run: Run, now: number): number {
  const start = Date.parse(run.started_at);
  const end = run.ended_at ? Date.parse(run.ended_at) : now;
  if (Number.isNaN(start) || Number.isNaN(end)) return 0;
  return Math.max(0, end - start);
}

export function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, "0")}m`;
  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

export function durationBars(runs: Run[], now: number): Map<string, { ms: number; pct: number }> {
  const ms = runs.map((r) => runDurationMs(r, now));
  const longest = Math.max(0, ...ms);
  return new Map(
    runs.map((r, i) => [r.id, { ms: ms[i], pct: longest ? (ms[i] / longest) * 100 : 0 }]),
  );
}

export function stopReason(run: Run): string | null {
  if (run.error) return run.error;
  if (run.status === "stopped" && run.idle_stopped) return "Stopped after sitting idle";
  return null;
}
