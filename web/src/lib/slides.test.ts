import { splitSlides, slideForLine } from "./slides";

const doc = ["# Research: X", "", "Intro line.", "", "## How it works", "", "flow", "", "### Detail", "", "more", "", "## Key components", "", "- a", "- b", ""].join("\n");
const headings = [
  { line: 1, level: 1, text: "Research: X" },
  { line: 5, level: 2, text: "How it works" },
  { line: 9, level: 3, text: "Detail" },
  { line: 13, level: 2, text: "Key components" },
];

describe("splitSlides", () => {
  it("splits at h2 and keeps the intro as a cover slide", () => {
    const s = splitSlides(doc, headings);
    expect(s.map((x) => [x.title, x.start, x.end])).toEqual([
      ["Research: X", 1, 4],
      ["How it works", 5, 12],
      ["Key components", 13, 17],
    ]);
    expect(s[1].content.startsWith("## How it works")).toBe(true);
    expect(slideForLine(s, 15)).toBe(2);
  });

  it("merges a title-only cover into the next slide", () => {
    const d = ["# Plan", "", "## A", "x", "## B", "y"].join("\n");
    const s = splitSlides(d, [
      { line: 1, level: 1, text: "Plan" },
      { line: 3, level: 2, text: "A" },
      { line: 5, level: 2, text: "B" },
    ]);
    expect(s.map((x) => x.title)).toEqual(["Plan", "B"]);
    expect(s[0].start).toBe(1);
    expect(s[0].end).toBe(4);
  });

  it("falls back to one slide without repeated headings", () => {
    const s = splitSlides("# Only\n\ntext\n", [{ line: 1, level: 1, text: "Only" }]);
    expect(s).toHaveLength(1);
    expect(s[0].title).toBe("Only");
  });
});
