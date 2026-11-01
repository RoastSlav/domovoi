import { EmptyState } from "../components/EmptyState";

/** A page that is not built yet. */
export function Placeholder({ title }: { title: string }) {
  return (
    <>
      <h1>{title}</h1>
      <EmptyState title={title === "Not found" ? "No such page" : "Coming in v0.1"} />
    </>
  );
}
