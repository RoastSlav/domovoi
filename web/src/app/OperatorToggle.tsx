import { useLocation } from "react-router";
import { IconButton } from "../components";
import { OPERATOR_ID, onWorkspace } from "../features/operator/OperatorSurface";
import { useRightSlot } from "./rightSlot";

/** The top-bar button that opens and closes the Operator panel. */
export function OperatorToggle() {
  const slot = useRightSlot();
  const { pathname } = useLocation();
  const open = slot.isOpen(OPERATOR_ID);
  return (
    <IconButton
      icon="chat"
      label={open ? "Close the Operator" : "Open the Operator"}
      aria-pressed={open}
      disabledReason={onWorkspace(pathname) ? "The Operator is open on this page." : undefined}
      onClick={(event) => slot.toggle(OPERATOR_ID, { opener: event.currentTarget })}
    />
  );
}
