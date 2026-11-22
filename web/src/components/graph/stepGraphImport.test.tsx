import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { OperationTrace } from "../../api/operationTrace";
import { buildGraphModel } from "../../features/operations/graph/graphModel";
import { TRACE_FIXTURES } from "../../test/operationFixtures";
import { StepGraph } from "./StepGraph";

// The first import of the engine fails, as a chunk that did not download; the second works.
let imports = 0;
vi.mock("./GraphEngine", async () => {
  imports += 1;
  if (imports === 1) {
    throw new Error("The chunk did not download.");
  }
  return import("./testEngine");
});

describe("the engine chunk", () => {
  it("a_rejected_engine_import_is_retried_with_a_fresh_import", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const trace = TRACE_FIXTURES["waiting-approval"] as OperationTrace;
    const { model } = buildGraphModel({
      trace,
      profile: "detailed",
      expanded: new Set(),
      selected: null,
      showAll: false,
    });
    render(
      <StepGraph
        model={model}
        selectedId={null}
        onSelect={vi.fn()}
        onOpen={vi.fn()}
        onClear={vi.fn()}
        onToggleGroup={vi.fn()}
      />,
    );
    expect(await screen.findByText("The graph could not be drawn")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("group", { name: "Step graph" })).toBeInTheDocument();
    expect(imports).toBeGreaterThan(1);
    vi.restoreAllMocks();
  });
});
