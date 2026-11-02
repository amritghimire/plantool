import { useEffect, useRef } from "react";

export type KeyMap = Record<string, () => void>;

export const SHORTCUT_GROUPS: { title: string; keys: [string[], string][] }[] = [
  {
    title: "Inbox",
    keys: [
      [["j"], "Next session"],
      [["k"], "Previous session"],
      [["Enter"], "Open the focused session"],
      [["/"], "Search sessions"],
    ],
  },
  {
    title: "Session",
    keys: [
      [["g", "i"], "Go to the inbox"],
      [["1"], "Research"],
      [["2"], "Plan"],
      [["3"], "Changes"],
      [["c"], "Open or close comments"],
      [["n"], "Next open comment"],
      [["p"], "Previous open comment"],
      [["a"], "Open the approval form (you still confirm)"],
      [["Esc"], "Close the drawer"],
    ],
  },
  {
    title: "Anywhere",
    keys: [[["?"], "Show these shortcuts"]],
  },
];

export const SEQUENCE_TIMEOUT = 1200;

export function isTypingTarget(t: EventTarget | null): boolean {
  return t instanceof HTMLElement && (t.matches("input, textarea, select") || t.isContentEditable);
}

export function dialogOpen(): boolean {
  return document.querySelector('[role="dialog"], [role="alertdialog"]') !== null;
}

export function resolveKey(
  map: KeyMap,
  prefix: string | null,
  key: string,
): { action?: () => void; prefix: string | null } {
  if (prefix && map[`${prefix} ${key}`]) return { action: map[`${prefix} ${key}`], prefix: null };
  if (map[key]) return { action: map[key], prefix: null };
  if (Object.keys(map).some((k) => k.startsWith(`${key} `))) return { prefix: key };
  return { prefix: null };
}

export function useShortcuts(keys: KeyMap) {
  const ref = useRef(keys);
  ref.current = keys;
  useEffect(() => {
    let prefix: string | null = null;
    let timer = 0;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTypingTarget(e.target) || dialogOpen()) return;
      const next = resolveKey(ref.current, prefix, e.key);
      window.clearTimeout(timer);
      prefix = next.prefix;
      if (prefix) {
        e.preventDefault();
        timer = window.setTimeout(() => (prefix = null), SEQUENCE_TIMEOUT);
        return;
      }
      if (!next.action) return;
      e.preventDefault();
      next.action();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("keydown", onKey);
    };
  }, []);
}

export function stepFocus(items: HTMLElement[], dir: 1 | -1) {
  if (!items.length) return;
  const at = items.findIndex((el) => el.contains(document.activeElement));
  const next = at < 0 ? (dir === 1 ? 0 : items.length - 1) : Math.min(items.length - 1, Math.max(0, at + dir));
  items[next].focus();
  items[next].scrollIntoView?.({ block: "nearest" });
}
