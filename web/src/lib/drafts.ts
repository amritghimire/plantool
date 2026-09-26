// Unsent comment text, kept outside the React tree so a document refresh (which remounts the
// composer) or a page reload does not lose what the human was typing.
const mem = new Map<string, string>();
const PREFIX = "plantool.draft:";

export function getDraft(key: string): string {
  const m = mem.get(key);
  if (m !== undefined) return m;
  try {
    return sessionStorage.getItem(PREFIX + key) ?? "";
  } catch {
    return "";
  }
}

export function setDraft(key: string, value: string) {
  mem.set(key, value);
  try {
    if (value) sessionStorage.setItem(PREFIX + key, value);
    else sessionStorage.removeItem(PREFIX + key);
  } catch {
    // storage unavailable; memory still holds it
  }
}

export function clearDraft(key: string) {
  setDraft(key, "");
  mem.delete(key);
}

export function moveDraft(from: string, to: string) {
  if (from === to) return;
  const v = getDraft(from);
  if (!v) return;
  setDraft(to, v);
  clearDraft(from);
}
