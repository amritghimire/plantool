import { act, renderHook } from "@testing-library/react";
import { SEQUENCE_TIMEOUT, SHORTCUT_GROUPS, resolveKey, useShortcuts } from "./shortcuts";

const press = (key: string, init: KeyboardEventInit = {}, target: EventTarget = window) =>
  act(() => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }));
  });

it("resolves single keys, sequences and prefixes", () => {
  const n = vi.fn();
  const gi = vi.fn();
  const map = { n, "g i": gi };
  expect(resolveKey(map, null, "n").action).toBe(n);
  expect(resolveKey(map, null, "g")).toEqual({ prefix: "g" });
  expect(resolveKey(map, "g", "i").action).toBe(gi);
  expect(resolveKey(map, "g", "n").action).toBe(n);
  expect(resolveKey(map, null, "i")).toEqual({ prefix: null });
});

describe("useShortcuts", () => {
  afterEach(() => vi.useRealTimers());

  it("runs single keys and two-key sequences", () => {
    const keys = { c: vi.fn(), "1": vi.fn(), "?": vi.fn(), "g i": vi.fn() };
    renderHook(() => useShortcuts(keys));
    press("c");
    press("1");
    press("?", { shiftKey: true });
    press("g");
    press("i");
    expect(keys.c).toHaveBeenCalledOnce();
    expect(keys["1"]).toHaveBeenCalledOnce();
    expect(keys["?"]).toHaveBeenCalledOnce();
    expect(keys["g i"]).toHaveBeenCalledOnce();
  });

  it("drops a pending prefix after the timeout", () => {
    vi.useFakeTimers();
    const keys = { "g i": vi.fn() };
    renderHook(() => useShortcuts(keys));
    press("g");
    act(() => vi.advanceTimersByTime(SEQUENCE_TIMEOUT + 1));
    press("i");
    expect(keys["g i"]).not.toHaveBeenCalled();
  });

  it("ignores modifiers, typing and open dialogs", () => {
    const keys = { a: vi.fn() };
    renderHook(() => useShortcuts(keys));
    press("a", { metaKey: true });
    press("a", { ctrlKey: true });
    press("a", { altKey: true });
    const input = document.createElement("textarea");
    document.body.append(input);
    press("a", {}, input);
    input.remove();
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.append(dialog);
    press("a");
    dialog.remove();
    expect(keys.a).not.toHaveBeenCalled();
    press("a");
    expect(keys.a).toHaveBeenCalledOnce();
  });
});

it("documents every shortcut in the help sheet", () => {
  const listed = SHORTCUT_GROUPS.flatMap((g) => g.keys.map(([k]) => k.join(" ")));
  for (const key of ["j", "k", "Enter", "/", "g i", "1", "2", "3", "c", "n", "p", "a", "Esc", "?"])
    expect(listed).toContain(key);
});
