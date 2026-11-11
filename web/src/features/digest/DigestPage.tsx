import { useState } from "react";
import { useDigestPreview, useSendDigest } from "../../api/hooks/system";
import { AllHostsNote } from "../../app/AllHostsNote";
import {
  Button,
  Card,
  ConfirmDialog,
  PageHeader,
  QueryBoundary,
  RelativeTime,
  Section,
  Skeleton,
  Stack,
  Markdown,
  Tag,
  Text,
  useActionFeedback,
} from "../../components";

/**
 * Today's digest as it will be sent, and sending it now. Each host is its own card when the
 * hub gives the per-host parts, and the whole message follows in one card.
 */
export function DigestPage() {
  const preview = useDigestPreview();
  const send = useActionFeedback(useSendDigest(), {
    success: "Digest sent to Discord",
    error: "send the digest",
  });
  const [confirming, setConfirming] = useState(false);

  return (
    <>
      <PageHeader
        title="Digest"
        topic="the digest"
        docs="concepts/digest.html"
        intro="One summary of every host per day: what changed, trends and open incidents."
        actions={
          <Button
            variant="primary"
            icon="digest"
            hint="Sends today’s digest to Discord now, as well as at its usual time."
            loading={send.isPending}
            disabledReason={preview.data ? undefined : "The digest has not loaded yet."}
            onClick={() => setConfirming(true)}
          >
            Send now
          </Button>
        }
      />
      <AllHostsNote>The digest is one summary of every host.</AllHostsNote>
      <Stack gap={5}>
        <Section title="Preview" description="This is what Discord gets if you send it now.">
          <QueryBoundary
            query={preview}
            context="build the digest"
            loading={<Skeleton shape="block" />}
          >
            {(data) => {
              const hosts = data.hosts ?? [];
              return (
                <article aria-label={`Digest for ${data.date}`}>
                  <Stack gap={3}>
                    <Text muted>{`For ${data.date}`}</Text>
                    {data.next_send_at && (
                      <Text muted size="xs" block>
                        Next scheduled send: <RelativeTime at={data.next_send_at} />
                      </Text>
                    )}
                    {hosts.length > 0
                      ? hosts.map((host) => (
                          <Card key={host.host}>
                            <Stack gap={2}>
                              <Text strong>{host.host}</Text>
                              {host.quiet ? (
                                <Tag tone="neutral">Nothing changed</Tag>
                              ) : (
                                <Markdown
                                  text={host.lines.map((line) => `- ${line}`).join("\n")}
                                  breaks
                                />
                              )}
                            </Stack>
                          </Card>
                        ))
                      : null}
                    {/* The hosts above are a reading aid; this is the message itself, whole. */}
                    <Card>
                      <Stack gap={2}>
                        {hosts.length > 0 && <Text muted>The whole message, as it is sent</Text>}
                        <Markdown text={data.markdown} breaks />
                      </Stack>
                    </Card>
                  </Stack>
                </article>
              );
            }}
          </QueryBoundary>
        </Section>
        {/* A "Cost" section (what this month's digests spent on the model) goes here. */}
      </Stack>
      {confirming && (
        <ConfirmDialog
          title="Send the digest now?"
          consequence="It goes to Discord now, as well as at its usual time."
          confirmLabel="Send digest"
          busy={send.isPending}
          onConfirm={() => {
            // The result arrives as a toast, which needs the dialog out of the way.
            setConfirming(false);
            send.mutate(undefined);
          }}
          onCancel={() => setConfirming(false)}
        />
      )}
    </>
  );
}
