import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { calls, resetApi, route } from "../../test/apiMock";
import { keys } from "../queryClient";
import { usePreferences, useSavePreferences } from "./preferences";

vi.mock("../client", () => import("../../test/apiMock"));

const saved = {
  version: 1,
  updated_at: "2026-01-01T00:00:00Z",
  presentation: { profile: "standard", pages: { operation: "essential" } },
};

beforeEach(() => resetApi());

test("preferences_hooks_send_the_documented_requests", async () => {
  route("GET /api/v1/me/preferences", () => ({
    ...saved,
    updated_at: null,
    presentation: { profile: "detailed", pages: {} },
  }));
  route("PUT /api/v1/me/preferences", () => saved);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const read = renderHook(() => usePreferences(), { wrapper });
  await waitFor(() => expect(read.result.current.data?.presentation.profile).toBe("detailed"));

  const write = renderHook(() => useSavePreferences(), { wrapper });
  await write.result.current.mutateAsync({
    message: "not sent",
    presentation: { profile: "standard", pages: { operation: "essential" } },
  });
  expect(calls.find((call) => call.route === "PUT /api/v1/me/preferences")?.init.body).toEqual({
    presentation: { profile: "standard", pages: { operation: "essential" } },
  });
  expect(client.getQueryData(keys.preferences)).toEqual(saved);

  const second = renderHook(() => usePreferences(), { wrapper });
  expect(second.result.current.data).toEqual(saved);
  expect(calls.filter((call) => call.route === "GET /api/v1/me/preferences")).toHaveLength(1);
});
