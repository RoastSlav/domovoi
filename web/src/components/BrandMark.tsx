import styles from "./auth.module.css";

/**
 * The Domovoi brand: the square mark with the product name beside it, as at the top of the
 * sidebar.
 *
 * Use it to frame a page that has no sidebar (sign-in). Do not use it as a link or a heading.
 */
export function BrandMark() {
  return (
    <span className={styles.brand}>
      <span className={styles.mark} aria-hidden="true">
        D
      </span>
      Domovoi
    </span>
  );
}
