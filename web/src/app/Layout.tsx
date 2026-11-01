import { NavLink, Outlet, useNavigate } from "react-router";
import { useLiveEvents, type LiveStatus } from "../api/events";
import { useHosts } from "../api/hooks/hosts";
import { useLogout, useSession } from "../api/hooks/session";
import { Button } from "../components/Button";
import styles from "./Layout.module.css";

const NAV = [
  ["/incidents", "Incidents"],
  ["/approvals", "Approvals"],
  ["/hosts", "Hosts"],
  ["/notes", "Notes"],
  ["/settings", "Settings"],
] as const;

const LIVE_TEXT: Record<LiveStatus, string> = {
  live: "Live",
  connecting: "Connecting",
  offline: "Offline",
};

function LiveIndicator({ status }: { status: LiveStatus }) {
  return (
    <span className={styles.live} data-status={status} role="status" aria-live="polite">
      <span className={styles.dot} aria-hidden="true" />
      {LIVE_TEXT[status]}
    </span>
  );
}

function HostPills() {
  const hosts = useHosts();
  if (!hosts.data?.length) {
    return null;
  }
  return (
    <ul className={styles.pills} aria-label="Hosts">
      {hosts.data.map((host) => (
        <li key={host.id} className={styles.pill} data-connected={host.connected}>
          <span className={styles.dot} aria-hidden="true" />
          {host.name}
          <span className="visually-hidden">{host.connected ? " connected" : " offline"}</span>
        </li>
      ))}
    </ul>
  );
}

/** The frame of every logged-in page: top bar, navigation and the page itself. */
export function Layout() {
  const live = useLiveEvents();
  const session = useSession();
  const logout = useLogout();
  const navigate = useNavigate();

  const onLogout = () =>
    logout.mutate(undefined, { onSettled: () => void navigate("/login", { replace: true }) });

  return (
    <div className={styles.shell}>
      <a className={styles.skip} href="#main">
        Skip to content
      </a>
      <header className={styles.header}>
        <div className={styles.bar}>
          <NavLink to="/incidents" className={styles.brand ?? ""}>
            Domovoi
          </NavLink>
          <HostPills />
          <LiveIndicator status={live} />
          <span className={styles.user}>
            {session.data?.user.username}
            <Button onClick={onLogout} disabled={logout.isPending}>
              Log out
            </Button>
          </span>
        </div>
        <nav aria-label="Main" className={styles.nav}>
          {NAV.map(([to, label]) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) => (isActive ? styles.active : undefined)}
            >
              {label}
            </NavLink>
          ))}
        </nav>
      </header>
      <main id="main" className={styles.main} tabIndex={-1}>
        <Outlet />
      </main>
    </div>
  );
}
