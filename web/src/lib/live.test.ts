import { act, renderHook } from "@testing-library/react";
import { useLive } from "./live";

it("keeps the view disconnected until reconnect reload finishes", async () => {
  vi.useFakeTimers();
  const sockets: { onopen?: () => void; onclose?: () => void; close: () => void }[] = [];
  class Socket {
    onopen?: () => void;
    onclose?: () => void;
    constructor() { sockets.push(this); }
    close() {}
  }
  vi.stubGlobal("WebSocket", Socket);
  let finish!: () => void;
  const reload = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
  const { result, unmount } = renderHook(() => useLive("repo/session", () => {}, reload));
  act(() => sockets[0].onopen?.());
  expect(result.current.connected).toBe(true);
  act(() => sockets[0].onclose?.());
  expect(result.current.connected).toBe(false);
  act(() => vi.advanceTimersByTime(600));
  act(() => sockets[1].onopen?.());
  expect(reload).toHaveBeenCalledOnce();
  expect(result.current.connected).toBe(false);
  await act(async () => finish());
  expect(result.current.connected).toBe(true);
  unmount(); vi.unstubAllGlobals(); vi.useRealTimers();
});
