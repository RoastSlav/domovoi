/** The conversation the panel shows, kept per browser so it is still there after a reload. */
const KEY = "operator:active";

/** The remembered conversation id, if storage allows. */
export function readActive(): string | undefined {
  try {
    return localStorage.getItem(KEY) || undefined;
  } catch {
    return undefined;
  }
}

/** Remembers `id`, or forgets the choice when it is absent. */
export function writeActive(id: string | undefined): void {
  try {
    if (id) {
      localStorage.setItem(KEY, id);
    } else {
      localStorage.removeItem(KEY);
    }
  } catch {
    // Blocked storage: the panel then starts on a new conversation next time.
  }
}
