import { Button, Kbd, Tooltip } from "../../components";

/** The Save button with its shortcut in a tooltip, and the reason when it cannot be used. */
export function SaveButton({
  busy,
  reason,
  onSave,
}: {
  busy: boolean;
  onSave: () => void;
  reason: string | undefined;
}) {
  const shortcut = reason ? undefined : (
    <>
      Save with <Kbd>Ctrl</Kbd> + <Kbd>S</Kbd>
    </>
  );
  return (
    <Tooltip content={shortcut}>
      <Button
        variant="primary"
        icon="check"
        loading={busy}
        disabledReason={reason}
        onClick={onSave}
      >
        Save
      </Button>
    </Tooltip>
  );
}
