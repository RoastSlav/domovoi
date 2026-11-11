import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { createMemoryRouter, Route, RouterProvider, Routes, useLocation } from "react-router";
import { HostProvider } from "../app/HostContext";
import { ToastProvider } from "../components/Toast";

/** Shows the current location, so tests can check where a page navigated. */
export function Location() {
  const location = useLocation();
  return <output data-testid="location">{location.pathname + location.search}</output>;
}

/** Renders `element` at `path` under `url`, with a fresh query client and stub routes. */
export function renderAt(url: string, path: string, element: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  // A data router, as the app uses, so pages that hold back a navigation work in tests.
  const router = createMemoryRouter(
    [
      {
        path: "*",
        element: (
          <>
            <Routes>
              <Route path={path} element={<HostProvider>{element}</HostProvider>} />
              <Route path="*" element={<p>other page</p>} />
            </Routes>
            <Location />
          </>
        ),
      },
    ],
    { initialEntries: [url] },
  );
  const result = render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { ...result, client };
}
