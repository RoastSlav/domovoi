import type { HelperView } from "../../api/types";
import {
  Banner,
  InlineMessage,
  KeyValueList,
  Stack,
  StatusPill,
  Tag,
  Text,
  Visible,
} from "../../components";
import { helperKeyStateMeta } from "../../design/tone";
import { HelperPinForm } from "./HelperPinForm";
import { HelperUnpin } from "./HelperUnpin";

/** The key the hub received, in groups of four, marked unverified until it is pinned. */
function Received({ view }: { view: HelperView }) {
  const key = view.received_key;
  if (!key) {
    return null;
  }
  const pinned = view.key_state === "pinned";
  return (
    <Stack gap={2}>
      <Text strong>Key received from the helper</Text>
      <Text mono block>
        {key.fingerprint_groups}
      </Text>
      <div>{pinned ? <Tag>Pinned by you</Tag> : <Tag tone="warning">Unverified</Tag>}</div>
    </Stack>
  );
}

/**
 * Enrolling a helper key: the received fingerprint, the instruction to read the host's own
 * printout and type it, and Pin. Nothing is pinned automatically, and the key id is only a
 * label. A key that differs from the pin blocks containment and says so first.
 */
export function HelperKeyEnroll({ view }: { view: HelperView }) {
  const state = view.key_state;
  return (
    <Visible category="P0" id="capabilities.helper-key.enroll">
      <Stack gap={4}>
        <div>
          <StatusPill meta={helperKeyStateMeta(state)} />
        </div>
        {state === "changed" && (
          <Banner tone="danger" title="Helper key changed: re-enrol">
            The helper on {view.host} now offers a different key than the one you pinned. It may
            have been reinstalled, or something else is answering. Containment on this host is
            blocked until you compare the new fingerprint on the host and pin it.
          </Banner>
        )}
        {state === "no_helper" && (
          <InlineMessage tone="info" title="No key received yet">
            The hub holds no key from a helper on {view.host}. Start the helper on the host and its
            agent will relay the key; there is nothing to pin until then.
          </InlineMessage>
        )}
        <Received view={view} />
        {state === "pinned" && view.pin && (
          <Stack gap={3}>
            <KeyValueList
              items={[
                { label: "Pinned by", value: view.pin.pinned_by },
                { label: "Pinned at", value: new Date(view.pin.pinned_at).toLocaleString() },
              ]}
            />
            <div>
              <HelperUnpin host={view.host} />
            </div>
          </Stack>
        )}
        {(state === "not_pinned" || state === "changed") && (
          <HelperPinForm host={view.host} replacing={state === "changed"} />
        )}
      </Stack>
    </Visible>
  );
}
