import { useEffect } from "react";
import { useLocation } from "react-router";
import { RightSlotSurface, useRightSlot } from "../../app/rightSlot";
import { LinkButton } from "../../components";
import { OperatorPanel } from "./OperatorPanel";

/** The id the Operator panel registers with the right-hand slot. */
export const OPERATOR_ID = "operator";

/** Whether `pathname` is the Operator workspace, where the panel would show it twice. */
export function onWorkspace(pathname: string): boolean {
  return pathname === "/operator" || pathname.startsWith("/operator/");
}

/**
 * Registers the Operator panel with the shell's right-hand slot. Mounted once, in the shell:
 * the slot decides where the panel goes, how wide it is and what Escape does. The panel closes
 * itself when the workspace opens, so one conversation never streams twice in a tab.
 */
export function OperatorSurface() {
  const slot = useRightSlot();
  const { pathname } = useLocation();
  const here = onWorkspace(pathname);
  const open = slot.isOpen(OPERATOR_ID);
  const { close } = slot;
  useEffect(() => {
    if (here && open) {
      close(OPERATOR_ID);
    }
  }, [here, open, close]);

  return (
    <RightSlotSurface
      spec={{ id: OPERATOR_ID, label: "Operator", icon: "chat" }}
      actions={
        <LinkButton to="/operator" size="sm" variant="ghost">
          Open in workspace
        </LinkButton>
      }
    >
      <OperatorPanel />
    </RightSlotSurface>
  );
}
