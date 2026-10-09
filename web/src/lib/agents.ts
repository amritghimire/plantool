export function providerLabel(id: string): string {
  return ({ claude: "Claude Code", codex: "Codex", copilot: "GitHub Copilot", ollama: "Ollama via OpenCode", opencode: "OpenCode" } as Record<string, string>)[id] ?? id;
}

export function savedAgentSetting(provider: string, setting: "model" | "effort"): string {
  try { return localStorage.getItem(`plantool.${setting}.${provider}`) || ""; } catch { return ""; }
}

export function saveAgentSettings(provider: string, model: string, effort: string): void {
  try {
    localStorage.setItem(`plantool.model.${provider}`, model);
    localStorage.setItem(`plantool.effort.${provider}`, effort);
  } catch { /* storage is optional */ }
}
