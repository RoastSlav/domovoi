import { useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type HostCeiling } from "../types";

/**
 * What host `name` reported as its unattended limits. Read only: the host owns them and
 * the hub cannot change them. Idle without a host name.
 */
export function useHostCeiling(name: string | undefined) {
  return useQuery({
    queryKey: keys.hostCeiling(name ?? ""),
    queryFn: () =>
      call<HostCeiling>(
        api.GET("/api/v1/hosts/{name}/ceiling", { params: { path: { name: name ?? "" } } }),
      ),
    enabled: name !== undefined && name !== "",
    staleTime: 15_000,
  });
}
