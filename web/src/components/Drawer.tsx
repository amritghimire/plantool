import { useEffect, useRef, type ReactNode } from "react";
import { XIcon } from "@phosphor-icons/react";

export type DrawerId = "comments" | "agent" | "details";

export const DRAWER_TITLE: Record<DrawerId, string> = {
  comments: "Comments",
  agent: "Agent run",
  details: "Session details",
};

export function drawerFrom(value: string | null): DrawerId | null {
  return value === "comments" || value === "agent" || value === "details"
    ? value
    : null;
}

export function DrawerClose({ id, onClose }: { id: DrawerId; onClose: () => void }) {
  return (
    <button
      className="drawer-close"
      type="button"
      aria-label={`Close ${DRAWER_TITLE[id].toLowerCase()}`}
      onClick={onClose}
    >
      <XIcon size={16} aria-hidden="true" />
    </button>
  );
}

export function Drawer({
  id,
  onClose,
  bare = false,
  children,
}: {
  id: DrawerId;
  onClose: () => void;
  bare?: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    const opener =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    ref.current?.focus();
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  }, [id]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const t = e.target;
      if (
        t instanceof HTMLElement &&
        (t.matches("input, textarea, select") || t.isContentEditable)
      )
        return;
      if (document.querySelector('[role="dialog"], [role="alertdialog"]')) return;
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const title = DRAWER_TITLE[id];
  return (
    <aside
      ref={ref}
      tabIndex={-1}
      id={`drawer-${id}`}
      className={`drawer drawer-${id}`}
      aria-labelledby={`drawer-${id}-title`}
    >
      {!bare && (
        <header className="drawer-head">
          <h2 id={`drawer-${id}-title`}>{title}</h2>
          <DrawerClose id={id} onClose={onClose} />
        </header>
      )}
      <div className="drawer-body">{children}</div>
    </aside>
  );
}
