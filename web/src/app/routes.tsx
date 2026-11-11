import { lazy, Suspense, useEffect } from "react";
import { Navigate, Outlet, Route, Routes, useNavigate } from "react-router";
import { setUnauthorizedHandler } from "../api/client";
import { useSession } from "../api/hooks/session";
import { ApiFailure } from "../api/types";
import { Button } from "../components/Button";
import { EmptyState } from "../components/EmptyState";
import { ApprovalsPage } from "../features/approvals/ApprovalsPage";
import { LoginPage } from "../features/auth/LoginPage";
import { ChatPage } from "../features/chat/ChatPage";
import { DigestPage } from "../features/digest/DigestPage";
import { LogsPage } from "../features/logs/LogsPage";
import { HostDetailPage } from "../features/hosts/HostDetailPage";
import { HostsPage } from "../features/hosts/HostsPage";
import { SetupPage } from "../features/auth/SetupPage";
import { IncidentDetailPage } from "../features/incidents/IncidentDetailPage";
import { IncidentListPage } from "../features/incidents/IncidentListPage";
import { NoteEditorPage } from "../features/notes/NoteEditorPage";
import { NotesPage } from "../features/notes/NotesPage";
import { RulesPage } from "../features/rules/RulesPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { FirstRun } from "../features/setup/FirstRun";
import { SkillsPage } from "../features/skills/SkillsPage";
import { Layout } from "./Layout";
import { Placeholder } from "./Placeholder";

/** The design catalogue; the dev server only, so production bundles never contain it. */
const DesignPage = import.meta.env.DEV ? lazy(() => import("../dev/DesignPage")) : null;

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
        {DesignPage && (
          <Route
            path="/dev/design"
            element={
              <Suspense fallback={null}>
                <DesignPage />
              </Suspense>
            }
          />
        )}
        <Route element={<RequireSession />}>
          <Route element={<Layout />}>
            <Route index element={<Navigate to="/incidents" replace />} />
            <Route path="/incidents" element={<IncidentListPage />} />
            <Route path="/incidents/:id" element={<IncidentDetailPage />} />
            <Route path="/chat" element={<ChatPage />} />
            <Route path="/rules" element={<RulesPage />} />
            <Route path="/approvals" element={<ApprovalsPage />} />
            <Route path="/hosts" element={<HostsPage />} />
            <Route path="/hosts/:name" element={<HostDetailPage />} />
            <Route path="/skills" element={<SkillsPage />} />
            <Route path="/notes" element={<NotesPage />} />
            <Route path="/notes/:id" element={<NoteEditorPage />} />
            <Route path="/digest" element={<DigestPage />} />
            <Route path="/logs" element={<LogsPage />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="/welcome" element={<FirstRun />} />
            <Route path="*" element={<Placeholder title="Not found" />} />
          </Route>
        </Route>
      </Routes>
    </>
  );
}
