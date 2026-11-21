import { EmptyState, Tag, Visible } from "../../components";

/**
 * Stands where the helper-key enrollment will go. The hub receives no helper keys yet, so
 * this shows nothing to compare or confirm and makes no request.
 */
export function HelperKeyPlaceholder() {
  return (
    <Visible category="P0" id="capabilities.helper-key.placeholder">
      <EmptyState icon="lock" title="Not available on this hub yet">
        <p>
          <Tag>Not available yet</Tag>
        </p>
        <p>
          Pinning a helper key means comparing the fingerprint the hub received with the one printed
          on the host. This hub does not receive helper keys yet, so there is nothing to compare or
          confirm.
        </p>
      </EmptyState>
    </Visible>
  );
}
