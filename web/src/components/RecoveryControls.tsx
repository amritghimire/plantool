import { useCallback, useState } from "react";
import { api } from "../api";
import { usePolling } from "../lib/usePolling";
import { stepLabel } from "../engineering";
export function RecoveryControls({
  item,
  onRecovered,
}: {
  item: string;
  onRecovered: () => void;
}) {
  const load = useCallback(() => api.recovery(item), [item]);
  const { data, error, refresh } = usePolling(load);
  const [busy, setBusy] = useState(false),
    [failure, setFailure] = useState("");
  return (
    <section className="next-action">
      <strong>Owner recovery</strong>
      <p>{data?.message ?? "Checking recovery eligibility…"}</p>
      {!data?.eligible && data?.attempt_id && <a href={`#attempt-transcript-${data.attempt_id}`}>Open the latest attempt transcript ↓</a>}
      {data?.routing_error && <p role="alert">No eligible model route now: {data.routing_error}. Update and verify routes in Settings → Models & providers; this panel refreshes current eligibility automatically.</p>}
      {data?.eligible && (
        <>
          <p>
            {data.review ? "Review" : "Retry"} {stepLabel(data.step)} using current evidence. This does not approve changes. The project scheduler has its own recovery action if it is paused.
          </p>
          <button
            className="btn"
            disabled={busy || !!data.routing_error}
            onClick={async () => {
              setBusy(true);
              setFailure("");
              try {
                await api.retryEngineering(item, data.attempt_id);
                refresh();
                onRecovered();
              } catch (e) {
                setFailure((e as Error).message);
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? "Queueing recovery…" : data.review ? "Review paused step with passkey" : "Retry failed step with passkey"}
          </button>
        </>
      )}
      {(error || failure) && <p role="alert">{error || failure}</p>}
    </section>
  );
}
