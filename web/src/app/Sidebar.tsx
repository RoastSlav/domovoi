import { useEffect, useRef, useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router";
import type { LiveStatus } from "../api/events";
import { useActions } from "../api/hooks/actions";
import { useLogout, useSession } from "../api/hooks/session";
import { Button } from "../components/Button";
import { HostSelect } from "./HostSelect";
import styles from "./Layout.module.css";
import { NAV, NavIcon } from "./NavIcon";

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

/** The number of actions waiting for a human, beside the Approvals link. */
function ApprovalsBadge() {
  const proposed = useActions("proposed");
  const count = proposed.data?.items.length ?? 0;
  if (count === 0) {
    return null;
  }
  return (
    <>
      {/* A space that is not laid out in the flex row but keeps "Approvals 2 waiting" apart. */}{" "}
      <span className={styles.badge}>
        {count} <span className="visually-hidden">waiting</span>
      </span>
    </>
  );
}

/** The signed-in user and the way out. */
function UserMenu() {
  const session = useSession();
  const logout = useLogout();
  const navigate = useNavigate();
  const onLogout = () =>
    logout.mutate(undefined, { onSettled: () => void navigate("/login", { replace: true }) });
  return (
    <div className={styles.user}>
      <span className={styles.username}>{session.data?.user.username}</span>
      <Button variant="ghost" onClick={onLogout} disabled={logout.isPending}>
        Log out
      </Button>
    </div>
  );
}

/**
 * The left column on wide screens; on narrow ones a top bar (brand, host, Menu) whose
 * navigation opens as a sheet. It closes on any navigation and on Escape.
 */
export function Sidebar({ live }: { live: LiveStatus }) {
  const location = useLocation();
  // Open for one location only, so navigating (or picking a host) closes the sheet by itself.
  const [openFor, setOpenFor] = useState<string | null>(null);
  const open = openFor === location.key;
  const menu = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpenFor(null);
        menu.current?.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);

  return (
    <aside className={styles.sidebar}>
      <div className={styles.top}>
        <NavLink to="/incidents" className={styles.brand ?? ""}>
          Domovoi
        </NavLink>
        <HostSelect />
        <button
          ref={menu}
          type="button"
          className={styles.menuButton}
          aria-expanded={open}
          aria-controls="sidebar-panel"
          onClick={() => setOpenFor(open ? null : location.key)}
        >
          Menu
        </button>
      </div>
      <div id="sidebar-panel" className={styles.panel} data-open={open}>
        <nav aria-label="Main" className={styles.nav}>
          {NAV.map(([to, label, icon]) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) => (isActive ? styles.active : undefined)}
            >
              <NavIcon name={icon} />
              <span>{label}</span>
              {to === "/approvals" && <ApprovalsBadge />}
            </NavLink>
          ))}
        </nav>
        <div className={styles.footer}>
          <LiveIndicator status={live} />
          <UserMenu />
        </div>
      </div>
    </aside>
  );
}
