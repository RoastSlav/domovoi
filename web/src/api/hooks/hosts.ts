import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Host } from "../types";

/** Every host and whether its agent is connected. */
export function useHosts() {
  return useQuery({
    queryKey: keys.hosts,
    queryFn: () => call<Host[]>(api.GET("/api/v1/hosts")),
    refetchInterval: 30_000,
  });
}

/** Host names by host id, empty until the hosts load. */
export function useHostNames(): Map<string, string> {
  const hosts = useHosts();
  return useMemo(
    () => new Map((hosts.data ?? []).map((host) => [host.id, host.name])),
    [hosts.data],
  );
}
