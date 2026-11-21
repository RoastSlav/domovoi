import { InlineMessage, LinkButton } from "../../components";
import type { PackagesStatus } from "../../api/types";

/**
 * Says whether this hub accepts packages: off is information, not a fault, and a missing owner
 * key means nothing could be imported. Not for errors loading the status; the tab shows those.
 */
export function PackagesStatusNote({ status }: { status: PackagesStatus }) {
  if (!status.enabled) {
    return (
      <InlineMessage
        tone="info"
        title="Package installation is off"
        action={<LinkButton to="/settings/packages">Open Packages settings</LinkButton>}
      >
        This hub does not accept or install packages yet. Turn it on in Settings, Packages (it asks
        for your password), then import a package.
      </InlineMessage>
    );
  }
  if (status.owner_keys.length === 0) {
    return (
      <InlineMessage tone="warning" title="No owner key is set">
        No owner key is set, so no package can be imported. Add a [[packages.owner_keys]] entry to
        hub.toml and restart the hub.
      </InlineMessage>
    );
  }
  return null;
}
