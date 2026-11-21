import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useHosts } from "./hosts";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type HostPackages } from "../types";

/** The host name for `hostId`, from the cached host list. */
function useHostName(hostId: string | undefined): string | undefined {
  return useHosts().data?.find((host) => host.id === hostId)?.name;
}

/** What a host last reported it has installed. Display only; idle without a host. */
export function useHostPackages(hostId: string | undefined) {
  const name = useHostName(hostId);
  return useQuery({
    queryKey: keys.hostPackages(hostId ?? ""),
    queryFn: () =>
      call<HostPackages>(
        api.GET("/api/v1/hosts/{name}/packages", { params: { path: { name: name ?? "" } } }),
      ),
    enabled: hostId !== undefined && name !== undefined,
  });
}

/** Asks the host's agent what is installed now and replaces the cached list with the answer. */
export function useRefreshHostPackages(hostId: string) {
  const client = useQueryClient();
  const name = useHostName(hostId);
  return useMutation({
    mutationFn: () =>
      call<HostPackages>(
        api.POST("/api/v1/hosts/{name}/packages/refresh", {
          params: { path: { name: name ?? "" } },
        }),
      ),
    onSuccess: (answer) => client.setQueryData(keys.hostPackages(hostId), answer),
  });
}
