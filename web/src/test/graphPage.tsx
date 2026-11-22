import { screen, waitFor } from "@testing-library/react";
import type { OperationDetail } from "../api/types";
import { showOperation } from "./operationPage";

/** The graph's focusable root, or null while the graph is not on the page. */
export const graphRoot = (): HTMLElement | null =>
  document.querySelector<HTMLElement>("[data-graph]");

const words = (value: string | null): string[] => (value ?? "").split(" ").filter(Boolean);

/** The ids the graph draws, from the root's `data-graph-nodes`. */
export const drawnNodes = (root: HTMLElement): string[] =>
  words(root.getAttribute("data-graph-nodes"));

/** The folded groups the graph draws, from the root's `data-graph-groups`. */
export const drawnGroups = (root: HTMLElement): string[] =>
  words(root.getAttribute("data-graph-groups"));

/** The page's current address (path and query), as the test router shows it. */
export const address = (): string => screen.getByTestId("location").textContent ?? "";

/** The outline's link for a step. */
export const outlineLink = (id: string): HTMLElement =>
  document.querySelector<HTMLElement>(`a[href$="?step=${id}"]`) as HTMLElement;

/** Renders the operation page in the Graph view and waits until the graph has laid out. */
export async function openGraphPage(
  detail: OperationDetail,
  options: Parameters<typeof showOperation>[1] = {},
) {
  const view = showOperation(detail, options);
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
  const root = await waitFor(() => {
    const found = graphRoot();
    expect(found).not.toBeNull();
    expect(found).toHaveAttribute("data-ready", "true");
    return found as HTMLElement;
  });
  return { ...view, root };
}
