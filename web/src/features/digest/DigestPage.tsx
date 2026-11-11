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
  Text,
  useActionFeedback,
} from "../../components";

/**
 * Today's digest as it will be sent, and sending it now. The hub gives the digest as one
 * piece of Markdown (a line or a few per host), so it is shown whole inside a card.
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
            {(data) => (
              <article aria-label={`Digest for ${data.date}`}>
                <Card>
                  <Stack gap={3}>
                    <Text muted>{`For ${data.date}`}</Text>
                    {data.next_send_at && (
                      <Text muted size="xs" block>
                        Next scheduled send: <RelativeTime at={data.next_send_at} />
                      </Text>
                    )}
                    <Markdown text={data.markdown} breaks />
                  </Stack>
                </Card>
              </article>
            )}
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
