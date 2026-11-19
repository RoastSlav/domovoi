import type { SurfaceId, SurfaceSpec } from "./types";

/** Everything the slot remembers: who is registered, who is open, where focus returns. */
export interface SlotState {
  specs: Record<SurfaceId, SurfaceSpec>;
  open: SurfaceId[];
  openers: Record<SurfaceId, HTMLElement | null>;
  available: number;
}

/** What can happen to the slot. */
export type SlotAction =
  | { type: "register"; spec: SurfaceSpec }
  | { type: "unregister"; id: SurfaceId }
  | { type: "open"; id: SurfaceId; opener: HTMLElement | null }
  | { type: "close"; id: SurfaceId }
  | { type: "measure"; available: number };

/** The slot before anything registers. */
export const INITIAL_SLOT: SlotState = { specs: {}, open: [], openers: {}, available: 0 };

/** Pure transitions of the slot. Opening an unregistered id changes nothing. */
export function slotReducer(state: SlotState, action: SlotAction): SlotState {
  switch (action.type) {
    case "register":
      return { ...state, specs: { ...state.specs, [action.spec.id]: action.spec } };
    case "unregister": {
      const without = <T>(record: Record<SurfaceId, T>) =>
        Object.fromEntries(Object.entries(record).filter(([id]) => id !== action.id));
      const specs = without(state.specs);
      const openers = without(state.openers);
      return { ...state, specs, openers, open: state.open.filter((id) => id !== action.id) };
    }
    case "open": {
      if (!(action.id in state.specs)) {
        return state;
      }
      const open = [...state.open.filter((id) => id !== action.id), action.id];
      // Bringing an open surface to the front keeps the opener it already had.
      const wasOpen = state.open.includes(action.id);
      const openers = wasOpen ? state.openers : { ...state.openers, [action.id]: action.opener };
      return { ...state, open, openers };
    }
    case "close":
      return { ...state, open: state.open.filter((id) => id !== action.id) };
    case "measure":
      return state.available === action.available
        ? state
        : { ...state, available: action.available };
  }
}
