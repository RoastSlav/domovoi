import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Fact } from "../../api/types";
import { resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { HostDetailPage } from "./HostDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

const minutesAgo = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString();

function fact(key: string, label: string, value: string, unit?: string, minutes = 2): Fact {
  return { key, label, value, unit: unit ?? null, observed_at: minutesAgo(minutes) };
}

const STATES = [
  ["web", "running (healthy)"],
  ["db", "running (healthy)"],
  ["cache", "running"],
  ["queue", "running"],
  ["proxy", "running"],
  ["mail", "running"],
  ["wiki", "running"],
  ["backup", "restarting (1) 12 seconds ago; restarted 412 times"],
  ["cadvisor", "running (unhealthy)"],
  ["zigbee", "exited (137) 2 days ago"],
  ["created-one", "created"],
  ["zz-last", "running"],
] as const;

const FACTS: Fact[] = [
  fact("cpu.usage", "CPU usage", "83", "%"),
  fact("load.avg1", "Load average (1 min)", "7.82"),
  fact("load.avg5", "Load average (5 min)", "6.14"),
  fact("load.avg15", "Load average (15 min)", "5.03"),
  fact("memory.total", "Memory total", "62.6", "GiB"),
  fact("memory.available", "Memory available", "1.9", "GiB"),
  fact("swap.used_pct", "Swap used", "77", "%"),
  fact("system.uptime", "Uptime", "41 days 6 hours"),
  fact("temperature.cpu", "CPU temperature", "78", "°C"),
  fact("temperature.nvme0", "NVMe 0 temperature", "63", "°C"),
  fact("disk.root.used_pct", "Disk / used", "58", "%"),
  fact("disk.root.free", "Disk / free", "212", "GiB"),
  fact("disk.srv.used_pct", "Disk /srv used", "97", "%"),
  fact("disk.srv.free", "Disk /srv free", "31", "GiB"),
  fact("disk.data.used_pct", "Disk /data used", "88", "%"),
  fact("zfs.tank.health", "ZFS pool tank health", "DEGRADED (1 device faulted)"),
  fact("smart.sda.health", "SMART sda health", "PASSED", undefined, 200),
  fact("net.eth0.rx_rate", "eth0 receive", "412.7", "Mbit/s"),
  fact("system.kernel", "Kernel", "6.8.0-48-generic"),
  fact("system.updates", "Pending updates", "47 (12 security)", undefined, 200),
  ...STATES.flatMap(([name, state]) => [
    fact(`container.${name}.state`, `${name} state`, state),
    fact(`container.${name}.image`, `${name} image`, `registry.example.net/${name}:1.0`),
    fact(`container.${name}.ports`, `${name} ports`, "8080/tcp -> 0.0.0.0:8080"),
  ]),
];

function show(facts: Fact[] = FACTS) {
  route("GET /api/v1/hosts/{name}", () => ({ host: nas, facts }));
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/hosts/{name}/skills", () => []);
  return renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
}

/** The health tile labelled `label`, as the element that holds its number and hint. */
async function tile(label: string): Promise<HTMLElement> {
  const text = await screen.findByText(label, { selector: "p" });
  return text.closest("div") as HTMLElement;
}

test("the_health_tiles_show_cpu_load_memory_swap_uptime_and_the_hottest_sensor", async () => {
  show();
  expect(await tile("CPU usage")).toHaveTextContent("83 %");
  expect(await tile("Load")).toHaveTextContent("7.825 min 6.14 · 15 min 5.03");
  expect(await tile("Memory")).toHaveTextContent("60.7 GiB usedof 62.6 GiB");
  expect(await tile("Swap")).toHaveTextContent("77 %");
  expect(await tile("Uptime")).toHaveTextContent("41 days 6 hours");
  expect(await tile("Temperature")).toHaveTextContent("78 °CCPU temperature");
});

test("health_tiles_are_toned_by_thresholds", async () => {
  show();
  const toneOf = async (label: string) =>
    (await tile(label)).querySelector("[data-tone]")?.getAttribute("data-tone");
  expect(await toneOf("CPU usage")).toBe("warning");
  expect(await toneOf("Memory")).toBe("danger");
  expect(await toneOf("Swap")).toBe("warning");
  expect(await toneOf("Temperature")).toBeUndefined();
});

test("the_temperature_tile_lists_every_sensor_in_a_tooltip", async () => {
  const user = userEvent.setup();
  show();
  const hint = within(await tile("Temperature")).getByText("CPU temperature");
  await user.hover(hint);
  expect(await screen.findByText("CPU temperature 78 °C, NVMe 0 temperature 63 °C")).toBeVisible();
});

test("disks_are_one_compact_row_each_fullest_first_with_a_toned_bar", async () => {
  show();
  const list = await screen.findByRole("list", { name: "Disk usage" });
  const rows = within(list).getAllByRole("listitem");
  expect(rows.map((row) => row.textContent)).toEqual([
    expect.stringContaining("/srv"),
    expect.stringContaining("/data"),
    expect.stringMatching(/^\/.*58 %/),
  ]);
  expect(rows[0]).toHaveTextContent("97 % · 31 GiB free");
  expect(within(list).getByRole("progressbar", { name: "/srv used" })).toHaveAttribute(
    "data-tone",
    "danger",
  );
  expect(within(list).getByRole("progressbar", { name: "/data used" })).toHaveAttribute(
    "data-tone",
    "warning",
  );
  expect(
    within(list).getByRole("progressbar", { name: "/ used" }).getAttribute("data-tone"),
  ).toBeNull();
  expect(within(rows[0] as HTMLElement).getByText("Almost full")).toBeVisible();
});

test("pool_and_smart_state_are_status_lines_under_the_disks", async () => {
  show();
  const disks = await screen.findByRole("group", { name: "Disks" });
  const pool = within(disks).getByText("DEGRADED (1 device faulted)");
  expect(pool.closest("[data-tone]")).toHaveAttribute("data-tone", "danger");
  expect(within(disks).getByText("PASSED")).toBeVisible();
});

test("containers_are_one_table_with_problems_first_and_show_all_for_the_rest", async () => {
  const user = userEvent.setup();
  show();
  const table = await screen.findByRole("table", { name: "Containers on nas" });
  const names = () =>
    within(table)
      .getAllByRole("row")
      .slice(1)
      .map((row) => within(row).getAllByRole("cell")[0]?.textContent);
  expect(names()).toEqual([
    "backup",
    "cadvisor",
    "zigbee",
    "cache",
    "created-one",
    "db",
    "mail",
    "proxy",
  ]);
  expect(within(table).getByText("Restarting")).toBeVisible();
  expect(within(table).getByText("Unhealthy")).toBeVisible();
  expect(within(table).getByText("Exited")).toBeVisible();
  const backup = within(table).getByText("backup").closest("tr") as HTMLElement;
  expect(within(backup).getByText("412")).toBeVisible();

  await user.click(screen.getByRole("button", { name: "Show all 12" }));
  expect(names()).toHaveLength(12);
  expect(screen.getByRole("button", { name: "Show fewer" })).toBeVisible();
});

test("every_other_fact_waits_behind_a_collapsed_more_facts_disclosure", async () => {
  const user = userEvent.setup();
  show();
  const toggle = await screen.findByRole("button", { name: "More facts (2)" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("Pending updates")).toBeNull();
  await user.click(toggle);
  expect(screen.getByText("Pending updates")).toBeVisible();
  expect(screen.getByText("Kernel")).toBeVisible();
});

test("the_network_facts_are_a_small_group_of_their_own", async () => {
  show();
  const network = await screen.findByRole("group", { name: "Network" });
  expect(within(network).getByText("eth0 receive")).toBeVisible();
  expect(within(network).getByText("412.7 Mbit/s")).toBeVisible();
});

test("each_part_says_once_when_it_was_updated_and_only_stale_facts_are_marked", async () => {
  show();
  await screen.findByRole("group", { name: "Disks" });
  expect(screen.queryByText(/^seen /)).toBeNull();
  expect(screen.getAllByText(/^Updated/).length).toBeGreaterThanOrEqual(4);
  const disks = screen.getByRole("group", { name: "Disks" });
  const stale = within(disks).getAllByText(/^Stale:/);
  expect(stale).toHaveLength(1);
  expect(stale[0]?.closest("dd")).toHaveTextContent("PASSED");
  expect(within(screen.getByRole("group", { name: "Network" })).queryByText(/^Stale:/)).toBeNull();
});
