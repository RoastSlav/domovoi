import type { PackagesStatus } from "../../api/types";
import { Stack, Text } from "../../components";

/**
 * The hub's owner keys and revoked key ids as plain text. There is no control here: each host
 * decides for itself which keys it trusts, and the hub cannot change them.
 */
export function OwnerKeys({ status }: { status: PackagesStatus }) {
  return (
    <Stack gap={2}>
      <Text muted size="sm" block>
        These keys come from hub.toml. This hub uses them only to refuse files nobody could install.
        Each host decides for itself which keys it trusts, and the hub cannot change them.
      </Text>
      <Text strong block>
        Owner keys
      </Text>
      {status.owner_keys.length === 0 ? (
        <Text muted block>
          None set.
        </Text>
      ) : (
        <ul aria-label="Owner keys">
          {status.owner_keys.map((key) => (
            <li key={key.id}>
              {key.label ? `${key.label}: ` : "Key with no label: "}
              <Text mono>{key.id}</Text>
            </li>
          ))}
        </ul>
      )}
      <Text strong block>
        Revoked keys
      </Text>
      {status.revoked.length === 0 ? (
        <Text muted block>
          None.
        </Text>
      ) : (
        <ul aria-label="Revoked keys">
          {status.revoked.map((id) => (
            <li key={id}>
              <Text mono>{id}</Text>
            </li>
          ))}
        </ul>
      )}
    </Stack>
  );
}
