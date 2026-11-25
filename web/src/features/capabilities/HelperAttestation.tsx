import type { HelperView } from "../../api/types";
import { CopyBlock, RelativeTime, Stack, StatusPill, Tag, Text, Visible } from "../../components";
import { helperFault, unverifiedReasonText } from "../../design/helperVocab";
import { attestationStateMeta } from "../../design/tone";
import { HelperVerifiedSet } from "./HelperVerifiedSet";
import {
  EDIT_NOTE,
  NOT_PROTECTED,
  RELAYED,
  hasVerifiedReport,
  shownAttestation,
} from "./helperText";

/** The local action for a token, as a sentence and the command to run on the host. */
function Fault({ token, relayed }: { relayed: boolean; token: string | null | undefined }) {
  const fault = helperFault(token);
  if (!fault) {
    return null;
  }
  return (
    <Stack gap={2}>
      <Text block>
        {relayed && <Tag>{RELAYED}</Tag>} {fault.text}
      </Text>
      {fault.command && (
        <CopyBlock label="Command to run on the host as root" text={fault.command} />
      )}
    </Stack>
  );
}

/**
 * Where the helper's attestation stands for one host: a verdict that reads "Verified" only with
 * a verified report in hand, otherwise the reason and the local fix. The agent's own hint is
 * marked as relayed and never changes the verdict.
 */
export function HelperAttestation({ view }: { view: HelperView }) {
  const { attestation } = view;
  const verified = hasVerifiedReport(view);
  return (
    <Visible category="P0" id="capabilities.helper.attestation">
      <Stack gap={3}>
        <Stack gap={2}>
          <div>
            <StatusPill meta={attestationStateMeta(shownAttestation(view))} />
          </div>
          {!verified && (
            <>
              <Text block>{unverifiedReasonText(attestation.reason)}</Text>
              <Text block muted size="sm">
                {NOT_PROTECTED}
              </Text>
              <Fault token={attestation.helper_reason} relayed={false} />
              <Fault token={attestation.unsigned_hint} relayed />
            </>
          )}
          {attestation.checked_at && (
            <Text block muted size="sm">
              Last asked <RelativeTime at={attestation.checked_at} />.
            </Text>
          )}
        </Stack>
        {verified ? (
          <HelperVerifiedSet view={view} />
        ) : (
          <Text block muted size="sm">
            {EDIT_NOTE}
          </Text>
        )}
      </Stack>
    </Visible>
  );
}
