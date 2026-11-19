import { INITIAL_SLOT, slotReducer, type SlotState } from "./state";
import type { SurfaceSpec } from "./types";

const spec = (id: string): SurfaceSpec => ({ id, label: id, icon: "info" });

function withSurfaces(...ids: string[]): SlotState {
  return ids.reduce(
    (state, id) => slotReducer(state, { type: "register", spec: spec(id) }),
    INITIAL_SLOT,
  );
}

test("slot_state_open_moves_an_open_surface_to_the_front_and_ignores_unregistered_ids", () => {
  let state = withSurfaces("a", "b");
  state = slotReducer(state, { type: "open", id: "a", opener: null });
  state = slotReducer(state, { type: "open", id: "b", opener: null });
  expect(state.open).toEqual(["a", "b"]);
  state = slotReducer(state, { type: "open", id: "a", opener: null });
  expect(state.open).toEqual(["b", "a"]);
  expect(slotReducer(state, { type: "open", id: "ghost", opener: null })).toBe(state);
});

test("slot_state_unregister_removes_the_surface_from_open", () => {
  let state = withSurfaces("a");
  state = slotReducer(state, { type: "open", id: "a", opener: null });
  state = slotReducer(state, { type: "unregister", id: "a" });
  expect(state.open).toEqual([]);
  expect(state.specs).toEqual({});
});
