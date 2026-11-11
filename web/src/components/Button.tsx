import type { ButtonHTMLAttributes, MouseEvent, ReactNode, Ref } from "react";
import { Link } from "react-router";
import { Icon, type IconName } from "../design/icons";
import { Spinner } from "./Spinner";
import { Tooltip } from "./Tooltip";
import styles from "./button.module.css";

/** How prominent a button is. */
export type ButtonVariant = "primary" | "secondary" | "danger" | "ghost";

/** What every kit button takes on top of a native button's attributes. */
export interface ButtonProps extends Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  "title" | "disabled"
> {
  /**
   * Why the button cannot be used now, and how to enable it. The button stays focusable and
   * explains itself in a tooltip; clicks do nothing.
   */
  disabledReason?: string | undefined;
  /** What the button does, when its label alone does not say; shown as a tooltip. */
  hint?: string | undefined;
  /** A leading icon. */
  icon?: IconName | undefined;
  /** The action is running: shows a spinner, keeps the width, ignores clicks. */
  loading?: boolean | undefined;
  ref?: Ref<HTMLButtonElement> | undefined;
  size?: "sm" | "md";
  variant?: ButtonVariant;
}

/**
 * A button; `type="button"` unless given, so it never submits a form by accident.
 *
 * Use `primary` for the one main action of a view, `danger` for something destructive,
 * `ghost` for quiet actions in toolbars, and `secondary` for the rest. Do not use it for
 * navigation (that is a link) or for an icon alone (that is `IconButton`).
 */
export function Button({
  variant = "secondary",
  size = "md",
  type = "button",
  icon,
  loading = false,
  disabledReason,
  hint,
  className,
  children,
  onClick,
  ...props
}: ButtonProps) {
  const blocked = loading || Boolean(disabledReason);
  const classes = [styles.button, styles[variant], styles[size], className]
    .filter(Boolean)
    .join(" ");
  const click = (event: MouseEvent<HTMLButtonElement>): void => {
    if (blocked) {
      event.preventDefault();
      return;
    }
    onClick?.(event);
  };
  const content: ReactNode = (
    <>
      <span className={styles.content}>
        {icon && <Icon name={icon} size={size === "sm" ? 14 : 16} />}
        {children}
      </span>
      {loading && (
        <span className={styles.busy}>
          <Spinner />
        </span>
      )}
    </>
  );
  const button = (
    <button
      type={type}
      className={classes}
      aria-disabled={blocked || undefined}
      aria-busy={loading || undefined}
      data-loading={loading || undefined}
      onClick={click}
      {...props}
    >
      {content}
    </button>
  );
  const tip = disabledReason ?? hint;
  // Always wrapped, so the button is not remounted (losing focus) when its reason changes.
  return <Tooltip content={tip}>{button}</Tooltip>;
}

/**
 * A link that looks like a button, for navigation that is the main action of a view ("New
 * note"). It is a link for assistive technology and the keyboard.
 *
 * Use it only when the action is going somewhere. Anything that changes data is a `Button`.
 */
export function LinkButton({
  to,
  variant = "secondary",
  size = "md",
  icon,
  children,
}: {
  children: ReactNode;
  icon?: IconName;
  size?: "sm" | "md";
  to: string;
  variant?: ButtonVariant;
}) {
  return (
    <Link to={to} className={[styles.button, styles.link, styles[variant], styles[size]].join(" ")}>
      <span className={styles.content}>
        {icon && <Icon name={icon} size={size === "sm" ? 14 : 16} />}
        {children}
      </span>
    </Link>
  );
}
