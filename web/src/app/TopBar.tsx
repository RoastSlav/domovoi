import { createContext, useContext, type ReactNode, type Ref } from "react";
import { createPortal } from "react-dom";
import { Link } from "react-router";
import { IconButton } from "../components";
import { Icon } from "../design/icons";
import brand from "./brand.module.css";
import { HostSelect } from "./HostSelect";
import { useTrail } from "./PageTitle";
import { SIDEBAR_ID } from "./Sidebar";
import styles from "./topbar.module.css";

/** The element in the top bar that holds the current page's actions. */
export const TopBarSlot = createContext<HTMLElement | null>(null);

/**
 * Puts a page's actions in the top bar, where they stay in reach while the page scrolls.
 * Use it for the one or two actions that belong to the whole page; everything else stays in
 * the page's `PageHeader`. Outside the shell it renders nothing.
 */
export function TopBarActions({ children }: { children: ReactNode }) {
  const slot = useContext(TopBarSlot);
  return slot ? createPortal(children, slot) : null;
}

/** Where the page is: its section, and on a page about one thing, a way back and its name. */
function Breadcrumb() {
  const { section, sectionItem, detail } = useTrail();
  return (
    <nav aria-label="Breadcrumb" className={styles.crumbs}>
      <ol>
        {section && (
          <li aria-current={detail ? undefined : "page"}>
            {detail && sectionItem ? <Link to={sectionItem.to}>{section}</Link> : section}
          </li>
        )}
        {detail && (
          <li aria-current="page">
            <Icon name="chevron-right" size={14} />
            {detail}
          </li>
        )}
      </ol>
    </nav>
  );
}

/**
 * The slim bar above every page: where you are, the page's actions, and the host selector.
 * On a phone it carries the logo and the Menu button that opens the navigation instead of
 * the breadcrumb.
 */
export function TopBar({
  menu,
  sheetOpen,
  onMenu,
  slot,
}: {
  /** The Menu button, so focus can return to it when the sheet closes. */
  menu: Ref<HTMLButtonElement>;
  onMenu: () => void;
  sheetOpen: boolean;
  /** Receives the element that `TopBarActions` fills. */
  slot: (element: HTMLElement | null) => void;
}) {
  return (
    <header className={styles.topbar}>
      <Link to="/" className={styles.logo} aria-label="Domovoi">
        <span className={brand.mark} aria-hidden="true">
          D
        </span>
      </Link>
      <Breadcrumb />
      <div ref={slot} className={styles.actions} />
      <div className={styles.hostSelect}>
        <HostSelect />
      </div>
      <span className={styles.menuButton}>
        <IconButton
          ref={menu}
          icon="menu"
          label="Menu"
          aria-expanded={sheetOpen}
          aria-controls={SIDEBAR_ID}
          onClick={onMenu}
        />
      </span>
    </header>
  );
}
