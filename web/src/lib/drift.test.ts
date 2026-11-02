import { cleanPaths, driftRows } from "./drift";

describe("drift rows", () => {
  it("strips markdown backticks, splits multi-path cells and de-duplicates", () => {
    expect(cleanPaths(["web/a.tsx", "`web/a.tsx`", "web/b.tsx`, `web/c.tsx", " ", "`web/b.tsx`"])).toEqual(["web/a.tsx", "web/b.tsx", "web/c.tsx"]);
  });

  it("sorts planned and changed files into outside, touched and untouched", () => {
    const rows = driftRows(["`web/a.tsx`", "web/a.tsx", "docs/*", "web/z.tsx"], ["web/a.tsx", "docs/x.md", "Cargo.lock", "Cargo.lock"]);
    expect(rows.outside).toEqual(["Cargo.lock"]);
    expect(rows.touched).toEqual(["web/a.tsx", "docs/*"]);
    expect(rows.untouched).toEqual(["web/z.tsx"]);
  });
});
