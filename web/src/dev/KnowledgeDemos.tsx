import { useState } from "react";
import {
  ApiFailure,
  type KnowledgeFact,
  type PastOperation,
  type SummaryField,
} from "../api/types";
import { Button, Cluster, PasswordPrompt, Stack, StatusPill } from "../components";
import { describeError } from "../design/messages";
import { VOCABULARIES } from "../design/tone";
import { CoverageRow, PathsBanner } from "../features/knowledge/PathsBanner";
import { PastOperationCard } from "../features/experience/PastOperationCard";
import { SummaryFieldRow } from "../features/experience/SummaryFieldRow";
import { FactsTable } from "../features/knowledge/FactsTable";
import { WRONG_PASSWORD } from "../features/knowledge/useStepUp";
import experienceStyles from "../features/experience/experience.module.css";
import searchHost from "../test/fixtures/experience/search-host.json";
import summaryReady from "../test/fixtures/experience/summary-ready.json";
import { knowledgeFact } from "../test/knowledgeFixtures";
import { Demo, Group } from "./Demo";
import styles from "./design.module.css";

const NAMES = {
  hosts: new Map([["host-nas", "nas"]]),
  entities: new Map<string, string>(),
};

const STATUSES = ["current", "stale", "disputed", "superseded", "retracted"] as const;

const FACTS: KnowledgeFact[] = [
  ...STATUSES.map((status, index) =>
    knowledgeFact({
      id: `demo-${status}`,
      status,
      stored_status: status,
      predicate: index % 2 === 0 ? "role" : "owner_note",
      value: `A ${status} value`,
      source: index === 2 ? "inferred" : "user",
    }),
  ),
  knowledgeFact({
    id: "demo-purged",
    status: "retracted",
    stored_status: "retracted",
    content_purged: true,
    value: null,
  }),
];

const PAST = (searchHost.items[0] as unknown as { operation: PastOperation }).operation;
const FIELDS = summaryReady.fields as unknown as SummaryField[];

const PROMPT_ERRORS = [
  { name: "wrong password", text: WRONG_PASSWORD },
  {
    name: "lockout",
    text: errorWords(new ApiFailure(429, "too_many_requests", "too many attempts", 28)),
  },
  {
    name: "hub conflict",
    text: errorWords(new ApiFailure(409, "conflict", "the helper does not cover this path")),
  },
];

function errorWords(failure: ApiFailure): string {
  const described = describeError(failure);
  return [described.title, described.detail].filter(Boolean).join(" ");
}

const KNOWLEDGE_VOCABULARIES = VOCABULARIES.filter((vocabulary) =>
  ["Knowledge status", "Management path state", "Coverage"].includes(vocabulary.name),
);

/** The knowledge pages' statuses, facts table, paths banner, password prompt and experience cards. */
export function KnowledgeDemos() {
  const [asking, setAsking] = useState<string | undefined>(undefined);
  const prompt = PROMPT_ERRORS.find((error) => error.name === asking);
  return (
    <Group title="Knowledge">
      <Demo name="StatusPill (knowledge status, path state, coverage; the last of each is unknown)">
        <Stack gap={2}>
          {KNOWLEDGE_VOCABULARIES.map((vocabulary) => (
            <div key={vocabulary.name} className={styles.row}>
              <h3>{vocabulary.name}</h3>
              <Cluster>
                {[...vocabulary.values, "unknown_value"].map((value) => (
                  <StatusPill key={value} meta={vocabulary.meta(value)} />
                ))}
              </Cluster>
            </div>
          ))}
        </Stack>
      </Demo>
      <Demo name="FactsTable (every status, the last row purged)">
        <FactsTable facts={FACTS} names={NAMES} caption="Facts in every status" />
      </Demo>
      <Demo name="PathsBanner (blocked, then not blocked) and CoverageRow">
        <Stack gap={3}>
          <PathsBanner
            host="nas"
            coverageAvailable={false}
            states={["candidate", "unconfirmed", "stale"]}
          />
          <PathsBanner host="nas" coverageAvailable states={["confirmed"]} />
          <CoverageRow available={false} />
          <CoverageRow available />
        </Stack>
      </Demo>
      <Demo name="PasswordPrompt (each error)">
        <Cluster>
          {PROMPT_ERRORS.map((error) => (
            <Button key={error.name} onClick={() => setAsking(error.name)}>
              {`Open with ${error.name}`}
            </Button>
          ))}
        </Cluster>
        {prompt && (
          <PasswordPrompt
            reason="Confirming a management path tells Domovoi it is safe to rely on. Enter your password to confirm it."
            confirmLabel="Confirm"
            error={prompt.text}
            onConfirm={() => setAsking(undefined)}
            onCancel={() => setAsking(undefined)}
          />
        )}
      </Demo>
      <Demo name="PastOperationCard">
        <PastOperationCard operation={PAST} snippet="the container ran out of memory" />
      </Demo>
      <Demo name="SummaryFieldRow (code, model and owner sources)">
        <ul className={experienceStyles.fields} aria-label="Summary fields">
          {FIELDS.map((field) => (
            <SummaryFieldRow
              key={field.field}
              operationId="demo-operation"
              field={field}
              hostNames={NAMES.hosts}
              me="user:me"
              onStale={() => undefined}
            />
          ))}
        </ul>
      </Demo>
    </Group>
  );
}
