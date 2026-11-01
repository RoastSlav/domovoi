/** How often `/healthz` is asked while the hub restarts. */
export const HEALTH_POLL_MS = 500;
/** How long a restart may take before the UI gives up waiting. */
export const HEALTH_TIMEOUT_MS = 60_000;

/**
 * Waits until the hub answers `/healthz` again after applying settings. Resolves `true`
 * when it does, `false` after `timeoutMs`.
 */
export async function waitForHealthy(
  timeoutMs = HEALTH_TIMEOUT_MS,
  pollMs = HEALTH_POLL_MS,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  // The restart begins right after the answer; give it a moment to go down first.
  await new Promise((resolve) => setTimeout(resolve, pollMs));
  while (Date.now() < deadline) {
    try {
      const reply = await fetch("/healthz", { cache: "no-store" });
      if (reply.ok) {
        return true;
      }
    } catch {
      // Still restarting.
    }
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
  return false;
}
