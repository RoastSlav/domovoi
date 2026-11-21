import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Preferences, type PresentationPrefs } from "../types";

/** The signed-in person's own display preferences. */
export function usePreferences() {
  return useQuery({
    queryKey: keys.preferences,
    queryFn: () => call<Preferences>(api.GET("/api/v1/me/preferences")),
    staleTime: 60_000,
  });
}

/**
 * Saves the display preferences and caches the hub's answer. `message` is not sent: it is the
 * caller's success toast, carried here so `useActionFeedback` can read it back.
 */
export function useSavePreferences() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ presentation }: { message?: string; presentation: PresentationPrefs }) =>
      call<Preferences>(api.PUT("/api/v1/me/preferences", { body: { presentation } })),
    onSuccess: (saved) => client.setQueryData(keys.preferences, saved),
  });
}
