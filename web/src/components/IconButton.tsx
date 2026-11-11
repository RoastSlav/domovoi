import { Icon, type IconName } from "../design/icons";
import { Spinner } from "./Spinner";
import { Tooltip } from "./Tooltip";
import type { ButtonProps } from "./Button";
import styles from "./button.module.css";

/**
 * A button that shows only an icon. `label` is required: it is the button's accessible name
 * and its tooltip.
 *
 * Use it for well-known actions in tight places (close, copy, more, refresh). Do not use it
 * when there is room for a word: a labelled `Button` is clearer.
 */
export function IconButton({
  icon,
  label,
  variant = "ghost",
  size = "md",
  type = "button",
  loading = false,
  disabledReason,
  className,
  onClick,
  ...props
}: Omit<ButtonProps, "children" | "hint" | "icon"> & { icon: IconName; label: string }) {
  const blocked = loading || Boolean(disabledReason);
  const classes = [styles.button, styles.iconOnly, styles[variant], styles[size], className]
    .filter(Boolean)
    .join(" ");
  return (
    <Tooltip content={disabledReason ?? label} describe={Boolean(disabledReason)}>
      <button
        type={type}
        className={classes}
        aria-label={label}
        aria-disabled={blocked || undefined}
        aria-busy={loading || undefined}
        onClick={(event) => {
          if (blocked) {
            event.preventDefault();
            return;
          }
          onClick?.(event);
        }}
        {...props}
      >
        {loading ? <Spinner /> : <Icon name={icon} size={size === "sm" ? 14 : 16} />}
      </button>
    </Tooltip>
  );
}
