import { EmptyState, LinkButton, PageHeader } from "../components";
import { usePageName } from "./PageTitle";

/** What an address the app does not know shows, inside the shell so the way on is in reach. */
export function NotFoundPage() {
  usePageName("Page not found");
  return (
    <>
      <PageHeader title="Page not found" />
      <EmptyState
        title="No such page"
        action={
          <LinkButton to="/incidents" variant="primary">
            Go to incidents
          </LinkButton>
        }
      >
        <p>The address may be wrong, or the page may have moved.</p>
      </EmptyState>
    </>
  );
}
