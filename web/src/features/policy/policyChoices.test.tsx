import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { resetApi } from "../../test/apiMock";
import { categoriesOf, categoryTitle, mergeActions, mergeNames } from "./choiceHelpers";
import { entry, serveHosts } from "./pickerFixtures";
import { usePolicyChoices } from "./policyChoices";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const restart = entry("container.restart", "Restart a container", "contain");
const logs = entry("container.logs", "Read container logs", "read");
const backup = entry("skill.backup_db", "Back up the database", "mutate");

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

test("merging_inventories_keeps_each_contract_once_sorted_by_title", () => {
  const merged = mergeActions([
    [restart, logs],
    [restart, backup],
  ]);
  expect(merged.map((a) => a.title)).toEqual([
    "Back up the database",
    "Read container logs",
    "Restart a container",
  ]);
});

test("categories_are_counted_and_given_a_readable_title", () => {
  const actions = mergeActions([[restart, logs, backup]]);
  expect(categoriesOf(actions)).toEqual([
    { category: "container.lifecycle", count: 2 },
    { category: "skill.lifecycle", count: 1 },
  ]);
  expect(categoryTitle("container.lifecycle")).toBe("Container: lifecycle");
  expect(
    mergeNames([
      ["b", "a"],
      ["a", "c"],
    ]),
  ).toEqual(["a", "b", "c"]);
});

test("usePolicyChoices_unions_actions_across_hosts_and_de_duplicates", async () => {
  resetApi();
  serveHosts(
    { alpha: [restart, logs], beta: [restart, backup] },
    { alpha: ["plex"], beta: ["plex", "sonarr"] },
  );
  const { result } = renderHook(() => usePolicyChoices(""), { wrapper });
  await waitFor(() => expect(result.current.actions).toHaveLength(3));
  expect(result.current.failed).toBe(false);
  await waitFor(() => expect(result.current.targets("container")).toEqual(["plex", "sonarr"]));
  expect(result.current.targets("path")).toEqual([]);
});

test("usePolicyChoices_reads_only_the_selected_host", async () => {
  resetApi();
  serveHosts({ alpha: [restart], beta: [backup] }, { alpha: ["plex"], beta: ["sonarr"] });
  const { result } = renderHook(() => usePolicyChoices("host-b"), { wrapper });
  await waitFor(() =>
    expect(result.current.actions.map((a) => a.contract)).toEqual(["skill.backup_db"]),
  );
  await waitFor(() => expect(result.current.targets()).toEqual(["sonarr"]));
});
