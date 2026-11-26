import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type ContainmentBlock, type ContainmentStatus } from "../types";

/** What the hub knows about `host`'s blocks; idle without a host. Display only, and answered when containment is not available. */
export function useHostContainment(host: string | undefined) {
  return useQuery({
    queryKey: keys.hostContainment(host ?? ""),
    queryFn: () =>
      call<ContainmentStatus>(
        api.GET("/api/v1/hosts/{name}/containment", { params: { path: { name: host ?? "" } } }),
      ),
    enabled: host !== undefined,
  });
}

/** Asks `host`'s helper which blocks it holds, as the signed-in person, then shows the answer. */
export function useRefreshContainment(host: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () =>
      call<ContainmentStatus>(
        api.POST("/api/v1/hosts/{name}/containment/refresh", { params: { path: { name: host } } }),
      ),
    onSuccess: (status) => client.setQueryData(keys.hostContainment(host), status),
  });
}

/** What releasing a block early sends: the password and the source, typed exactly. */
export interface ReleaseBlockInput {
  step: string;
  password: string;
  confirmSource: string;
}

/** Takes a block away before its time runs out; asks for the password and the typed source. */
export function useReleaseBlock(operationId: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: ReleaseBlockInput) =>
      call<ContainmentBlock>(
        api.POST("/api/v1/operations/{id}/steps/{step}/containment/release", {
          params: { path: { id: operationId, step: input.step } },
          body: { password: input.password, confirm_source: input.confirmSource },
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: keys.operations }),
        client.invalidateQueries({ queryKey: ["hosts", "containment"] }),
      ]);
    },
  });
}
