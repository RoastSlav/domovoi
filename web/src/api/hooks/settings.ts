import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type HostChecks, type HostDocker, type Saved, type TestResult } from "../types";

/** What happens to a secret on save: keep it, set a new value, or remove it. */
export type SecretChange = { keep: true } | { set: string } | { clear: true };

/** A settings change, shaped like `hub.toml`; `null` removes a value. */
export type Changes = Record<string, unknown>;

/** A save: the revision it was made on, the changes, and the password when needed. */
export interface SaveRequest {
  base_revision: number;
  changes: Changes;
  password?: string;
}

/** Saves and applies a settings change. */
export function useSaveSettings() {
  return useMutation({
    mutationFn: (request: SaveRequest) =>
      call<Saved>(
        api.PATCH("/api/v1/settings", {
          body: { ...request, changes: request.changes as Record<string, never> },
        }),
      ),
  });
}

/** Which connection a test tries. */
export type TestTarget = "discord" | "llm" | "heartbeat";

/** Tries a connection with candidate values; nothing is saved. */
export function useTestConnection(target: TestTarget) {
  return useMutation({
    mutationFn: (changes: Changes) => {
      const body = { changes: changes as Record<string, never> };
      switch (target) {
        case "discord":
          return call<TestResult>(api.POST("/api/v1/settings/test/discord", { body }));
        case "llm":
          return call<TestResult>(api.POST("/api/v1/settings/test/llm", { body }));
        case "heartbeat":
          return call<TestResult>(api.POST("/api/v1/settings/test/heartbeat", { body }));
      }
    },
  });
}

/** Lists models for candidate provider settings without saving them. */
export function useModelDiscovery() {
  return useMutation({
    mutationFn: async (changes: Changes) => {
      const result = await call<{ models: string[] }>(
        api.POST("/api/v1/settings/models", {
          body: { changes: changes as Record<string, never> },
        }),
      );
      return result.models;
    },
  });
}

/** The JSON schema of `hub.toml`, for field help. */
export function useSettingsSchema() {
  return useQuery({
    queryKey: keys.settingsSchema,
    queryFn: () => call<Record<string, unknown>>(api.GET("/api/v1/settings/schema")),
    staleTime: Infinity,
  });
}

/** A host's checks and the hub's overrides. */
export function useHostChecks(name: string) {
  return useQuery({
    queryKey: keys.hostChecks(name),
    queryFn: () =>
      call<HostChecks>(api.GET("/api/v1/hosts/{name}/checks", { params: { path: { name } } })),
  });
}

/** A host's container runtime: what its agent found and the hub's choice. */
export function useHostDocker(name: string) {
  return useQuery({
    queryKey: keys.hostDocker(name),
    queryFn: () =>
      call<HostDocker>(api.GET("/api/v1/hosts/{name}/docker", { params: { path: { name } } })),
  });
}

/** Switches a host's container runtime on or off, or `null` to leave it to the agent. */
export function useSetHostDocker(name: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean | null) =>
      call<{ delivered: boolean }>(
        api.PUT("/api/v1/hosts/{name}/docker", {
          params: { path: { name } },
          body: { enabled },
        }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.hostDocker(name) }),
  });
}

/** Overrides as saved for a host: enabled, interval and thresholds per check key. */
export type Overrides = Record<
  string,
  {
    enabled?: boolean | null;
    interval_seconds?: number | null;
    thresholds?: Record<string, unknown>;
  }
>;

/** Replaces a host's check overrides; the answer says whether the agent got them now. */
export function useSaveHostChecks(name: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (overrides: Overrides) =>
      call<{ delivered: boolean }>(
        api.PUT("/api/v1/hosts/{name}/checks", {
          params: { path: { name } },
          body: { overrides: overrides as never },
        }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.hostChecks(name) }),
  });
}
