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
      {data?.routing_error && <p role="alert">No eligible model route now: {data.routing_error}. Update and verify routes in Settings → Models & providers; this panel refreshes current eligibility automatically.</p>}
      {data?.eligible && (
        <>
          <p>
            Retry {stepLabel(data.step)} in a new checkpoint history. This does
            not approve a plan, milestone or PR. Recover the project scheduler
            separately if it is frozen.
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
            {busy ? "Queueing recovery…" : "Retry failed step with passkey"}
          </button>
        </>
      )}
      {(error || failure) && <p role="alert">{error || failure}</p>}
    </section>
  );
}
