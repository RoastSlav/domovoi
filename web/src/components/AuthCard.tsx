import { useEffect, type ReactNode } from "react";
import { BrandMark } from "./BrandMark";
import styles from "./auth.module.css";

/**
 * The frame of a page shown before there is a session: a centred 360 px card on the page
 * background (with a faint accent glow behind it), the brand tile above the page's title (its
 * only `h1`) and a sentence under it.
 * `footer` is a quiet line below the card, such as a link to the other sign-in page.
 *
 * Use it for sign-in and account setup. Do not use it for pages inside the shell, which
 * start with `PageHeader`.
 */
export function AuthCard({
  title,
  intro,
  footer,
  children,
}: {
  children: ReactNode;
  footer?: ReactNode;
  intro?: ReactNode;
  title: string;
}) {
  useEffect(() => {
    document.title = `${title} · Domovoi`;
  }, [title]);
  return (
    <main className={styles.page}>
      <div className={styles.card}>
        <div className={styles.head}>
          <BrandMark showName={false} size="lg" />
          <h1 className={styles.title}>{title}</h1>
          {intro && <p className={styles.intro}>{intro}</p>}
        </div>
        <div className={styles.body}>{children}</div>
      </div>
      {footer && <p className={styles.footer}>{footer}</p>}
    </main>
  );
}
