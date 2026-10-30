import { useRef, type ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { cn } from "../lib/cn";

export function Modal({
  title,
  children,
  onClose,
  busy = false,
  className,
  destructive = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  busy?: boolean;
  className?: string;
  destructive?: boolean;
}) {
  const invokingControl = useRef(
    typeof document !== "undefined" &&
      document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null,
  );
  const restoreFocus = (event: Event) => {
    event.preventDefault();
    if (invokingControl.current?.isConnected) invokingControl.current.focus();
  };
  const close = (open: boolean) => {
    if (!open && !busy) onClose();
  };
  if (destructive)
    return (
      <AlertDialog.Root open onOpenChange={close}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="modal-overlay" />
          <AlertDialog.Content
            className={cn("modal form-stack", className)}
            onCloseAutoFocus={restoreFocus}
            onEscapeKeyDown={(event) => {
              if (busy) event.preventDefault();
            }}
          >
            <AlertDialog.Title className="sr-only">{title}</AlertDialog.Title>
            <AlertDialog.Description className="sr-only">
              Review the consequences below before confirming this action.
            </AlertDialog.Description>
            {children}
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>
    );
  return (
    <Dialog.Root open onOpenChange={close}>
      <Dialog.Portal>
        <Dialog.Overlay className="modal-overlay" />
        <Dialog.Content
          className={cn("modal form-stack", className)}
          aria-describedby={undefined}
          onCloseAutoFocus={restoreFocus}
          onEscapeKeyDown={(event) => {
            if (busy) event.preventDefault();
          }}
          onPointerDownOutside={(event) => {
            if (busy) event.preventDefault();
          }}
        >
          <Dialog.Title className="sr-only">{title}</Dialog.Title>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
