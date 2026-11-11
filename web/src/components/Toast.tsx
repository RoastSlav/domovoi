import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Icon } from "../design/icons";
import type { DescribedError } from "../design/messages";
import { Button } from "./Button";
import { ErrorDetail } from "./ErrorDetail";
import { IconButton } from "./IconButton";
import { TONE_ICONS } from "./InlineMessage";
import styles from "./message.module.css";

/** How long a success or info toast stays. */
export const TOAST_MS = 4000;
/** The most toasts on screen at once; older ones leave first. */
export const MAX_TOASTS = 3;

/** One button on a toast: Undo, Retry, View. */
export interface ToastAction {
  label: string;
  onClick: () => void;
}

/** What a toast says. */
export interface ToastInput {
  action?: ToastAction | undefined;
  detail?: ReactNode;
  title: string;
}

interface ToastItem extends ToastInput {
  id: number;
  tone: "success" | "info" | "danger";
}

/** How page code raises toasts. Each call returns the toast's id, for `dismiss`. */
export interface ToastApi {
  dismiss: (id: number) => void;
  /** Stays until dismissed. Pass a `DescribedError` from `describeError`. */
  error: (toast: ToastInput | DescribedError) => number;
  info: (toast: ToastInput | string) => number;
  success: (toast: ToastInput | string) => number;
}

const ToastContext = createContext<ToastApi | null>(null);

function Toast({ toast, onDismiss }: { onDismiss: () => void; toast: ToastItem }) {
  const [paused, setPaused] = useState(false);
  const sticky = toast.tone === "danger";
  useEffect(() => {
    if (sticky || paused) {
      return undefined;
    }
    const timer = setTimeout(onDismiss, TOAST_MS);
    return () => clearTimeout(timer);
  }, [sticky, paused, onDismiss]);
  const action = toast.action;
  return (
    <div
      className={styles.toast}
      data-tone={toast.tone}
      role={sticky ? "alert" : undefined}
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onDismiss();
        }
      }}
    >
      <span className={styles.icon}>
        <Icon name={TONE_ICONS[toast.tone]} />
      </span>
      <div className={styles.text}>
        <p className={styles.title}>{toast.title}</p>
        {toast.detail && <div className={styles.body}>{toast.detail}</div>}
      </div>
      <div className={styles.action}>
        {action && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              action.onClick();
              onDismiss();
            }}
          >
            {action.label}
          </Button>
        )}
        <IconButton icon="close" label="Dismiss" size="sm" onClick={onDismiss} />
      </div>
    </div>
  );
}

function ToastSlot({ toast, dismiss }: { dismiss: (id: number) => void; toast: ToastItem }) {
  const onDismiss = useCallback(() => dismiss(toast.id), [dismiss, toast.id]);
  return <Toast toast={toast} onDismiss={onDismiss} />;
}

/**
 * Holds the toasts and draws them: bottom right on wide screens, bottom centre on phones,
 * newest on top, at most three. Wrap the app in it once.
 */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const next = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const api = useMemo<ToastApi>(() => {
    const push = (tone: ToastItem["tone"], input: ToastInput): number => {
      const id = next.current++;
      setToasts((current) => [{ ...input, id, tone }, ...current].slice(0, MAX_TOASTS));
      return id;
    };
    const plain = (input: ToastInput | string): ToastInput =>
      typeof input === "string" ? { title: input } : input;
    return {
      dismiss,
      success: (input) => push("success", plain(input)),
      info: (input) => push("info", plain(input)),
      error: (input) =>
        push(
          "danger",
          "retryAfter" in input || "issues" in input
            ? { title: input.title, detail: <ErrorDetail error={input} /> }
            : input,
        ),
    };
  }, [dismiss]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className={styles.viewport} role="log" aria-label="Notifications" aria-live="polite">
        {toasts.map((toast) => (
          <ToastSlot key={toast.id} toast={toast} dismiss={dismiss} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}

/**
 * Raises toasts: `success` and `info` leave after a few seconds, `error` stays until
 * dismissed. For the result of a mutation use `useActionFeedback`, which calls this for you.
 */
export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (!api) {
    throw new Error("useToast needs a ToastProvider above it");
  }
  return api;
}
