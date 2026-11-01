import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Session } from "../types";

/** The current session; fails with a 401 when logged out. */
export function useSession() {
  return useQuery({
    queryKey: keys.session,
    queryFn: () => call<Session>(api.GET("/api/v1/session")),
    staleTime: 60_000,
  });
}

/** Logs in and caches the new session, CSRF token included. */
export function useLogin() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { username: string; password: string }) =>
      call<Session>(api.POST("/api/v1/auth/login", { body })),
    onSuccess: (session) => client.setQueryData(keys.session, session),
  });
}

/** Creates the admin account with the setup token and caches the new session. */
export function useSetup() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { token: string; username: string; password: string }) =>
      call<Session>(api.POST("/api/v1/auth/setup", { body })),
    onSuccess: (session) => client.setQueryData(keys.session, session),
  });
}

/** Ends the session and forgets everything cached for it. */
export function useLogout() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => call(api.POST("/api/v1/auth/logout")),
    onSettled: () => client.clear(),
  });
}
