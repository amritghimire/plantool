import type { Heading } from "../types";

export interface Slide {
  index: number;
  title: string;
  start: number;
  end: number;
  content: string;
}

const HEADING_RE = /^\s{0,3}#{1,6}\s/;

function hasBody(lines: string[]): boolean {
  return lines.some((l) => l.trim() !== "" && !HEADING_RE.test(l));
}

/** Split a document into slides at the shallowest heading level that occurs at least twice (preferring h2). */
export function splitSlides(content: string, headings: Heading[]): Slide[] {
  const lines = content.split("\n");
  const total = lines.length;
  const whole = (title: string): Slide[] => [{ index: 0, title, start: 1, end: total, content }];
  if (!headings.length) return whole("Document");
  const counts = new Map<number, number>();
  for (const h of headings) counts.set(h.level, (counts.get(h.level) ?? 0) + 1);
  const level = [2, 3, 1, 4, 5, 6].find((l) => (counts.get(l) ?? 0) >= 2);
  if (!level) return whole(headings[0].text);
  const cuts = [...new Set(headings.filter((h) => h.level <= level).map((h) => h.line))].sort((a, b) => a - b);
  const bounds = cuts[0] === 1 ? cuts : [1, ...cuts];
  const segments: { start: number; end: number }[] = bounds.map((s, i) => ({ start: s, end: i + 1 < bounds.length ? bounds[i + 1] - 1 : total }));
  const merged: { start: number; end: number }[] = [];
  for (const seg of segments) {
    const body = lines.slice(seg.start - 1, seg.end);
    if (!body.some((l) => l.trim() !== "")) continue;
    const prev = merged[merged.length - 1];
    if (prev && !hasBody(lines.slice(prev.start - 1, prev.end))) prev.end = seg.end;
    else merged.push({ ...seg });
  }
  return merged.map((seg, index) => {
    const h = headings.find((x) => x.line >= seg.start && x.line <= seg.end);
    return { index, title: h?.text ?? "Overview", start: seg.start, end: seg.end, content: lines.slice(seg.start - 1, seg.end).join("\n") };
  });
}

export function slideForLine(slides: Slide[], line: number): number {
  const i = slides.findIndex((s) => line >= s.start && line <= s.end);
  return i < 0 ? 0 : i;
}
