import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useMediaQuery, WIDE_QUERY } from "../useMediaQuery";
import { slotLayout, type SlotLayout } from "./layout";
import { INITIAL_SLOT, slotReducer, type SlotAction } from "./state";
import type { OpenOptions, RightSlotApi, SurfaceId } from "./types";

/** The elements of a visible panel that its surface fills through portals. */
export interface PanelTargets {
  body?: HTMLElement | null;
  actions?: HTMLElement | null;
}

/** What a surface hands the slot besides its content. */
export interface SurfaceHandlers {
  onClose?: (() => void) | undefined;
}

/** Plumbing shared by the provider, the host and the surfaces; not for pages. */
export interface SlotInternals {
  api: RightSlotApi;
  layout: SlotLayout;
  dispatch: (action: SlotAction) => void;
  targets: Readonly<Record<SurfaceId, PanelTargets>>;
  setTarget: (id: SurfaceId, kind: keyof PanelTargets, element: HTMLElement | null) => void;
  /** Records (or, with `undefined`, forgets) what a surface does when the slot closes it. */
  setHandlers: (id: SurfaceId, handlers: SurfaceHandlers | undefined) => void;
  /** True once, for a surface that was just opened and asked for focus. */
  takeFocus: (id: SurfaceId) => boolean;
}

const Context = createContext<SlotInternals | null>(null);

/** The slot's internals; throws outside the provider. */
export function useSlotInternals(): SlotInternals {
  const value = useContext(Context);
  if (!value) {
    throw new Error("useRightSlot needs a RightSlotProvider above it");
  }
  return value;
}

/** What a page or a toolbar button may do with the right-hand slot. */
export function useRightSlot(): RightSlotApi {
  return useSlotInternals().api;
}

/** True while the slot is a modal sheet, so the page behind it is inert. */
export function useSlotModal(): boolean {
  return useSlotInternals().layout.mode === "sheet";
}

/** Owns the slot's state, once, in the shell. */
export function RightSlotProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(slotReducer, INITIAL_SLOT);
  const [targets, setTargets] = useState<Record<SurfaceId, PanelTargets>>({});
  const wide = useMediaQuery(WIDE_QUERY);
  const handlers = useRef<Record<SurfaceId, SurfaceHandlers>>({});
  const latest = useRef(state);
  useEffect(() => {
    latest.current = state;
  }, [state]);
  const wantsFocus = useRef(new Set<SurfaceId>());
  const returnTo = useRef<HTMLElement | null | undefined>(undefined);

  const open = useCallback((id: SurfaceId, options: OpenOptions = {}) => {
    const current = latest.current;
    if (!(id in current.specs)) {
      return;
    }
    if (!current.open.includes(id) && options.focus !== false) {
      wantsFocus.current.add(id);
    }
    const active = document.activeElement;
    const opener = options.opener ?? (active instanceof HTMLElement ? active : null);
    dispatch({ type: "open", id, opener });
  }, []);

  const close = useCallback((id: SurfaceId) => {
    const current = latest.current;
    if (!current.open.includes(id)) {
      return;
    }
    returnTo.current = current.openers[id] ?? null;
    wantsFocus.current.delete(id);
    dispatch({ type: "close", id });
    handlers.current[id]?.onClose?.();
  }, []);

  // After the page is interactive again (the host lifts `inert` first), focus goes back.
  useEffect(() => {
    const target = returnTo.current;
    if (target === undefined) {
      return;
    }
    returnTo.current = undefined;
    const element = target?.isConnected ? target : document.getElementById("main");
    element?.focus();
  }, [state.open]);

  const setTarget = useCallback(
    (id: SurfaceId, kind: keyof PanelTargets, element: HTMLElement | null) => {
      setTargets((all) =>
        all[id]?.[kind] === element ? all : { ...all, [id]: { ...all[id], [kind]: element } },
      );
    },
    [],
  );

  const setHandlers = useCallback((id: SurfaceId, next: SurfaceHandlers | undefined) => {
    if (next) {
      handlers.current[id] = next;
    } else {
      handlers.current = Object.fromEntries(
        Object.entries(handlers.current).filter(([key]) => key !== id),
      );
    }
  }, []);

  const takeFocus = useCallback((id: SurfaceId) => wantsFocus.current.delete(id), []);

  const api = useMemo<RightSlotApi>(
    () => ({
      specs: state.specs,
      openIds: state.open,
      isOpen: (id) => state.open.includes(id),
      open,
      close,
      toggle: (id, options) => (state.open.includes(id) ? close(id) : open(id, options)),
    }),
    [state.specs, state.open, open, close],
  );

  const layout = useMemo(
    () => slotLayout({ available: state.available, wide, open: state.open }),
    [state.available, wide, state.open],
  );

  const value = useMemo<SlotInternals>(
    () => ({ api, layout, dispatch, targets, setTarget, setHandlers, takeFocus }),
    [api, layout, targets, setTarget, setHandlers, takeFocus],
  );
  return <Context.Provider value={value}>{children}</Context.Provider>;
}
