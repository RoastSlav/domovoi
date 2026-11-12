import { NavLink } from "react-router";
import { useOverview } from "../api/hooks/overview";
import { Count, Tooltip } from "../components";
import { Icon } from "../design/icons";
import { useSelectedHost } from "./HostContext";
import { NAV_GROUPS, type NavItem } from "./nav";
import styles from "./sidebar.module.css";

/** One link, with its count when there is something to count and its name as a tooltip on the rail. */
function NavEntry({ item, rail, value }: { item: NavItem; rail: boolean; value: number }) {
  const link = (
    <NavLink
      to={item.to}
      end={item.to === "/"}
      className={({ isActive }) => (isActive ? `${styles.link} ${styles.active}` : styles.link)}
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

/** The grouped links; counts follow the selected host and show only above zero. */
export function SidebarNav({ rail }: { rail: boolean }) {
  const [host] = useSelectedHost();
  const overview = useOverview(host);
  return (
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
  );
}
