import { useState, type ReactElement } from "react";
import type { Capabilities, HostDetail } from "../../api/types";
import { route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { newRule, type RuleForm } from "./ruleModel";

type Entry = Capabilities["entries"][number];

/** One inventory row for the picker tests. */
export function entry(contract: string, title: string, effect: string, extra = {}): Entry {
  return {
    catalogue: { state: "complete" },
    category: `${contract.split(".")[0]}.lifecycle`,
    contract,
    describe: `${title} on the host.`,
    effect,
    execution: "agent",
    from_hello_skills: false,
    in_container: false,
    root_agent: false,
    runner_installed: false,
    state: "available",
    target_kind: "container",
    title,
    version: 1,
    ...extra,
  };
}

/** The two hosts the picker tests use. */
export const HOST_A = { ...nas, id: "host-a", name: "alpha" };
export const HOST_B = { ...nas, id: "host-b", name: "beta" };

/** Registers the two hosts, their inventories and their container facts. */
export function serveHosts(
  caps: Record<string, Entry[]>,
  containers: Record<string, string[]> = {},
) {
  route("GET /api/v1/hosts", () => [HOST_A, HOST_B]);
  route("GET /api/v1/hosts/{name}/capabilities", (init) => ({
    host: init.params?.path?.name ?? "",
    entries: caps[init.params?.path?.name ?? ""] ?? [],
    unknown_contracts: [],
  }));
  route("GET /api/v1/hosts/{name}", (init): HostDetail => {
    const name = init.params?.path?.name ?? "";
    return {
      host: name === "beta" ? HOST_B : HOST_A,
      facts: (containers[name] ?? []).map((c) => ({
        key: `container.${c}.state`,
        label: c,
        observed_at: "2026-10-31T12:00:00Z",
        value: "running",
      })),
      retired_facts: [],
    };
  });
}

/** Holds a rule form so a picker can be exercised and its result read back from the page. */
export function Harness({
  initial,
  children,
}: {
  children: (form: RuleForm, change: (patch: Partial<RuleForm>) => void) => ReactElement;
  initial?: Partial<RuleForm>;
}) {
  const [form, setForm] = useState<RuleForm>({ ...newRule([]), ...initial });
  return (
    <>
      {children(form, (patch) => setForm((now) => ({ ...now, ...patch })))}
      <output data-testid="form">{JSON.stringify(form)}</output>
    </>
  );
}

/** Renders `element` with a query client. */
export function show(element: ReactElement) {
  return renderAt("/", "/", element);
}
