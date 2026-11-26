import type { Ref } from "react";
import { Link } from "react-router";
import { IconButton } from "../components";
import { Icon } from "../design/icons";
import brand from "./brand.module.css";
import { HostSelect } from "./HostSelect";
import { OperatorToggle } from "./OperatorToggle";
import { useTrail } from "./PageTitle";
import { SIDEBAR_ID } from "./Sidebar";
import styles from "./topbar.module.css";

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
}: {
  /** The Menu button, so focus can return to it when the sheet closes. */
  menu: Ref<HTMLButtonElement>;
  onMenu: () => void;
  sheetOpen: boolean;
}) {
  return (
    <header className={styles.topbar} data-slot-inert>
      <Link to="/" className={styles.logo} aria-label="Domovoi">
        <span className={brand.mark} aria-hidden="true">
          D
        </span>
      </Link>
      <Breadcrumb />
      <OperatorToggle />
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
