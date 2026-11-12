import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { DigestPage } from "./DigestPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/digest/preview", () => ({
    date: "2026-11-06",
    markdown: "## nas\n\n- **2 incidents** resolved",
  }));
});

async function sendNow(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByRole("article", { name: "Digest for 2026-11-06" });
  await user.click(screen.getByRole("button", { name: "Send now" }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Send digest" }));
}

test("digest_page_explains_that_it_is_one_summary_per_day", async () => {
  renderAt("/digest", "/digest", <DigestPage />);

  // The page says what a digest is and what is in it.
  expect(await screen.findByText(/^One summary of every host per day:/)).toBeVisible();
  expect(screen.getByText(/what changed, trends and open incidents\./)).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about the digest (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/digest.html");
});

test("digest_page_shows_the_preview_as_it_will_be_sent", async () => {
  renderAt("/digest", "/digest", <DigestPage />);

  const preview = await screen.findByRole("article", { name: "Digest for 2026-11-06" });
  expect(await within(preview).findByRole("heading", { name: "nas" })).toBeVisible();
  expect(within(preview).getByText("2 incidents").tagName).toBe("STRONG");
  expect(screen.getByRole("region", { name: "Preview" })).toContainElement(preview);
});

test("digest_preview_shows_one_card_per_host_when_the_hub_gives_them", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/digest/preview", () => ({
    date: "2026-11-06",
    markdown: "The whole digest message",
    hosts: [
      { host: "nas", quiet: false, lines: ["**2 incidents** resolved"] },
      { host: "pi", quiet: true, lines: [] },
    ],
  }));
  renderAt("/digest", "/digest", <DigestPage />);

  const preview = await screen.findByRole("article", { name: "Digest for 2026-11-06" });
  expect(within(preview).getByText("nas")).toBeVisible();
  expect(within(preview).getByText("2 incidents").tagName).toBe("STRONG");
  expect(within(preview).getByText("pi")).toBeVisible();
  expect(within(preview).getByText("Nothing changed")).toBeVisible();
  // The whole message stays below the host cards, folded because they repeat it.
  expect(within(preview).queryByText("The whole digest message")).toBeNull();
  await user.click(within(preview).getByRole("button", { name: "Show the whole message as sent" }));
  expect(await within(preview).findByText("The whole digest message")).toBeInTheDocument();
});

test("a_host_with_many_lines_shows_six_and_expands_to_the_rest", async () => {
  const user = userEvent.setup();
  const lines = Array.from({ length: 20 }, (_, i) => `line number ${i + 1}`);
  route("GET /api/v1/digest/preview", () => ({
    date: "2026-11-06",
    markdown: "message",
    hosts: [
      { host: "nas", quiet: false, lines },
      { host: "pi", quiet: false, lines: ["only one"] },
    ],
  }));
  renderAt("/digest", "/digest", <DigestPage />);

  const preview = await screen.findByRole("article", { name: "Digest for 2026-11-06" });
  expect(await within(preview).findByText("line number 6")).toBeVisible();
  expect(within(preview).queryByText("line number 7")).toBeNull();
  expect(within(preview).getAllByRole("button", { name: /^Show \d+ more$/ })).toHaveLength(1);

  await user.click(within(preview).getByRole("button", { name: "Show 14 more" }));
  expect(await within(preview).findByText("line number 20")).toBeVisible();
  expect(within(preview).getByRole("button", { name: "Show fewer" })).toBeVisible();
});

test("digest_send_now_confirms_in_a_toast_that_it_went_to_discord", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/digest/send", () => ({ date: "2026-11-06", markdown: "sent" }));
  renderAt("/digest", "/digest", <DigestPage />);

  await sendNow(user);

  expect(await screen.findByText("Digest sent to Discord")).toBeVisible();
  expect(calls.filter((c) => c.route === "POST /api/v1/digest/send")).toHaveLength(1);
});

test("digest_send_failure_reads_as_words_and_says_why_discord_refused", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/digest/send", () =>
    failure(502, "delivery_failed", "rejected: discord 403: Missing Access"),
  );
  renderAt("/digest", "/digest", <DigestPage />);

  await sendNow(user);

  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not send the digest");
  expect(alert).toHaveTextContent("Missing Access");
  expect(alert).not.toHaveTextContent("delivery_failed");
});

test("digest_preview_error_shows_retry_and_never_an_empty_digest", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/digest/preview", () => failure(500, "internal", "boom"));
  renderAt("/digest", "/digest", <DigestPage />);

  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not build the digest");
  expect(screen.queryByRole("article")).toBeNull();
  expect(screen.getByRole("button", { name: "Send now" })).toHaveAttribute("aria-disabled", "true");

  route("GET /api/v1/digest/preview", () => ({ date: "2026-11-06", markdown: "all quiet" }));
  await user.click(within(alert).getByRole("button", { name: "Retry" }));
  expect(await screen.findByRole("article", { name: "Digest for 2026-11-06" })).toBeVisible();
});

test("digest_preview_says_when_the_next_send_is_due", async () => {
  route("GET /api/v1/digest/preview", () => ({
    date: "2026-11-06",
    markdown: "all quiet",
    next_send_at: inTwoHours(),
  }));
  renderAt("/digest", "/digest", <DigestPage />);

  const preview = await screen.findByRole("article", { name: "Digest for 2026-11-06" });
  expect(within(preview).getByText(/^Next scheduled send:/)).toBeVisible();
});

/** A time two hours from now, as the hub sends it. */
function inTwoHours(): string {
  return new Date(Date.now() + 2 * 3_600_000).toISOString();
}
