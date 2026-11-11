import { Link } from "react-router";
import { EmptyState, PageHeader } from "../components";

/** What unknown paths show. */
export function Placeholder({ title }: { title: string }) {
  return (
    <>
      <PageHeader title={title} />
      <EmptyState title="No such page" action={<Link to="/incidents">Go to incidents</Link>}>
        <p>The address may be wrong, or the page may have moved.</p>
      </EmptyState>
    </>
  );
}
