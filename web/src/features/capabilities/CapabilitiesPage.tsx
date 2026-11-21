import { InfoTip, PageHeader, VisibilityPage } from "../../components";
import { CapabilityTabs } from "./CapabilityTabs";

/**
 * The Capability Center: what Domovoi would offer on a host, the packages it can install and
 * the limits each host sets. Inventory and limits are read-only views; installing goes through
 * the wizard's password-and-name confirmation.
 */
export function CapabilitiesPage() {
  return (
    <VisibilityPage page="capabilities">
      <PageHeader
        title="Capabilities"
        docs="concepts/capabilities.html"
        intro={
          <>
            What this hub would offer on a host right now.{" "}
            <InfoTip title="available is not approval">
              A capability the host implements and you enable is available here; whether a
              particular request may run is decided when it is proposed, not on this page.
            </InfoTip>
          </>
        }
      />
      <CapabilityTabs />
    </VisibilityPage>
  );
}
