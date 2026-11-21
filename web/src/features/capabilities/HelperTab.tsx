import { LinkButton, Stack } from "../../components";
import { HelperPlaceholder } from "./HelperPlaceholder";

/** The helper tab: host-independent, because there is nothing to show yet. */
export function HelperTab() {
  return (
    <Stack gap={4}>
      <HelperPlaceholder />
      <div>
        <LinkButton to="/capabilities/helper-key">Helper key</LinkButton>
      </div>
    </Stack>
  );
}
