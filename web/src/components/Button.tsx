import type { ButtonHTMLAttributes } from "react";
import styles from "./ui.module.css";

/** How prominent a button is. */
export type ButtonVariant = "primary" | "secondary" | "danger";

/** A button; `type="button"` unless given, so it never submits a form by accident. */
export function Button({
  variant = "secondary",
  type = "button",
  className,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  const classes = [styles.button, styles[variant], className].filter(Boolean).join(" ");
  return <button type={type} className={classes} {...props} />;
}
