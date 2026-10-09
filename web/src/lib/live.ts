import { useEffect, useRef, useState } from "react";
import { liveUrl } from "../api";
import type { LiveEvent } from "../types";

export function useLive(key: string | null, onEvent: (e: LiveEvent) => void, onReconnect: () => void | Promise<void>) {
  const [connected, setConnected] = useState(false);
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
        if (attempts > 0) {
          setConnected(false);
          void Promise.resolve(reconnect.current()).then(() => { if (!closed) setConnected(true); }).catch(() => { if (!closed) setConnected(false); });
        } else setConnected(true);
        attempts = 0;
      };
      ws.onmessage = (m) => {
        try {
          const ev = JSON.parse(m.data as string) as LiveEvent;
          if (ev.type === "resync") { setConnected(false); void Promise.resolve(reconnect.current()).then(() => { if (!closed) setConnected(true); }).catch(() => {}); }
          else handler.current(ev);
        } catch {
          // ignore malformed frames
        }
      };
      ws.onclose = () => {
        if (closed) return;
        setConnected(false);
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
  return { connected };
}
