import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useHosts } from "./hosts";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type HostPackages, type PackageSkillsAnswer } from "../types";

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

/** What the agent said when a person turned a stored package's skills on or off. */
export interface PackageSkillsRequest {
  confirmText: string;
  enabled: boolean;
  password: string;
  version: string;
}

/**
 * Turns the skills of a package stored on a host on or off. The hub asks for the password and
 * the typed package name; the agent checks the files again before it agrees. Refreshes what the
 * hub shows of the host's packages and skills afterwards.
 */
export function useSetPackageSkills(hostId: string, pkg: string) {
  const client = useQueryClient();
  const name = useHostName(hostId);
  return useMutation({
    mutationFn: (request: PackageSkillsRequest) =>
      call<PackageSkillsAnswer>(
        api.PUT("/api/v1/hosts/{name}/packages/{package}/skills", {
          params: { path: { name: name ?? "", package: pkg } },
          body: {
            enabled: request.enabled,
            version: request.version,
            password: request.password,
            confirm_text: request.confirmText,
          },
        }),
      ),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.hostPackages(hostId) });
      void client.invalidateQueries({ queryKey: ["hosts"] });
    },
  });
}
