import { Skeleton } from "../components";
import styles from "./Layout.module.css";
import sidebar from "./sidebar.module.css";
import topbar from "./topbar.module.css";
import { useSidebarRail } from "./useSidebarRail";

/**
 * The shape of a page that is on its way: the page header (a title and an intro line) and
 * some rows. Shown in the page's place while its code downloads, so the frame stays still.
 */
export function PageSkeleton() {
  return (
    <div className={styles.pageSkeleton} aria-busy="true">
      <p className="visually-hidden" role="status">
        Loading the page…
      </p>
      <div className={styles.headerSkeleton}>
        <span className={styles.titleSkeleton}>
          <Skeleton shape="line" count={1} />
        </span>
        <span className={styles.introSkeleton}>
          <Skeleton shape="line" count={1} />
        </span>
      </div>
      <Skeleton shape="rows" count={6} />
    </div>
  );
}

/**
 * The whole frame as a skeleton, for the moment before the app knows who is signed in. It
 * has the sidebar's and the top bar's shape, so nothing jumps when the real ones arrive.
 */
export function ShellSkeleton() {
  const { rail } = useSidebarRail();
  return (
    <div className={styles.shell} data-rail={rail}>
      <div className={sidebar.sidebar} aria-hidden="true" />
      <div className={styles.column}>
        <div className={topbar.topbar} aria-hidden="true">
          <span className={topbar.ghostBar}>
            <Skeleton shape="line" count={1} />
          </span>
        </div>
        <main className={styles.main}>
          <div className={styles.content}>
            <PageSkeleton />
          </div>
        </main>
      </div>
    </div>
  );
}
