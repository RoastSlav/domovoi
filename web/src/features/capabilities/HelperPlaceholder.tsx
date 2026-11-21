import { EmptyState, Tag, Visible } from "../../components";

/**
 * Stands where the helper view will go. The hub collects no helper status, safety coverage or
 * helper keys yet, so this shows no data and makes no request. Not for showing anything about a
 * host's helper.
 */
export function HelperPlaceholder() {
  return (
    <Visible category="P0" id="capabilities.helper.placeholder">
      <EmptyState icon="lock" title="Not available on this hub yet">
        <p>
          <Tag>Not available yet</Tag>
        </p>
        <p>
          This hub does not collect helper status, safety coverage or helper keys yet, so there is
          nothing to show here and no data is made up. This is a statement about the hub, not about
          your hosts.
        </p>
      </EmptyState>
    </Visible>
  );
}
