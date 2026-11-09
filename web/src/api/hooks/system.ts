import { useMutation, useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type Advisory,
  type DigestPreview,
  type LogLine,
  type Settings,
} from "../types";

/** The effective configuration, with secrets shown only as set or unset. */
export function useSettings() {
  return useQuery({
    queryKey: keys.settings,
    queryFn: () => call<Settings>(api.GET("/api/v1/settings")),
  });
}

/** Advisories not yet resolved, newest first. */
export function useAdvisories() {
  return useQuery({
    queryKey: keys.advisories,
    queryFn: () => call<Advisory[]>(api.GET("/api/v1/advisories")),
  });
}

/** Today's digest as it would be sent now. */
export function useDigestPreview() {
  return useQuery({
    queryKey: keys.digest,
    queryFn: () => call<DigestPreview>(api.GET("/api/v1/digest/preview")),
  });
}

/** Sends today's digest now. */
export function useSendDigest() {
  return useMutation({
    mutationFn: () => call(api.POST("/api/v1/digest/send")),
  });
}

/** How often the Logs page asks for the lines again by itself. */
export const LOGS_REFRESH_MS = 5_000;

/**
 * The hub's own recent log lines, oldest first, at `level` or above, asked again every
 * {@link LOGS_REFRESH_MS}. Stays idle while `enabled` is false, for a source the page is not showing.
 */
export function useLogs(level: string | undefined, lines: number | undefined, enabled = true) {
  return useQuery({
    queryKey: keys.logs(level, lines),
    queryFn: () =>
      call<LogLine[]>(
        api.GET("/api/v1/system/logs", { params: { query: defined({ level, lines }) } }),
      ),
    enabled,
    refetchInterval: LOGS_REFRESH_MS,
  });
}
