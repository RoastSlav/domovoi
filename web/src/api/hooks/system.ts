import { useMutation, useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Advisory, type DigestPreview, type Settings } from "../types";

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
