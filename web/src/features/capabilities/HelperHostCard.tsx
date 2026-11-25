import { useRecheckHelper } from "../../api/hooks/helper";
import type { HelperView } from "../../api/types";
import {
  Button,
  Card,
  Cluster,
  LinkButton,
  Stack,
  StatusPill,
  Text,
  useActionFeedback,
} from "../../components";
import { helperStateWord } from "../../design/helperVocab";
import { helperKeyStateMeta } from "../../design/tone";
import { HelperAttestation } from "./HelperAttestation";
import { RELAYED, shownAttestation } from "./helperText";

/**
 * One host's helper: the key state, the attestation, what the agent says and the Re-check
 * button. The card's stripe follows what the hub verified, never what the agent said.
 */
export function HelperHostCard({ view }: { view: HelperView }) {
  const recheck = useActionFeedback(useRecheckHelper(view.host), {
    success: "Asked the helper for a fresh report. The result appears here shortly.",
    error: "re-check the helper",
  });
  const noKey = view.key_state === "no_helper";
  return (
    <Card
      label={`Helper on ${view.host}`}
      tone={shownAttestation(view) === "verified" ? "success" : "warning"}
    >
      <Stack gap={3}>
        <Cluster justify="between">
          <Cluster>
            <Text strong>{view.host}</Text>
            <StatusPill meta={helperKeyStateMeta(view.key_state)} />
          </Cluster>
          <Cluster>
            <Button
              variant="secondary"
              size="sm"
              icon="refresh"
              loading={recheck.isPending}
              disabledReason={
                noKey ? "The hub holds no key from this host's helper yet." : undefined
              }
              onClick={() => recheck.mutate()}
            >
              Re-check
            </Button>
            <LinkButton to={`/capabilities/helper-key?host=${encodeURIComponent(view.host)}`}>
              Helper key
            </LinkButton>
          </Cluster>
        </Cluster>
        <HelperAttestation view={view} />
        <Text block muted size="sm">
          Helper, as the agent says: {helperStateWord(view.helper.state)}. {RELAYED}
        </Text>
      </Stack>
    </Card>
  );
}
