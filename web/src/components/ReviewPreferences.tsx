import * as Popover from "@radix-ui/react-popover";
import { SlidersHorizontal } from "@phosphor-icons/react";

export function ReviewPreferences({
  author,
  onAuthor,
  showResolved,
  onResolved,
}: {
  author: string;
  onAuthor: (name: string) => void;
  showResolved: boolean;
  onResolved: (value: boolean) => void;
}) {
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button type="button" className="review-options-trigger">
          <SlidersHorizontal size={15} aria-hidden="true" />
          Review options
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
          <label>
            Reviewer
            <input
              value={author}
              onChange={(event) => onAuthor(event.target.value)}
            />
          </label>
          <label className="check-row">
            <input
              type="checkbox"
              checked={showResolved}
              onChange={(event) => onResolved(event.target.checked)}
            />
            Show resolved comments
          </label>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
