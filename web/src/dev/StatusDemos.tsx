import {
  Banner,
  Button,
  Cluster,
  Count,
  InlineMessage,
  LoadError,
  Stack,
  StatusPill,
  Tag,
  toneStripe,
  useToast,
} from "../components";
import { ApiFailure } from "../api/types";
import { describeError } from "../design/messages";
import { TONES, VOCABULARIES } from "../design/tone";
import { Demo, Group } from "./Demo";
import styles from "./design.module.css";

/** Status display: pills for every status, tags, counts and the tone stripe. */
export function StatusDemos() {
  return (
    <Group title="Status">
      <Demo name="StatusPill (every vocabulary; the last of each is an unknown value)">
        <Stack gap={2}>
          {VOCABULARIES.map((vocabulary) => (
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
      <Demo name="StatusPill sizes and dot">
        <Cluster>
          {TONES.map((tone) => (
            <StatusPill
              key={tone}
              size="sm"
              showIcon={false}
              meta={{ label: tone, tone, hint: `The ${tone} tone.`, icon: "dot" }}
            />
          ))}
        </Cluster>
      </Demo>
      <Demo name="Tag">
        <Cluster>
          <Tag>container agent</Tag>
          <Tag>hub-managed</Tag>
          {TONES.map((tone) => (
            <Tag key={tone} tone={tone}>
              {tone}
            </Tag>
          ))}
        </Cluster>
      </Demo>
      <Demo name="Count">
        <Cluster>
          {TONES.map((tone) => (
            <Count key={tone} tone={tone} value={12} label={`in ${tone}`} />
          ))}
        </Cluster>
      </Demo>
      <Demo name="toneStripe">
        <Stack gap={2}>
          {TONES.map((tone) => (
            <div key={tone} className={styles.striped} {...toneStripe(tone)}>
              A row with a {tone} stripe
            </div>
          ))}
        </Stack>
      </Demo>
    </Group>
  );
}

const SAMPLE_ERRORS: [string, unknown][] = [
  ["conflict", new ApiFailure(409, "conflict", "the approval window closed 2 minutes ago")],
  ["too_many_requests", new ApiFailure(429, "too_many_requests", "too many attempts", 28)],
  [
    "invalid",
    new ApiFailure(400, "invalid", "the settings are not valid", undefined, [
      { path: "web.port", message: "Port must be between 1 and 65535.", severity: "error" },
    ]),
  ],
  ["network", new TypeError("Failed to fetch")],
];

/** Messages and feedback: inline messages, banners, toasts and load errors. */
export function FeedbackDemos() {
  const toast = useToast();
  return (
    <Group title="Messaging">
      <Demo name="InlineMessage">
        <Stack gap={2}>
          {TONES.map((tone) => (
            <InlineMessage
              key={tone}
              tone={tone}
              title={`A ${tone} message`}
              action={tone === "warning" ? <Button size="sm">Reload</Button> : undefined}
            >
              One sentence that says what happened, then what to do.
            </InlineMessage>
          ))}
        </Stack>
      </Demo>
      <Demo name="Banner">
        <Stack gap={2}>
          <Banner tone="progress" title="Live updates paused. Reconnecting…" />
          <Banner
            tone="info"
            title="Domovoi 0.1.0 is available"
            action={<Button size="sm">How to update</Button>}
            onDismiss={() => undefined}
          >
            You run 0.0.8.
          </Banner>
          <Banner tone="danger" solid title="The hub cannot reach its database" />
        </Stack>
      </Demo>
      <Demo name="Toasts (useToast, useActionFeedback)">
        <Cluster>
          <Button onClick={() => toast.success("Incident acknowledged")}>Success</Button>
          <Button
            onClick={() =>
              toast.success({
                title: "Incident muted for 1 hour",
                action: { label: "Undo", onClick: () => toast.info("Incident unmuted") },
              })
            }
          >
            Success with Undo
          </Button>
          <Button onClick={() => toast.info("Nothing changed")}>Info</Button>
          {SAMPLE_ERRORS.map(([name, error]) => (
            <Button
              key={name}
              variant="danger"
              onClick={() => toast.error(describeError(error, "approve the action"))}
            >
              Error: {name}
            </Button>
          ))}
        </Cluster>
      </Demo>
      <Demo name="LoadError (describeError for each kind of failure)">
        <Stack gap={2}>
          {SAMPLE_ERRORS.map(([name, error]) => (
            <LoadError key={name} error={error} context="load hosts" onRetry={() => undefined} />
          ))}
        </Stack>
      </Demo>
    </Group>
  );
}
