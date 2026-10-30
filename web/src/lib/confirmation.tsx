import { useCallback, useEffect, useRef, useState } from "react";
import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { Modal } from "../components/Modal";

export function useConfirmation() {
  const [request, setRequest] = useState<{
    description: string;
    destructive: boolean;
  } | null>(null);
  const pending = useRef<((confirmed: boolean) => void) | null>(null);
  useEffect(
    () => () => {
      pending.current?.(false);
    },
    [],
  );
  const confirm = useCallback(
    (description: string, destructive = false) =>
      new Promise<boolean>((resolve) => {
        pending.current?.(false);
        pending.current = resolve;
        setRequest({ description, destructive });
      }),
    [],
  );
  const finish = (confirmed: boolean) => {
    pending.current?.(confirmed);
    pending.current = null;
    setRequest(null);
  };
  const dialog = request && (
    <Modal
      destructive
      title={request.destructive ? "Confirm deletion" : "Confirm this change"}
      onClose={() => finish(false)}
    >
      <h3>
        {request.destructive ? "Confirm deletion" : "Confirm this change"}
      </h3>
      <p className="dialog-intro">{request.description}</p>
      <div className="dialog-actions button-row">
        <span className="spacer" />
        <AlertDialog.Cancel asChild>
          <button className="btn" type="button" onClick={() => finish(false)}>
            Cancel
          </button>
        </AlertDialog.Cancel>
        <button
          className={request.destructive ? "btn danger" : "btn primary"}
          type="button"
          onClick={() => finish(true)}
        >
          {request.destructive ? "Delete" : "Continue"}
        </button>
      </div>
    </Modal>
  );
  return { confirm, dialog };
}
