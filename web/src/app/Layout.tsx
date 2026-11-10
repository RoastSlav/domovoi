import { Outlet } from "react-router";
import { useLiveEvents } from "../api/events";
import { SetupBanner } from "../features/setup/SetupBanner";
import { UpdateBanner } from "../features/updates/UpdateBanner";
import { HostProvider } from "./HostContext";
import styles from "./Layout.module.css";
import { Sidebar } from "./Sidebar";

/** The frame of every logged-in page: sidebar (a top bar on narrow screens) and the page. */
export function Layout() {
  const live = useLiveEvents();
  return (
    <HostProvider>
      <div className={styles.shell}>
        <a className={styles.skip} href="#main">
          Skip to content
        </a>
        <Sidebar live={live} />
        <main id="main" className={styles.main} tabIndex={-1}>
          <UpdateBanner />
          <SetupBanner />
          <Outlet />
        </main>
      </div>
    </HostProvider>
  );
}
