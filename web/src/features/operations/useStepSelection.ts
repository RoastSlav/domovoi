import { useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "react-router";
import type { OperationTrace } from "../../api/operationTrace";
import { useRightSlot } from "../../app/rightSlot";

/** The slot surface the step inspector registers as. */
export const INSPECTOR_ID = "inspector";

/**
 * The selected step lives in the address (`?step=<id>`). The inspector opens while a valid
 * step is selected and closes when the parameter goes; a parameter naming no step of the
 * trace is dropped. Focus moves into the inspector only when a row click opened it, not on
 * a deep link.
 */
export function useStepSelection(trace: OperationTrace | undefined) {
  const [params, setParams] = useSearchParams();
  const slot = useRightSlot();
  const slotRef = useRef(slot);
  useEffect(() => {
    slotRef.current = slot;
  });
  const raw = params.get("step");
  const valid = raw && trace?.steps.some((step) => step.id === raw) ? raw : undefined;
  const clicked = useRef<HTMLElement | null>(null);
  const registered = INSPECTOR_ID in slot.specs;

  const clear = useCallback(() => {
    setParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.delete("step");
        return next;
      },
      { replace: true },
    );
  }, [setParams]);

  const select = useCallback(
    (id: string, opener: HTMLElement) => {
      clicked.current = opener;
      setParams(
        (current) => {
          const next = new URLSearchParams(current);
          next.set("step", id);
          return next;
        },
        { replace: true },
      );
    },
    [setParams],
  );

  useEffect(() => {
    if (trace && raw && !valid) {
      clear();
    }
  }, [trace, raw, valid, clear]);

  useEffect(() => {
    if (!registered) {
      return;
    }
    if (valid) {
      const opener = clicked.current;
      clicked.current = null;
      slotRef.current.open(INSPECTOR_ID, { focus: opener !== null, opener });
    } else {
      slotRef.current.close(INSPECTOR_ID);
    }
  }, [valid, registered]);

  return { selected: valid, select, clear };
}
