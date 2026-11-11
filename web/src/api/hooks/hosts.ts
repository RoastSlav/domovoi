import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { api } from "../client";
import { keys } from "../queryClient";
import { LOGS_REFRESH_MS } from "./system";
import {
  call,
  defined,
  type Host,
  type HostDetail,
  type Invite,
  type LogLine,
  type Skill,
} from "../types";

/** How often the host list refreshes on its own. */
export const HOSTS_REFRESH_MS = 30_000;

/** How often the host list refreshes while an enrollment waits for its agent. */
export const ENROLL_POLL_MS = 2_000;

/** Every host and whether its agent is connected; polls every `pollMs`. */
export function useHosts(pollMs: number = HOSTS_REFRESH_MS) {
  return useQuery({
    queryKey: keys.hosts,
    queryFn: () => call<Host[]>(api.GET("/api/v1/hosts")),
    refetchInterval: pollMs,
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

/** One host with its latest digest facts; refreshes as often as the host list, so its
 * connection state does not go stale. */
export function useHost(name: string) {
  return useQuery({
    queryKey: keys.host(name),
    queryFn: () =>
      call<HostDetail>(api.GET("/api/v1/hosts/{name}", { params: { path: { name } } })),
    refetchInterval: HOSTS_REFRESH_MS,
  });
}

/** The skills host `name`'s agent reported; idle while `name` is empty. */
export function useSkills(name: string) {
  return useQuery({
    queryKey: keys.skills(name),
    queryFn: () =>
      call<Skill[]>(api.GET("/api/v1/hosts/{name}/skills", { params: { path: { name } } })),
    enabled: name !== "",
  });
}

/**
 * The recent log lines host `name`'s agent keeps, oldest first. Asks again every
 * {@link LOGS_REFRESH_MS} while `refresh` is true; with it false the page only
 * asks when the person clicks. Stays idle while `name` is empty, or while `enabled`
 * is false for a source the page is not showing.
 */
export function useHostLogs(
  name: string,
  level: string,
  lines: number,
  enabled = true,
  refresh = true,
) {
  return useQuery({
    queryKey: keys.hostLogs(name, level, lines),
    queryFn: () =>
      call<LogLine[]>(
        api.GET("/api/v1/hosts/{name}/logs", {
          params: { path: { name }, query: defined({ level, lines }) },
        }),
      ),
    enabled: name !== "" && enabled,
    refetchInterval: refresh ? LOGS_REFRESH_MS : false,
  });
}

/** Turns host `name`'s skill on or off; the host's skill list refreshes once it is saved. */
export function useSetSkillEnabled(name: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ enabled, skill }: { enabled: boolean; skill: string }) =>
      call(
        api.PUT("/api/v1/hosts/{name}/skills/{skill}", {
          params: { path: { name, skill } },
          body: { enabled },
        }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.skills(name) }),
  });
}

/** Creates a one-time enrollment invite, creating the host if it is new. */
export function useCreateInvite() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (name: string) =>
      call<Invite>(api.POST("/api/v1/hosts/{name}/enrollment", { params: { path: { name } } })),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.hosts }),
  });
}

/** Revokes a host's agent and disconnects it. */
export function useRevoke() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (name: string) =>
      call<{ revoked: boolean }>(
        api.POST("/api/v1/hosts/{name}/revoke", { params: { path: { name } } }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.hosts }),
  });
}
