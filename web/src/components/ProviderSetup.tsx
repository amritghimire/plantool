import { CopyButton } from "./CopyButton";
import { providerLabel } from "../lib/agents";

interface ProviderInfo {
  id: string;
  available: boolean;
  version?: string;
  error?: string;
}

export function ProviderSetup({ providers, onRecheck }: { providers: ProviderInfo[]; onRecheck?: () => void }) {
  const commands: Record<string, string> = { claude: "claude auth login", codex: "codex login", copilot: "copilot login", ollama: "ollama serve", opencode: "opencode" };
  return <details className="provider-setup"><summary>Agent setup</summary>{onRecheck && <button className="btn ghost" type="button" onClick={onRecheck}>Re-check</button>}<ul>
    {providers.map((item) => <li key={item.id}><strong>{providerLabel(item.id)}</strong><span className={item.available ? "muted" : "error"}>{item.available ? item.error ? "Sign-in not verified" : "Ready" : item.version ? "Not signed in or not ready" : "Not installed or unavailable"}</span>{item.error && <p>{item.error}</p>}<CopyButton text={commands[item.id] ?? item.id} label="Copy setup command" /></li>)}
  </ul></details>;
}
