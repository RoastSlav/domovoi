import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { useVisibility } from "../components";
import { failure, resetApi, route } from "../test/apiMock";
import { VisibilityProvider } from "./VisibilityProvider";
import { readCache, writeCache } from "./visibilityCache";

vi.mock("../api/client", () => import("../test/apiMock"));

function Probe() {
  const v = useVisibility("P3");
  return <output data-testid="p3">{v.presentation}</output>;
}

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <VisibilityProvider>
        <Probe />
      </VisibilityProvider>
    </QueryClientProvider>,
  );
}

const answer = (profile: string) => ({
  version: 1,
  updated_at: null,
  presentation: { profile, pages: {} },
});

beforeEach(() => resetApi());

test("provider_uses_the_cache_until_the_hub_answers_then_the_hub_wins", async () => {
  writeCache({ profile: "standard", pages: {} });
  let release: (value: unknown) => void = () => {};
  route("GET /api/v1/me/preferences", () => new Promise((resolve) => (release = resolve)));
  mount();
  expect(screen.getByTestId("p3")).toHaveTextContent("collapsed");
  release(answer("essential"));
  await waitFor(() => expect(screen.getByTestId("p3")).toHaveTextContent("hidden"));
  expect(readCache()?.profile).toBe("essential");
});

test("provider_shows_detailed_when_the_hub_fails_and_nothing_is_cached", async () => {
  route("GET /api/v1/me/preferences", () => failure(500, "internal", "boom"));
  mount();
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(screen.getByTestId("p3")).toHaveTextContent("shown");
  expect(screen.queryByRole("alert")).toBeNull();
});

test("provider_reads_an_unknown_profile_from_the_hub_as_detailed", async () => {
  route("GET /api/v1/me/preferences", () => answer("compact"));
  mount();
  await waitFor(() => expect(readCache()?.profile).toBe("detailed"));
  expect(screen.getByTestId("p3")).toHaveTextContent("shown");
});
