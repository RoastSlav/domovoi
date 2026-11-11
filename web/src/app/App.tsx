import { QueryClientProvider } from "@tanstack/react-query";
import { createBrowserRouter, RouterProvider } from "react-router";
import { queryClient } from "../api/queryClient";
import { ToastProvider } from "../components";
import { AppRoutes } from "./routes";

// A data router, so a page with unsaved changes can hold back a navigation (Back included).
// Every path goes to `AppRoutes`, which keeps the route table in one place.
const router = createBrowserRouter([{ path: "*", element: <AppRoutes /> }]);

/** Root component: providers and routes. */
export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>
  );
}
