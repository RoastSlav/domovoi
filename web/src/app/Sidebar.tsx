import { useEffect, useRef, type KeyboardEvent } from "react";
import { NavLink, useNavigate } from "react-router";
import type { LiveStatus } from "../api/events";
import { useOverview } from "../api/hooks/overview";
import { useLogout, useSession } from "../api/hooks/session";
import { Count, IconButton, Menu, StatusPill, Tooltip, useActionFeedback } from "../components";
import { Icon } from "../design/icons";
import { useTheme, type Theme } from "../design/theme";
import { liveMeta } from "../design/tone";
import { useSelectedHost } from "./HostContext";
import styles from "./Layout.module.css";
import { NAV_GROUPS, type NavItem } from "./nav";

/** The id of the sidebar, which the phone's Menu button controls. */
export const SIDEBAR_ID = "sidebar-panel";

const THEMES: readonly (readonly [Theme, string])[] = [
  ["system", "Theme: same as the device"],
  ["light", "Theme: light"],
  ["dark", "Theme: dark"],
];

/** One link, with its count when there is something to count and its name as a tooltip on the rail. */
function NavEntry({ item, rail, value }: { item: NavItem; rail: boolean; value: number }) {
  const link = (
    <NavLink
      to={item.to}
      className={({ isActive }) => (isActive ? styles.active : undefined)}
      onPointerEnter={item.preload}
      onFocus={item.preload}
    >
      <Icon name={item.icon} />
      <span className={styles.label}>{item.label}</span>
      {item.count && value > 0 && (
        <>
          {/* Keeps "Approvals" and "2 waiting" apart in the link's name. */}{" "}
          <span className={styles.count}>
            <Count tone={item.count.tone} value={value} label={item.count.label} />
          </span>
        </>
      )}
    </NavLink>
  );
  return (
    <li>
      {rail ? (
        <Tooltip content={item.label} side="right" describe={false}>
          {link}
        </Tooltip>
      ) : (
        link
      )}
    </li>
  );
}

/** Whether changes arrive as they happen: a pill, or on the rail its icon alone. */
function LiveState({ status }: { status: LiveStatus }) {
  const meta = liveMeta(status);
  return (
    <div role="status">
      <span className={styles.livePill}>
        <StatusPill meta={meta} />
      </span>
      <Tooltip content={meta.hint} side="right">
        <span className={styles.liveIcon} data-tone={meta.tone} aria-hidden="true">
          <Icon name={meta.icon} />
        </span>
      </Tooltip>
    </div>
  );
}

/** Who is signed in, the theme, and the way out. Account settings join this menu later. */
function UserMenu() {
  const session = useSession();
  const navigate = useNavigate();
  const logout = useActionFeedback(useLogout(), { success: () => null, error: "sign out" });
  const { theme, setTheme } = useTheme();
  const username = session.data?.user.username ?? "";
  const onLogout = () =>
    logout.mutate(undefined, { onSuccess: () => void navigate("/login", { replace: true }) });
  return (
    <div className={styles.user}>
      <span className={styles.username}>{username}</span>
      <Menu
        label="Account menu"
        icon="user"
        header={`Signed in as ${username}`}
        items={[
          ...THEMES.map(([value, label]) => ({
            label,
            checked: theme === value,
            onSelect: () => setTheme(value),
          })),
          { label: "Log out", icon: "logout", onSelect: onLogout },
        ]}
      />
    </div>
  );
}

/**
 * The brown frame: brand, grouped navigation with counts, the live state and the account
 * menu. From 768 px it is a column, or an icon rail whose names show as tooltips. On a
 * phone it is a sheet over the page: while open it keeps focus inside, and Escape closes it.
 */
export function Sidebar({
  live,
  open,
  onClose,
  rail,
  onToggleRail,
}: {
  live: LiveStatus;
  onClose: () => void;
  onToggleRail: () => void;
  /** Whether the phone's sheet is open. */
  open: boolean;
  rail: boolean;
}) {
  const [host] = useSelectedHost();
  const overview = useOverview(host);
  const sheet = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) {
      sheet.current?.querySelector<HTMLElement>("a[href], button")?.focus();
    }
  }, [open]);

  const onKeyDown = (event: KeyboardEvent): void => {
    if (!open) {
      return;
    }
    if (event.key === "Escape") {
      onClose();
      return;
    }
    if (event.key !== "Tab") {
      return;
    }
    const all = [...(sheet.current?.querySelectorAll<HTMLElement>("a[href], button") ?? [])];
    const first = all[0];
    const last = all.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  return (
    <div
      ref={sheet}
      id={SIDEBAR_ID}
      className={styles.sidebar}
      data-open={open}
      {...(open ? { role: "dialog", "aria-modal": true, "aria-label": "Menu" } : {})}
      onKeyDown={onKeyDown}
    >
      <div className={styles.sidebarHead}>
        <NavLink to="/incidents" className={styles.brand ?? ""}>
          <span className={styles.mark} aria-hidden="true">
            D
          </span>
          <span className={styles.brandName}>Domovoi</span>
        </NavLink>
        <span className={styles.closeButton}>
          <IconButton icon="close" label="Close menu" onClick={onClose} />
        </span>
        <span className={styles.railToggle}>
          <IconButton
            icon={rail ? "chevron-right" : "chevron-left"}
            label={rail ? "Expand the sidebar" : "Collapse the sidebar"}
            aria-expanded={!rail}
            onClick={onToggleRail}
          />
        </span>
      </div>
      <nav aria-label="Main" className={styles.nav}>
        {NAV_GROUPS.map((group) => (
          <div
            key={group.title}
            className={styles.group}
            role="group"
            aria-labelledby={`nav-${group.title}`}
          >
            <h2 id={`nav-${group.title}`} className={styles.groupTitle}>
              {group.title}
            </h2>
            <ul>
              {group.items.map((item) => (
                <NavEntry
                  key={item.to}
                  item={item}
                  rail={rail}
                  value={item.count ? (overview.data?.[item.count.field] ?? 0) : 0}
                />
              ))}
            </ul>
          </div>
        ))}
      </nav>
      <div className={styles.footer}>
        <LiveState status={live} />
        <UserMenu />
      </div>
    </div>
  );
}
