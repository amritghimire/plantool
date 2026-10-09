import { useEffect, useId, useRef, useState } from "react";

export function Mermaid({ code }: { code: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const id = useId().replace(/[^a-zA-Z0-9]/g, "");
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const mermaid = (await import("mermaid")).default;
        const dark = document.documentElement.getAttribute("data-theme") === "dark" || (!document.documentElement.getAttribute("data-theme") && matchMedia("(prefers-color-scheme: dark)").matches);
        mermaid.initialize({ startOnLoad: false, theme: dark ? "dark" : "default", securityLevel: "strict" });
        const { svg } = await mermaid.render(`m${id}`, code);
        if (!cancelled && ref.current) ref.current.innerHTML = svg;
      } catch (e) {
        if (!cancelled) setErr((e as Error).message);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [code, id]);
  const labels = Array.from(code.matchAll(/\[([^\]]+)\]/g), (m) => m[1].replace(/<[^>]+>/g, " "));
  const summary = labels.length ? `Diagram: ${labels.join("; ")}` : "Diagram source is available below.";
  return <figure className="mermaid-figure">
    <figcaption>{summary}</figcaption>
    {err ? <p role="status">The diagram could not render. Read its source below.</p> : <div className="mermaid" ref={ref} role="img" aria-label={summary} />}
    <details open={!!err}><summary>Diagram source</summary><pre>{code}</pre></details>
  </figure>;
}
