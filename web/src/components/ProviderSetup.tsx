import { providerLabel } from "../lib/agents";

interface ProviderInfo {
  id: string;
  available: boolean;
  version?: string;
  error?: string;
}

export function ProviderSetup({ providers }: { providers: ProviderInfo[] }) {
  return <details className="provider-setup"><summary>Agent setup</summary><ul>
    {providers.map((item) => <li key={item.id}><strong>{providerLabel(item.id)}</strong><span className={item.available ? "muted" : "error"}>{item.available ? item.version || "Ready" : item.error || "Unavailable"}</span></li>)}
  </ul></details>;
}
