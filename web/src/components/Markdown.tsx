import { useEffect, useId, useRef, useState } from "react";

function mermaidTheme(dark: boolean) {
  const css = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  const sheet = token("--sheet", dark ? "#0e1220" : "#ffffff");
  const sheet2 = token("--sheet-2", dark ? "#131829" : "#f6f6f5");
  const ink = token("--ink", dark ? "#eceef3" : "#111111");
  const rule = token("--rule-strong", dark ? "#666e86" : "#8a8a8a");
  return {
    darkMode: dark,
    fontFamily: '"Public Sans", -apple-system, BlinkMacSystemFont, sans-serif',
    fontSize: "14px",
    background: sheet,
    mainBkg: sheet,
    primaryColor: sheet,
    primaryTextColor: ink,
    primaryBorderColor: ink,
    secondaryColor: sheet2,
    secondaryTextColor: ink,
    secondaryBorderColor: ink,
    tertiaryColor: sheet2,
    tertiaryTextColor: ink,
    tertiaryBorderColor: rule,
    nodeBorder: ink,
    nodeTextColor: ink,
    lineColor: ink,
    textColor: ink,
    titleColor: ink,
    edgeLabelBackground: sheet,
    clusterBkg: sheet2,
    clusterBorder: rule,
    noteBkgColor: sheet2,
    noteTextColor: ink,
    noteBorderColor: rule,
    actorBkg: sheet,
    actorBorder: ink,
    actorTextColor: ink,
    actorLineColor: rule,
    signalColor: ink,
    signalTextColor: ink,
    labelBoxBkgColor: sheet,
    labelBoxBorderColor: ink,
    labelTextColor: ink,
    loopTextColor: ink,
    sequenceNumberColor: sheet,
  };
}

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
        mermaid.initialize({ startOnLoad: false, theme: "base", themeVariables: mermaidTheme(dark), securityLevel: "strict" });
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
