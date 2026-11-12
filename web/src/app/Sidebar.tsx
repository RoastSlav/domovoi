import { useEffect, useRef, type KeyboardEvent } from "react";
import { Link } from "react-router";
import type { LiveStatus } from "../api/events";
import { Button, IconButton, Kbd } from "../components";
import brand from "./brand.module.css";
import { searchShortcut, searchShortcutAria } from "./commandPalette";
import styles from "./sidebar.module.css";
import { SidebarFooter } from "./SidebarFooter";
import { SidebarNav } from "./SidebarNav";

/** The id of the sidebar, which the phone's Menu button controls. */
export const SIDEBAR_ID = "sidebar-panel";

/**
 * The frame's side: brand and collapse button, the search that opens the command palette,
 * grouped navigation with counts, the live state and the account menu. From 768 px it is a
 * column, or an icon rail whose names show as tooltips. On a phone it is a sheet over the
 * page: while open it keeps focus inside, and Escape closes it.
 */
export function Sidebar({
  live,
  open,
  onClose,
  onOpenPalette,
  paletteOpen,
  rail,
  onToggleRail,
}: {
  live: LiveStatus;
  onClose: () => void;
  /** Opens the command palette (the search button's job). */
  onOpenPalette: () => void;
  onToggleRail: () => void;
  /** Whether the phone's sheet is open. */
  open: boolean;
  /** Whether the command palette is open, for the search button's state. */
  paletteOpen: boolean;
  rail: boolean;
}) {
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

  const search = {
    "aria-haspopup": "dialog",
    "aria-expanded": paletteOpen,
    "aria-keyshortcuts": searchShortcutAria(),
    onClick: onOpenPalette,
  } as const;

  return (
    <div
      ref={sheet}
      id={SIDEBAR_ID}
      className={styles.sidebar}
      data-open={open}
      data-rail={rail}
      {...(open ? { role: "dialog", "aria-modal": true, "aria-label": "Menu" } : {})}
      onKeyDown={onKeyDown}
    >
      <div className={styles.head}>
        <Link to="/" className={styles.brand}>
          <span className={brand.mark} aria-hidden="true">
            D
          </span>
          <span className={styles.brandName}>Domovoi</span>
        </Link>
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
      {rail ? (
        <IconButton icon="search" label="Search" {...search} />
      ) : (
        <Button size="sm" icon="search" className={styles.search} {...search}>
          <span className={styles.searchLabel}>Search…</span>
          <Kbd>{searchShortcut()}</Kbd>
        </Button>
      )}
      <SidebarNav rail={rail} />
      <SidebarFooter live={live} />
    </div>
  );
}
