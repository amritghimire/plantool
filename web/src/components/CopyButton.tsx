import { useState } from "react";

export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // fall through to the textarea trick
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

export function CopyButton({ text, label = "Copy", className = "btn ghost small" }: { text: string; label?: string; className?: string }) {
  const [state, setState] = useState<"idle" | "done" | "failed">("idle");
  return (
    <button
      className={className}
      type="button"
      onClick={() => {
        void copyText(text).then((ok) => {
          setState(ok ? "done" : "failed");
          window.setTimeout(() => setState("idle"), 1600);
        });
      }}
    >
      {state === "done" ? "Copied" : state === "failed" ? "Select and copy" : label}
    </button>
  );
}
