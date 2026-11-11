import { Icon, type IconName } from "../design/icons";
import { useTheme, type Theme } from "../design/theme";
import styles from "./ThemeSwitch.module.css";

const OPTIONS: readonly (readonly [theme: Theme, label: string, icon: IconName])[] = [
  ["system", "System", "monitor"],
  ["light", "Light", "sun"],
  ["dark", "Dark", "moon"],
];

/** Picks the colour theme: follow the device, or always light or dark. */
export function ThemeSwitch() {
  const { theme, setTheme } = useTheme();
  return (
    <fieldset className={styles.group}>
      <legend className="visually-hidden">Theme</legend>
      {OPTIONS.map(([value, label, icon]) => (
        <label key={value} className={styles.option} data-checked={theme === value}>
          <input
            type="radio"
            name="theme"
            className="visually-hidden"
            checked={theme === value}
            onChange={() => setTheme(value)}
          />
          <Icon name={icon} size={14} />
          {label}
        </label>
      ))}
    </fieldset>
  );
}
