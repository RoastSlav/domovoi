import { nas } from "../../test/fixtures";
import { MAX_HOSTS, canContinue, initialState, type WizardState } from "./wizardState";

const state = (patch: Partial<WizardState>): WizardState => ({
  step: "choose",
  kind: "install",
  name: "",
  version: "",
  hostIds: [],
  ...patch,
});

describe("wizard state", () => {
  it("wizard_state_decides_the_first_step_and_what_blocks_next", () => {
    const bare = initialState(new URLSearchParams(), [nas], undefined);
    expect(bare).toMatchObject({ step: "choose", kind: "install", hostIds: [] });

    const given = new URLSearchParams("package=nginx-tools&version=1.2.0&host=nas&kind=uninstall");
    expect(initialState(given, [nas], undefined)).toEqual({
      step: "hosts",
      kind: "uninstall",
      name: "nginx-tools",
      version: "1.2.0",
      hostIds: [nas.id],
    });

    const half = initialState(new URLSearchParams("package=nginx-tools"), [nas], undefined);
    expect(half.step).toBe("choose");
    expect(initialState(new URLSearchParams(), [nas], "nas").hostIds).toEqual([nas.id]);
    expect(initialState(new URLSearchParams("host=other"), [nas], "nas").hostIds).toEqual([]);

    expect(canContinue(state({}))).toEqual({ ok: false, reason: "Choose a package first." });
    expect(canContinue(state({ name: "a", version: "1" }))).toEqual({ ok: true });
    expect(canContinue(state({ step: "hosts" }))).toEqual({
      ok: false,
      reason: "Choose at least one host.",
    });
    const many = Array.from({ length: MAX_HOSTS + 1 }, (_, i) => `h${i}`);
    expect(canContinue(state({ step: "hosts", hostIds: many }))).toEqual({
      ok: false,
      reason: "Choose at most 16 hosts.",
    });
    expect(canContinue(state({ step: "hosts", hostIds: many.slice(0, MAX_HOSTS) }))).toEqual({
      ok: true,
    });
    expect(MAX_HOSTS).toBe(16);
  });
});
