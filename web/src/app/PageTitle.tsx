import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useLocation } from "react-router";
import { NAV_ITEMS, type NavItem } from "./nav";

/** The product's name, last in every document title. */
export const APP_NAME = "Domovoi";

/** Sections that are not in the navigation. */
const OTHER_SECTIONS: Record<string, string> = { welcome: "Setup" };

/** Where the current page is: its section of the navigation and, on a page about one thing, that thing. */
export interface Trail {
  detail?: string | undefined;
  /** The section's name; absent on a path the app does not know. */
  section?: string | undefined;
  /** The section's own page, when it is in the navigation. */
  sectionItem?: NavItem | undefined;
}

interface Named {
  name: string;
  path: string;
}

const NameContext = createContext<((named: Named | null) => void) | null>(null);
const TrailContext = createContext<Trail>({});

function decoded(segment: string): string {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

/** What a page about one thing is called until the page itself says (`usePageName`). */
function defaultDetail(section: string, rest: string | undefined): string | undefined {
  if (!rest) {
    return undefined;
  }
  switch (section) {
    case "hosts":
      return decoded(rest);
    case "incidents":
      return "Incident";
    case "notes":
      return rest === "new" ? "New note" : "Note";
    default:
      return undefined;
  }
}

/** The title of the browser tab for `trail`: the most specific part first. */
export function documentTitle(trail: Trail): string {
  return [trail.detail, trail.section, APP_NAME].filter(Boolean).join(" · ");
}

/** After a navigation, moves focus to the new page's heading, waiting for it if the page is
 * still downloading. The first page of a visit keeps the browser's own focus. */
function useHeadingFocus(pathname: string): void {
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return undefined;
    }
    const main = document.getElementById("main");
    if (!main) {
      return undefined;
    }
    const focusHeading = (): boolean => {
      const heading = main.querySelector("h1");
      if (!heading) {
        return false;
      }
      heading.tabIndex = -1;
      heading.focus();
      return true;
    };
    if (focusHeading()) {
      return undefined;
    }
    const observer = new MutationObserver(() => {
      if (focusHeading()) {
        observer.disconnect();
      }
    });
    observer.observe(main, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [pathname]);
}

/**
 * Knows where the current page is, keeps the browser tab's title in step, and after a
 * navigation moves focus to the new page's heading so keyboard and screen reader users land
 * on it and hear where they are.
 */
export function PageTitleProvider({ children }: { children: ReactNode }) {
  const { pathname } = useLocation();
  const [named, setNamed] = useState<Named | null>(null);
  const [, section = "", rest] = pathname.split("/");

  const trail = useMemo<Trail>(() => {
    const sectionItem = NAV_ITEMS.find((item) => item.to === `/${section}`);
    return {
      sectionItem,
      section: sectionItem?.label ?? OTHER_SECTIONS[section],
      // A name given by another page is not this page's.
      detail: named?.path === pathname ? named.name : defaultDetail(section, rest),
    };
  }, [named, pathname, section, rest]);

  useEffect(() => {
    document.title = documentTitle(trail);
  }, [trail]);
  useHeadingFocus(pathname);

  return (
    <NameContext.Provider value={setNamed}>
      <TrailContext.Provider value={trail}>{children}</TrailContext.Provider>
    </NameContext.Provider>
  );
}

/** Where the current page is, for the breadcrumb. */
export function useTrail(): Trail {
  return useContext(TrailContext);
}

/**
 * Names the one thing a page is about (an incident's title, a note's title), for the
 * breadcrumb and the browser tab. Call it from the page once the name is known; pass
 * `undefined` while it is loading. Outside the shell it does nothing.
 */
export function usePageName(name: string | undefined): void {
  const setNamed = useContext(NameContext);
  const { pathname } = useLocation();
  useEffect(() => {
    if (!setNamed || !name) {
      return undefined;
    }
    setNamed({ name, path: pathname });
    return () => setNamed(null);
  }, [setNamed, name, pathname]);
}
