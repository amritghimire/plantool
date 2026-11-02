import { useEffect, useState } from "react";
import { SunIcon, MoonIcon, MonitorIcon } from "@phosphor-icons/react";

type Theme = "system" | "light" | "dark";

function read(): Theme {
  try {
    const t = localStorage.getItem("plantool.theme");
    return t === "light" || t === "dark" ? t : "system";
  } catch {
    return "system";
  }
}

export function applyTheme(t: Theme) {
  const root = document.documentElement;
  if (t === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", t);
}

export function ThemeToggle({ className = "spine-icon theme-toggle", size = 16 }: { className?: string; size?: number }) {
  const [theme, setTheme] = useState<Theme>(read);
  useEffect(() => {
    applyTheme(theme);
    try {
      localStorage.setItem("plantool.theme", theme);
    } catch {
      // ignore
    }
  }, [theme]);
  const next: Record<Theme, Theme> = { system: "light", light: "dark", dark: "system" };
  return (
    <button className={className} type="button" aria-label={`Theme: ${theme}. Switch to ${next[theme]}`} title={`Theme: ${theme}`} onClick={() => setTheme(next[theme])}>
      {theme === "system" ? <MonitorIcon size={size} aria-hidden="true" /> : theme === "light" ? <SunIcon size={size} aria-hidden="true" /> : <MoonIcon size={size} aria-hidden="true" />}
    </button>
  );
}
