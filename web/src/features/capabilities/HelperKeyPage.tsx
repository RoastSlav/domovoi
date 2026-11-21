import { LinkButton, PageHeader, Stack, VisibilityPage } from "../../components";
import { HelperKeyPlaceholder } from "./HelperKeyPlaceholder";

/** Where the owner will pin the helper's public key; a labelled placeholder until the hub has helper endpoints. */
export function HelperKeyPage() {
  return (
    <VisibilityPage page="capabilities">
      <PageHeader
        title="Helper key"
        intro="Pin the helper's public key after comparing fingerprints."
      />
      <Stack gap={4}>
        <HelperKeyPlaceholder />
        <div>
          <LinkButton to="/capabilities?tab=helper">Back to the helper tab</LinkButton>
        </div>
      </Stack>
    </VisibilityPage>
  );
}
