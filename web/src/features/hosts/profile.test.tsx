import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ProfileTab } from "./ProfileTab";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const LATER = "2026-11-01T12:00:30Z";

let collected = T0;
let agent = "0.6.0";

function profileBody() {
  return {
    collected_at: collected,
    note: "note-nas",
    markdown: `Agent version ${agent}.`,
    sections: [
      { title: "Host overview", ok: true },
      { title: "Listening ports", ok: false },
    ],
  };
}

beforeEach(() => {
  resetApi();
  collected = T0;
  agent = "0.6.0";
  route("GET /api/v1/hosts/{name}/profile", () => profileBody());
  route("POST /api/v1/hosts/{name}/profile", () => {
    collected = LATER;
    agent = "0.7.0";
    return profileBody();
  });
});

function show(connected = true) {
  renderAt("/hosts/nas", "/hosts/:name", <ProfileTab host="nas" connected={connected} />);
}

test("profile_tab_shows_the_collection_time_and_links_its_generated_note", async () => {
  show();
  expect(await screen.findByText(/^Collected/)).toBeVisible();
  expect(screen.getByRole("link", { name: "Profile note" })).toHaveAttribute(
    "href",
    "/notes/note-nas",
  );
  expect(await screen.findByText("Agent version 0.6.0.")).toBeVisible();
});

test("profile_tab_names_the_sections_whose_tools_did_not_answer", async () => {
  show();
  expect(await screen.findByText(/^Not available: Listening ports/)).toBeVisible();
});

test("profile_refresh_asks_the_agent_and_shows_the_new_profile", async () => {
  const user = userEvent.setup();
  show();
  await screen.findByText("Agent version 0.6.0.");
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  expect(
    calls.filter((c) => c.route === "POST /api/v1/hosts/{name}/profile"),
    "the click sends the hub's refresh",
  ).toHaveLength(1);
  expect(await screen.findByText("Agent version 0.7.0.")).toBeVisible();
});

test("profile_refresh_repeats_that_the_agent_is_not_connected", async () => {
  route("POST /api/v1/hosts/{name}/profile", () =>
    failure(409, "conflict", "the agent is not connected"),
  );
  const user = userEvent.setup();
  show();
  await screen.findByText("Agent version 0.6.0.");
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  expect(await screen.findByText("The agent is not connected.")).toBeVisible();
});

test("profile_tab_promises_a_first_profile_for_a_connected_host_without_one", async () => {
  route("GET /api/v1/hosts/{name}/profile", () => failure(404, "not_found", "no profile yet"));
  show();
  expect(await screen.findByText(/^Collecting the first profile/)).toBeVisible();
});

test("profile_tab_asks_to_connect_an_agent_before_anything", async () => {
  route("GET /api/v1/hosts/{name}/profile", () => failure(404, "not_found", "no profile yet"));
  show(false);
  expect(await screen.findByText(/^Connect an agent to profile this machine/)).toBeVisible();
  expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
});
