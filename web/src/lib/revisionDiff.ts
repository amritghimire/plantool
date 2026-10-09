export function revisionDiff(before: string, after: string): string {
  const a = before.split("\n");
  const b = after.split("\n");
  if (a.length * b.length > 4_000_000) return a.map((s) => `- ${s}`).concat(b.map((s) => `+ ${s}`)).join("\n");
  const rows = Array.from({ length: a.length + 1 }, () => new Uint32Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i--) for (let j = b.length - 1; j >= 0; j--) rows[i][j] = a[i] === b[j] ? rows[i + 1][j + 1] + 1 : Math.max(rows[i + 1][j], rows[i][j + 1]);
  const out: string[] = [];
  let i = 0, j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) { out.push(`  ${a[i++]}`); j++; }
    else if (j < b.length && (i === a.length || rows[i][j + 1] >= rows[i + 1][j])) out.push(`+ ${b[j++]}`);
    else out.push(`- ${a[i++]}`);
  }
  return out.join("\n");
}
