export function Handoff({ step, label, action, onAction }: { step: string; label: string; action: string; onAction: () => void }) {
  return <div className="handoff"><p role="status" aria-live="polite"><strong>{step} · {label}</strong></p><button className="btn primary small" type="button" onClick={onAction}>{action}</button></div>;
}
