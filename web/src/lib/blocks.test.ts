import { blockForLine, collectBlocks } from "./blocks";

const DOC = `# Plan

Intro paragraph
continues here.

- [ ] one
- [x] two

\`\`\`rust
fn x() {}
\`\`\`
`;

describe("collectBlocks", () => {
  it("maps lines to the innermost attachable block", () => {
    const blocks = collectBlocks(DOC);
    expect(blockForLine(blocks, 1)?.type).toBe("heading");
    expect(blockForLine(blocks, 4)?.start).toBe(3);
    expect(blockForLine(blocks, 6)?.type).toBe("listItem");
    expect(blockForLine(blocks, 7)?.start).toBe(7);
    expect(blockForLine(blocks, 10)?.type).toBe("code");
    expect(blockForLine(blocks, 2)).toBeNull();
  });
});
