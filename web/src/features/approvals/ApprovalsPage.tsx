import { Link } from "react-router";
import { EmptyState, PageHeader, Section } from "../../components";

/**
 * Where approvals happen. Approving is done on the operation that asks, so nothing here
 * decides; the page points at the operations.
 */
export function ApprovalsPage() {
  return (
    <>
      <PageHeader
        title="Approvals"
        docs="concepts/actions.html"
        intro="Nothing runs until you approve it. Approvals are decided on the operation that asks."
      />
      <Section title="Waiting for you">
        <EmptyState icon="approve" title="Approve on the operation">
          <p>
            Each step that needs you waits on its operation, with the exact plan to approve. Open
            the <Link to="/operations">operations</Link> to see what is waiting.
          </p>
        </EmptyState>
      </Section>
    </>
  );
}
