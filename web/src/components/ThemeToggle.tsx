import { useEffect, useState } from "react";
import { Sun, Moon, Monitor } from "@phosphor-icons/react";

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

export function ThemeToggle() {
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
    <button className="rail-button theme-toggle" type="button" aria-label={`Theme: ${theme}. Switch to ${next[theme]}`} title={`Theme: ${theme}`} onClick={() => setTheme(next[theme])}>
      {theme === "system" ? <Monitor size={21} aria-hidden="true" /> : theme === "light" ? <Sun size={21} aria-hidden="true" /> : <Moon size={21} aria-hidden="true" />}
    </button>
  );
}
