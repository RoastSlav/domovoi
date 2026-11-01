import { EmptyState } from "../components/EmptyState";

/** What unknown paths show. */
export function Placeholder({ title }: { title: string }) {
  return (
    <>
      <h1>{title}</h1>
      <EmptyState title="No such page" />
    </>
  );
}
