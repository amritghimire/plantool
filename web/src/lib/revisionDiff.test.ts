import { revisionDiff } from "./revisionDiff";
it("shows changed tasks and retains context", () => {
  const result = revisionDiff("# Plan\n- [ ] A", "# Plan\n- [x] A\n- [ ] B");
  expect(result).toContain("  # Plan");
  expect(result).toContain("- - [ ] A");
  expect(result).toContain("+ - [x] A");
  expect(result).toContain("+ - [ ] B");
});
