import { useQuery } from "@tanstack/react-query";
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
