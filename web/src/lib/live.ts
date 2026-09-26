import { useEffect, useRef } from "react";
import { liveUrl } from "../api";
import type { LiveEvent } from "../types";

export function useLive(key: string | null, onEvent: (e: LiveEvent) => void, onReconnect: () => void) {
  const handler = useRef(onEvent);
  const reconnect = useRef(onReconnect);
  handler.current = onEvent;
  reconnect.current = onReconnect;
  useEffect(() => {
    if (!key) return;
    let ws: WebSocket | null = null;
    let closed = false;
    let attempts = 0;
    let timer: number | undefined;
    const connect = () => {
      ws = new WebSocket(liveUrl(key));
      ws.onopen = () => {
        if (attempts > 0) reconnect.current();
        attempts = 0;
      };
      ws.onmessage = (m) => {
        try {
          const ev = JSON.parse(m.data as string) as LiveEvent;
          if (ev.type === "resync") reconnect.current();
          else handler.current(ev);
        } catch {
          // ignore malformed frames
        }
      };
      ws.onclose = () => {
        if (closed) return;
        attempts += 1;
        const delay = Math.min(10_000, 300 * 2 ** Math.min(attempts, 5));
        timer = window.setTimeout(connect, delay);
      };
      ws.onerror = () => ws?.close();
    };
    connect();
    return () => {
      closed = true;
      if (timer) window.clearTimeout(timer);
      ws?.close();
    };
  }, [key]);
}
