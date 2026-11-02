export function cleanPaths(entries: string[]): string[] {
  const seen = new Set<string>();
  for (const part of entries.flatMap((entry) => entry.split(","))) {
    const path = part.replace(/`/g, "").trim();
    if (path) seen.add(path);
  }
  return [...seen];
}

const covers = (planned: string, path: string) =>
  planned === path || (planned.endsWith("*") && path.startsWith(planned.slice(0, -1)));

export function driftRows(planned: string[], changed: string[]) {
  const plan = cleanPaths(planned);
  const files = cleanPaths(changed);
  return {
    outside: files.filter((f) => !plan.some((p) => covers(p, f))),
    touched: plan.filter((p) => files.some((f) => covers(p, f))),
    untouched: plan.filter((p) => !files.some((f) => covers(p, f))),
  };
}
