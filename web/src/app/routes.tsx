import { lazy, Suspense, useEffect } from "react";
import { Outlet, Route, Routes, useNavigate } from "react-router";
import { setUnauthorizedHandler } from "../api/client";
import { useSession } from "../api/hooks/session";
import { ApiFailure } from "../api/types";
import { LoadError } from "../components";
import { LoginPage } from "../features/auth/LoginPage";
import { SetupPage } from "../features/auth/SetupPage";
import { IncidentDetailPage } from "../features/incidents/IncidentDetailPage";
import { IncidentListPage } from "../features/incidents/IncidentListPage";
import { Layout } from "./Layout";
import styles from "./Layout.module.css";
import { NotFoundPage } from "./NotFoundPage";
import { ShellSkeleton } from "./PageSkeleton";
import { PAGES } from "./pages";

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
      <main className={styles.content}>
        <LoadError error={session.error} context="reach the hub" onRetry={session.refetch} />
      </main>
    );
  }
  // Never a blank screen: the shell's shape, until the session is known.
  return <ShellSkeleton />;
}

/**
 * Every page of the app. Incidents, Login and Setup are in the first download; the rest
 * are fetched when first opened (see `pages.ts`), inside the layout's `Suspense`.
 */
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
            <Route index element={<PAGES.overview.Page />} />
            <Route path="/attention" element={<PAGES.attention.Page />} />
            <Route path="/search" element={<PAGES.search.Page />} />
            <Route path="/incidents" element={<IncidentListPage />} />
            <Route path="/incidents/:id" element={<IncidentDetailPage />} />
            <Route path="/operations" element={<PAGES.operations.Page />} />
            <Route path="/operations/:id" element={<PAGES.operation.Page />} />
            <Route path="/operator" element={<PAGES.operator.Page />} />
            <Route path="/operator/:id" element={<PAGES.operator.Page />} />
            <Route path="/rules" element={<PAGES.rules.Page />} />
            <Route path="/approvals" element={<PAGES.approvals.Page />} />
            <Route path="/hosts" element={<PAGES.hosts.Page />} />
            <Route path="/hosts/:name" element={<PAGES.host.Page />} />
            <Route path="/skills" element={<PAGES.skills.Page />} />
            <Route path="/capabilities" element={<PAGES.capabilities.Page />} />
            <Route path="/capabilities/install" element={<PAGES.packageInstall.Page />} />
            <Route path="/capabilities/jobs/:id" element={<PAGES.installJob.Page />} />
            <Route path="/capabilities/helper-key" element={<PAGES.helperKey.Page />} />
            <Route path="/notes" element={<PAGES.notes.Page />} />
            <Route path="/notes/:id" element={<PAGES.note.Page />} />
            <Route path="/digest" element={<PAGES.digest.Page />} />
            <Route path="/audit" element={<PAGES.audit.Page />} />
            <Route path="/logs" element={<PAGES.logs.Page />} />
            <Route path="/knowledge" element={<PAGES.knowledge.Page />} />
            <Route path="/knowledge/facts/:id" element={<PAGES.fact.Page />} />
            <Route path="/knowledge/entities/:id" element={<PAGES.entity.Page />} />
            <Route path="/knowledge/paths" element={<PAGES.paths.Page />} />
            <Route path="/knowledge/experience" element={<PAGES.experience.Page />} />
            <Route path="/policy" element={<PAGES.policy.Page />} />
            <Route path="/policy/versions/:number" element={<PAGES.policyVersion.Page />} />
            <Route path="/policy/draft" element={<PAGES.policyDraft.Page />} />
            <Route path="/policy/draft/publish" element={<PAGES.policyPublish.Page />} />
            <Route path="/policy/decisions/:execution" element={<PAGES.policyDecision.Page />} />
            <Route path="/policy/simulator" element={<PAGES.policySimulator.Page />} />
            <Route path="/settings" element={<PAGES.settings.Page />} />
            <Route path="/settings/:section" element={<PAGES.settings.Page />} />
            <Route path="/welcome" element={<PAGES.firstRun.Page />} />
            <Route path="*" element={<NotFoundPage />} />
          </Route>
        </Route>
      </Routes>
    </>
  );
}
