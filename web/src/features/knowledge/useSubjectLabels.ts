import { useMemo } from "react";
import { useHostNames } from "../../api/hooks/hosts";
import { useEntities } from "../../api/hooks/knowledge";
import type { SubjectNames } from "./subjects";

/**
 * The host and entity names a page needs to write subjects in words. Both lists are shared
 * queries, so a page that labels many subjects makes one request for each.
 */
export function useSubjectLabels(): SubjectNames {
  const hosts = useHostNames();
  const entities = useEntities();
  return useMemo(
    () => ({
      hosts,
      entities: new Map((entities.data ?? []).map((entity) => [entity.id, entity.name])),
    }),
    [hosts, entities.data],
  );
}
