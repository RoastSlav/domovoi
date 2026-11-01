import { useEffect } from "react";
import { Navigate, Outlet, Route, Routes, useNavigate } from "react-router";
import { setUnauthorizedHandler } from "../api/client";
import { useSession } from "../api/hooks/session";
import { ApiFailure } from "../api/types";
import { Button } from "../components/Button";
import { EmptyState } from "../components/EmptyState";
import { ApprovalsPage } from "../features/approvals/ApprovalsPage";
import { LoginPage } from "../features/auth/LoginPage";
import { SetupPage } from "../features/auth/SetupPage";
import { IncidentDetailPage } from "../features/incidents/IncidentDetailPage";
import { IncidentListPage } from "../features/incidents/IncidentListPage";
import { Layout } from "./Layout";
import { Placeholder } from "./Placeholder";

/** Sends 401s to the login or setup page through the router instead of a full reload. */
function UnauthorizedRedirect() {
  const navigate = useNavigate();
  useEffect(() => setUnauthorizedHandler((to) => void navigate(to, { replace: true })), [navigate]);
  return null;
}

/** Renders its routes only with a session; the client redirects when there is none. */
function RequireSession() {
  const session = useSession();
  if (session.data) {
    return <Outlet />;
  }
  if (session.error && !(session.error instanceof ApiFailure && session.error.status === 401)) {
    return (
      <main>
        <EmptyState title="Cannot reach the hub">
          <Button onClick={() => void session.refetch()}>Try again</Button>
        </EmptyState>
      </main>
    );
  }
  return null;
}

/** Every page of the app. */
export function AppRoutes() {
  return (
    <>
      <UnauthorizedRedirect />
      <Routes>
        <Route path="/setup" element={<SetupPage />} />
        <Route path="/login" element={<LoginPage />} />
        <Route element={<RequireSession />}>
          <Route element={<Layout />}>
            <Route index element={<Navigate to="/incidents" replace />} />
            <Route path="/incidents" element={<IncidentListPage />} />
            <Route path="/incidents/:id" element={<IncidentDetailPage />} />
            <Route path="/approvals" element={<ApprovalsPage />} />
            <Route path="/hosts" element={<Placeholder title="Hosts" />} />
            <Route path="/notes" element={<Placeholder title="Notes" />} />
            <Route path="/settings" element={<Placeholder title="Settings" />} />
            <Route path="*" element={<Placeholder title="Not found" />} />
          </Route>
        </Route>
      </Routes>
    </>
  );
}
