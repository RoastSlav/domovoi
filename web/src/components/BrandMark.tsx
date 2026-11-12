import styles from "./auth.module.css";

/**
 * The Domovoi brand: a 24 px walnut tile with a light "D", and the product name beside it
 * unless `showName` is `false` (the name then stays in the text for assistive technology).
 * `size="lg"` is a 32 px tile for the top of a card.
 *
 * Use it to frame a page that has no sidebar (sign-in). Walnut is for this mark alone. Do
 * not use it as a link or a heading.
 */
export function BrandMark({
  showName = true,
  size = "md",
}: {
  showName?: boolean;
  size?: "md" | "lg";
}) {
  return (
    <span className={styles.brand}>
      <span className={styles.mark} data-size={size} aria-hidden="true">
        D
      </span>
      {showName ? "Domovoi" : <span className="visually-hidden">Domovoi</span>}
    </span>
  );
}
