import { Suspense, useEffect, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router";
import { useLiveEvents } from "../api/events";
import { Banner, useToast } from "../components";
import { SetupBanner } from "../features/setup/SetupBanner";
import { UpdateBanner } from "../features/updates/UpdateBanner";
import { HostProvider } from "./HostContext";
import styles from "./Layout.module.css";
import { PageSkeleton } from "./PageSkeleton";
import { PageTitleProvider } from "./PageTitle";
import { Sidebar } from "./Sidebar";
import { TopBar, TopBarSlot } from "./TopBar";
import { useSidebarRail } from "./useSidebarRail";

/** Says "Back online" once live updates return, but only if the person saw them paused. */
function useBackOnlineToast(paused: boolean): void {
  const toast = useToast();
  const wasPaused = useRef(false);
  useEffect(() => {
    if (paused) {
      wasPaused.current = true;
    } else if (wasPaused.current) {
      wasPaused.current = false;
      toast.success("Back online");
    }
  }, [paused, toast]);
}

/**
 * The frame of every signed-in page: the sidebar (a sheet on a phone), the top bar, the
 * hub-wide banners and the page. A page that is still downloading shows as a skeleton in
 * the page's place, so the frame never blinks.
 */
export function Layout() {
  const live = useLiveEvents();
  const location = useLocation();
  // Open for one location only, so navigating (or picking a host) closes the sheet by itself.
  const [openFor, setOpenFor] = useState<string | null>(null);
  const open = openFor === location.key;
  const menu = useRef<HTMLButtonElement>(null);
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  const { rail, toggle } = useSidebarRail();
  useBackOnlineToast(live.paused);

  const closeSheet = () => {
    setOpenFor(null);
    menu.current?.focus();
  };

  return (
    <HostProvider>
      <PageTitleProvider>
        <div className={styles.shell} data-rail={rail}>
          <a className={styles.skip} href="#main">
            Skip to content
          </a>
          <Sidebar
            live={live.status}
            open={open}
            onClose={closeSheet}
            rail={rail}
            onToggleRail={toggle}
          />
          <div className={styles.column}>
            <TopBar
              menu={menu}
              sheetOpen={open}
              onMenu={() => setOpenFor(open ? null : location.key)}
              slot={setSlot}
            />
            <main id="main" className={styles.main} tabIndex={-1}>
              <div className={styles.content}>
                <div className={styles.banners}>
                  {live.paused && (
                    <Banner tone="warning" title="Live updates paused">
                      Reconnecting…
                    </Banner>
                  )}
                  <UpdateBanner />
                  <SetupBanner />
                </div>
                <TopBarSlot.Provider value={slot}>
                  <Suspense fallback={<PageSkeleton />}>
                    <Outlet />
                  </Suspense>
                </TopBarSlot.Provider>
              </div>
            </main>
          </div>
        </div>
      </PageTitleProvider>
    </HostProvider>
  );
}
