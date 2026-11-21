import { useCallback, useMemo, useState } from "react";
import type { PolicyDraft, PolicyRule } from "../../api/types";
import { diffCount, duplicateOf, toDto } from "./ruleModel";

/**
 * The working copy of the draft's rules and how it differs from the saved draft.
 *
 * The page edits `working`; nothing reaches the hub until the draft is saved. When the hub's
 * draft changes while the copy is clean the copy follows it silently; while it is dirty
 * `staleRevision` is true and the person decides whether to reload.
 * @param fetched The draft as the hub last answered.
 */
export function useDraftState(fetched: PolicyDraft) {
  const [saved, setSaved] = useState(fetched);
  const [working, setWorking] = useState<PolicyRule[]>(fetched.rules);
  const changes = useMemo(() => diffCount(saved.rules, working), [saved, working]);
  const dirty = changes > 0;
  const staleRevision = fetched.revision !== saved.revision;

  // A clean copy follows the hub at once; setting state while rendering avoids a stale frame.
  if (staleRevision && !dirty) {
    setSaved(fetched);
    setWorking(fetched.rules);
  }

  /** Puts a rule in the copy: replaces `replaces` when given and present, else appends. */
  const apply = useCallback((rule: PolicyRule, replaces?: string) => {
    setWorking((now) =>
      replaces !== undefined && now.some((r) => r.id === replaces)
        ? now.map((r) => (r.id === replaces ? rule : r))
        : [...now, rule],
    );
  }, []);

  const remove = useCallback((id: string) => {
    setWorking((now) => now.filter((r) => r.id !== id));
  }, []);

  /** Adds a shadow, unlocked copy of a rule and returns its new id. */
  const duplicate = useCallback(
    (id: string): string | undefined => {
      const source = working.find((r) => r.id === id);
      if (!source) return undefined;
      const result = toDto(
        duplicateOf(
          source,
          working.map((r) => r.id),
        ),
      );
      if (!result.ok) return undefined;
      setWorking((now) => [...now, result.rule]);
      return result.rule.id;
    },
    [working],
  );

  const reset = useCallback(() => setWorking(saved.rules), [saved]);

  /** Throws the edits away and takes the hub's draft. */
  const reload = useCallback(() => {
    setSaved(fetched);
    setWorking(fetched.rules);
  }, [fetched]);

  /** Takes a draft the hub just stored as the saved one. */
  const adopt = useCallback((draft: PolicyDraft) => {
    setSaved(draft);
    setWorking(draft.rules);
  }, []);

  return {
    adopt,
    apply,
    changes,
    dirty,
    duplicate,
    reload,
    remove,
    reset,
    saved,
    staleRevision,
    working,
  };
}
