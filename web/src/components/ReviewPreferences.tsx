import * as Popover from "@radix-ui/react-popover";
import { CaretDownIcon, SlidersHorizontalIcon } from "@phosphor-icons/react";
import type { ViewMode } from "./DocView";

export const VIEW_MODES: [ViewMode, string][] = [
  ["rendered", "Document"],
  ["slides", "Slides"],
  ["source", "Source"],
];

export function ReviewPreferences({
  author,
  onAuthor,
  mode,
  onMode,
}: {
  author: string;
  onAuthor: (name: string) => void;
  mode?: ViewMode;
  onMode?: (mode: ViewMode) => void;
}) {
  const modeLabel = VIEW_MODES.find(([m]) => m === mode)?.[1];
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button
          type="button"
          className="review-options-trigger"
          title={modeLabel ? `${modeLabel} · view and review options` : "Review options"}
        >
          <SlidersHorizontalIcon className="review-options-icon" size={16} aria-hidden="true" />
          {modeLabel ? (
            <>
              <span className="view-mode-label">{modeLabel}</span>
              <CaretDownIcon className="view-mode-caret" size={12} aria-hidden="true" />
              <span className="sr-only"> · view and review options</span>
            </>
          ) : (
            <span className="sr-only">Review options</span>
          )}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className="review-popover"
          align="end"
          sideOffset={8}
          collisionPadding={12}
          aria-label="Review options"
        >
          {mode && onMode && (
            <div className="view-modes" role="group" aria-label="Document display">
              {VIEW_MODES.map(([m, label]) => (
                <Popover.Close asChild key={m}>
                  <button
                    type="button"
                    className="menu-item"
                    aria-pressed={mode === m}
                    onClick={() => onMode(m)}
                  >
                    {label}
                  </button>
                </Popover.Close>
              ))}
            </div>
          )}
          <label>
            Reviewer
            <input
              value={author}
              onChange={(event) => onAuthor(event.target.value)}
            />
          </label>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
