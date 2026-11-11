import { StatusPill, ThemeSwitch } from "../components";
import { ICON_NAMES, Icon } from "../design/icons";
import { TONES } from "../design/tone";
import { LayoutDemos, StateDemos } from "./ContentDemos";
import { FormDemos } from "./FormDemos";
import { ControlDemos, OverlayDemos } from "./ControlDemos";
import { FeedbackDemos, StatusDemos } from "./StatusDemos";
import tokensCss from "../styles/tokens.css?raw";
import styles from "./design.module.css";

/** Every token name with `prefix`, in the order tokens.css declares them, without repeats. */
function tokens(prefix: string): string[] {
  const names = [...tokensCss.matchAll(/^\s*(--[\w-]+):/gm)].map((match) => match[1] ?? "");
  return [...new Set(names)].filter((name) => name.startsWith(prefix));
}

function Swatches({ prefix }: { prefix: string }) {
  return (
    <ul className={styles.swatches}>
      {tokens(prefix).map((name) => (
        <li key={name}>
          <span className={styles.swatch} style={{ background: `var(${name})` }} />
          <code>{name}</code>
        </li>
      ))}
    </ul>
  );
}

const TYPE_STEPS = ["xs", "sm", "base", "lg", "xl", "2xl"];

/**
 * The living catalogue: every token, tone, status, icon and kit component as it renders now,
 * grouped like web/DESIGN.md. Served only by
 * the dev server at /dev/design; check new work against it in both themes.
 */
export default function DesignPage() {
  return (
    <main className={styles.page}>
      <header className={styles.head}>
        <h1>Design catalogue</h1>
        <div className={styles.theme}>
          <ThemeSwitch />
        </div>
      </header>

      <section>
        <h2>Semantic colours</h2>
        <Swatches prefix="--color-" />
      </section>

      <section>
        <h2>Tones</h2>
        <table className={styles.table}>
          <thead>
            <tr>
              <th scope="col">Tone</th>
              <th scope="col">Pill</th>
              <th scope="col">On the page</th>
              <th scope="col">Solid (counts only)</th>
            </tr>
          </thead>
          <tbody>
            {TONES.map((tone) => (
              <tr key={tone}>
                <th scope="row">{tone}</th>
                <td>
                  <StatusPill
                    meta={{ label: tone, tone, hint: `The ${tone} tone.`, icon: "dot" }}
                  />
                </td>
                <td style={{ color: `var(--tone-${tone}-fg)` }}>Text in {tone}</td>
                <td>
                  <span
                    className={styles.count}
                    style={{
                      background: `var(--tone-${tone}-solid)`,
                      color: `var(--tone-${tone}-on-solid)`,
                    }}
                  >
                    12
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <StatusDemos />
      <FeedbackDemos />
      <ControlDemos />
      <StateDemos />
      <LayoutDemos />
      <FormDemos />
      <OverlayDemos />

      <section>
        <h2>Type scale</h2>
        {TYPE_STEPS.map((step) => (
          <p
            key={step}
            className={styles.type}
            style={{ fontSize: `var(--text-${step})`, lineHeight: `var(--leading-${step})` }}
          >
            <code>--text-{step}</code> The agent on nas-01 reported 3 failed checks at 02:14.
          </p>
        ))}
        <p className={styles.mono}>Mono: systemctl restart nginx.service · 0O1lI · 1234567890</p>
      </section>

      <section>
        <h2>Icons</h2>
        <ul className={styles.icons}>
          {ICON_NAMES.map((name) => (
            <li key={name}>
              <Icon name={name} size={20} />
              <Icon name={name} size={16} />
              <Icon name={name} size={14} />
              <code>{name}</code>
            </li>
          ))}
        </ul>
      </section>

      <section>
        <h2>Palette primitives</h2>
        <p className={styles.note}>For reference only: components use the semantic tokens.</p>
        <Swatches prefix="--palette-" />
      </section>
    </main>
  );
}
