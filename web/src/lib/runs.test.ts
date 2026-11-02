import { durationBars, formatDuration, stopReason } from "./runs";
import type { Run } from "../types";

const run = (id: string, start: string, end: string | null, over: Partial<Run> = {}): Run => ({ id, provider: "codex", provider_session_id: null, stage: "implementing", cwd: "/r", status: end ? "stopped" : "running", model: null, started_at: start, ended_at: end, error: null, seq: 0, ...over });

describe("duration bars", () => {
  it("scales each run against the longest one and measures live runs to now", () => {
    const now = Date.parse("2026-01-01T04:00:00Z");
    const bars = durationBars([
      run("long", "2026-01-01T00:00:00Z", "2026-01-01T02:00:00Z"),
      run("half", "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z"),
      run("live", "2026-01-01T03:30:00Z", null),
    ], now);
    expect(bars.get("long")).toEqual({ ms: 7_200_000, pct: 100 });
    expect(bars.get("half")?.pct).toBe(50);
    expect(bars.get("live")?.pct).toBe(25);
  });

  it("gives zero width when nothing has run yet", () => {
    const at = "2026-01-01T00:00:00Z";
    expect(durationBars([run("a", at, at)], Date.parse(at)).get("a")).toEqual({ ms: 0, pct: 0 });
  });

  it("formats durations compactly", () => {
    expect(formatDuration(4_000)).toBe("4s");
    expect(formatDuration(12 * 60_000)).toBe("12m");
    expect(formatDuration((8 * 60 + 29) * 60_000)).toBe("8h 29m");
    expect(formatDuration(50 * 3_600_000)).toBe("2d 2h");
  });

  it("only reports a stop reason the run carries", () => {
    expect(stopReason(run("a", "", "x"))).toBeNull();
    expect(stopReason(run("a", "", "x", { idle_stopped: true }))).toBe("Stopped after sitting idle");
    expect(stopReason(run("a", "", "x", { status: "failed", error: "exit 1" }))).toBe("exit 1");
  });
});
