import type { ReactNode } from "react";
import styles from "./design.module.css";

/** One group of the catalogue, named like its section in web/DESIGN.md. */
export function Group({ title, children }: { children: ReactNode; title: string }) {
  return (
    <section>
      <h2>{title}</h2>
      {children}
    </section>
  );
}

/** One component in the catalogue: its name, then its variants and states. */
export function Demo({ name, children }: { children: ReactNode; name: string }) {
  return (
    <div className={styles.demo}>
      <h3>{name}</h3>
      <div className={styles.demoBody}>{children}</div>
    </div>
  );
}
