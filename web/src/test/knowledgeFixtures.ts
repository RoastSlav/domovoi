import type {
  KnowledgeEntity,
  KnowledgeFact,
  KnowledgeFactPage,
  KnowledgeRelation,
  KnowledgeRelationPage,
  ManagementPath,
  ManagementPaths,
} from "../api/types";
import entitiesJson from "./fixtures/knowledge/entities.json";
import factPurgedJson from "./fixtures/knowledge/fact-purged.json";
import factRetractedJson from "./fixtures/knowledge/fact-retracted.json";
import factsPageJson from "./fixtures/knowledge/facts-page.json";
import pathsAvailableJson from "./fixtures/knowledge/paths-available.json";
import pathsUnavailableJson from "./fixtures/knowledge/paths-unavailable.json";
import relationsPageJson from "./fixtures/knowledge/relations-page.json";

/** A live fact the owner entered about the `nas` host, overridable. */
export function knowledgeFact(overrides: Partial<KnowledgeFact> = {}): KnowledgeFact {
  return {
    id: "fact-1",
    subject: "host:host-nas",
    predicate: "role",
    value: "media server",
    source: "user",
    status: "current",
    stored_status: "current",
    version: 1,
    observed_at: "2026-10-31T12:00:00Z",
    valid_until: null,
    last_verified_at: null,
    fresh_until: "2026-11-30T12:00:00Z",
    supersedes: null,
    evidence: null,
    actor: "user:me",
    created_at: "2026-10-31T12:00:00Z",
    retracted_at: null,
    content_purged: false,
    ...overrides,
  };
}

/** An unconfirmed management path of `nas` the owner entered, overridable. */
export function managementPath(overrides: Partial<ManagementPath> = {}): ManagementPath {
  return {
    fact: knowledgeFact({
      id: "path-1",
      predicate: "management_path",
      value: { cidr: "10.8.0.0/24" },
    }),
    state: "unconfirmed",
    confirmation: null,
    ...overrides,
  };
}

/** A live relation "plex runs on nas", overridable. */
export function knowledgeRelation(overrides: Partial<KnowledgeRelation> = {}): KnowledgeRelation {
  return {
    id: "rel-1",
    from: "container:host-nas/plex",
    kind: "runs_on",
    to: "host:host-nas",
    source: "user",
    status: "current",
    stored_status: "current",
    version: 1,
    observed_at: "2026-10-31T12:00:00Z",
    evidence: null,
    actor: "user:me",
    created_at: "2026-10-31T12:00:00Z",
    retracted_at: null,
    content_purged: false,
    ...overrides,
  };
}

/** An entity the owner named, overridable. */
export function knowledgeEntity(overrides: Partial<KnowledgeEntity> = {}): KnowledgeEntity {
  return {
    id: "ent-1",
    kind: "vpn",
    name: "Home VPN",
    created_by: "user:me",
    created_at: "2026-10-30T12:00:00Z",
    version: 1,
    ...overrides,
  };
}

/** Seven live facts: a role, two proposals, a stale note, a disputed pair and a found path. */
export const FACTS_PAGE = factsPageJson as unknown as KnowledgeFactPage;
/** A retracted fact whose value is still shown. */
export const FACT_RETRACTED = factRetractedJson as unknown as KnowledgeFact;
/** A retracted fact whose content was purged. */
export const FACT_PURGED = factPurgedJson as unknown as KnowledgeFact;
/** Paths of `nas` while the helper's coverage cannot be verified. */
export const PATHS_UNAVAILABLE = pathsUnavailableJson as unknown as ManagementPaths;
/** Paths of `nas` when coverage can be checked. */
export const PATHS_AVAILABLE = pathsAvailableJson as unknown as ManagementPaths;
/** Two relations. */
export const RELATIONS_PAGE = relationsPageJson as unknown as KnowledgeRelationPage;
/** Two entities. */
export const ENTITIES = entitiesJson as unknown as KnowledgeEntity[];
