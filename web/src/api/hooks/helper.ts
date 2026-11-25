import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type HelperView } from "../types";

/** What the hub knows about `host`'s helper; idle without a host. Display only. */
export function useHostHelper(host: string | undefined) {
  return useQuery({
    queryKey: keys.hostHelper(host ?? ""),
    queryFn: () =>
      call<HelperView>(
        api.GET("/api/v1/hosts/{name}/helper", { params: { path: { name: host ?? "" } } }),
      ),
    enabled: host !== undefined,
  });
}

/** Asks `host`'s helper for a fresh attestation, then reads the helper view again. */
export function useRecheckHelper(host: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () =>
      call<unknown>(
        api.POST("/api/v1/hosts/{name}/helper/recheck", { params: { path: { name: host } } }),
      ),
    onSettled: () => client.invalidateQueries({ queryKey: keys.hostHelper(host) }),
  });
}

/** Pins the key the hub holds from `host`'s helper: the full fingerprint typed from the host, and the password. */
export function usePinHelperKey(host: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (request: { fingerprint: string; password: string }) =>
      call<HelperView>(
        api.PUT("/api/v1/hosts/{name}/helper/key", {
          params: { path: { name: host } },
          body: request,
        }),
      ),
    onSuccess: (view) => client.setQueryData(keys.hostHelper(host), view),
  });
}

/** Removes the pin of `host`'s helper key; asks for the password. */
export function useUnpinHelperKey(host: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (request: { password: string }) =>
      call<HelperView>(
        api.DELETE("/api/v1/hosts/{name}/helper/key", {
          params: { path: { name: host } },
          body: request,
        }),
      ),
    onSuccess: (view) => client.setQueryData(keys.hostHelper(host), view),
  });
}
