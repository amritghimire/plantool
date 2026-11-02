import { useCallback, useEffect, useRef, useState } from "react";
import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { Modal } from "../components/Modal";

export interface ConfirmOptions {
  title?: string;
  confirmLabel?: string;
  fields?: { label: string; value: string }[];
  reasonLabel?: string;
}

interface Request extends ConfirmOptions {
  description: string;
  destructive: boolean;
}

export function useConfirmation() {
  const [request, setRequest] = useState<Request | null>(null);
  const [reason, setReason] = useState("");
  const pending = useRef<((result: string | null) => void) | null>(null);
  useEffect(
    () => () => {
      pending.current?.(null);
    },
    [],
  );
  const ask = useCallback(
    (req: Request) =>
      new Promise<string | null>((resolve) => {
        pending.current?.(null);
        pending.current = resolve;
        setReason("");
        setRequest(req);
      }),
    [],
  );
  const confirm = useCallback(
    async (description: string, destructive = false, options?: ConfirmOptions) =>
      (await ask({ description, destructive, ...options })) !== null,
    [ask],
  );
  const confirmWithReason = useCallback(
    (description: string, options: ConfirmOptions & { reasonLabel: string }) =>
      ask({ description, destructive: false, ...options }),
    [ask],
  );
  const finish = (result: string | null) => {
    pending.current?.(result);
    pending.current = null;
    setRequest(null);
  };
  const title =
    request?.title ??
    (request?.destructive ? "Confirm deletion" : "Confirm this change");
  const needsReason = !!request?.reasonLabel;
  const dialog = request && (
    <Modal destructive title={title} onClose={() => finish(null)}>
      <h3>{title}</h3>
      <p className="dialog-intro">{request.description}</p>
      {request.fields && (
        <dl className="confirm-fields">
          {request.fields.map((f) => (
            <div key={f.label}>
              <dt>{f.label}</dt>
              <dd>{f.value}</dd>
            </div>
          ))}
        </dl>
      )}
      {needsReason && (
        <label>
          {request.reasonLabel}
          <textarea
            rows={3}
            value={reason}
            autoFocus
            onChange={(e) => setReason(e.target.value)}
          />
        </label>
      )}
      <div className="dialog-actions button-row">
        {needsReason && !reason.trim() && (
          <span className="dialog-why" id="confirm-why">
            Write a reason first
          </span>
        )}
        <span className="spacer" />
        <AlertDialog.Cancel asChild>
          <button className="btn" type="button" onClick={() => finish(null)}>
            Cancel
          </button>
        </AlertDialog.Cancel>
        <button
          className={request.destructive ? "btn danger" : "btn primary"}
          type="button"
          disabled={needsReason && !reason.trim()}
          aria-describedby={needsReason && !reason.trim() ? "confirm-why" : undefined}
          onClick={() => finish(needsReason ? reason.trim() : "")}
        >
          {request.confirmLabel ??
            (request.destructive ? "Delete" : "Continue")}
        </button>
      </div>
    </Modal>
  );
  return { confirm, confirmWithReason, dialog };
}
