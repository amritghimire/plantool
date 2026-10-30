export function Handoff({
  step,
  label,
  action,
  onAction,
}: {
  step: string;
  label: string;
  action: string;
  onAction: () => void;
}) {
  const displayLabel: Record<string, string> = {
    "Owner approving": "Ready for your review",
    "Owner reviewing": "Ready for your feedback",
    "Owner choosing next step": "Ready when you are",
    "Agent working": "Agent is working",
    "Agent needs an answer": "Your input is needed",
  };
  return (
    <div className="handoff">
      <p role="status" aria-live="polite">
        <strong>
          {step} · {displayLabel[label] ?? label}
        </strong>
      </p>
      <button className="btn primary small" type="button" onClick={onAction}>
        {action}
      </button>
    </div>
  );
}
