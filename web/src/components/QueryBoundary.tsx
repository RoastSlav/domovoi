import type { ReactNode } from "react";
import { LoadError } from "./LoadError";
import { Loading } from "./Loading";

/** The part of a TanStack query result the boundary reads. */
export interface BoundaryQuery<T> {
  data: T | undefined;
  error: unknown;
  isPending: boolean;
  refetch: () => unknown;
}

/**
 * Renders a query's four states in order, so a page cannot forget one: loading, then a
 * `LoadError` with Retry, then the empty state, then the content. Data that is already on
 * screen stays while a background refresh fails.
 *
 * Use it around everything a page loads. `context` is the verb phrase for the error
 * ("load hosts"); `loading` is a `Skeleton` in the content's shape when you have one.
 */
export function QueryBoundary<T>({
  query,
  loading,
  empty,
  isEmpty,
  context,
  children,
}: {
  children: (data: T) => ReactNode;
  context: string;
  /** What to show when `isEmpty` says there is nothing: an `EmptyState`. */
  empty?: ReactNode;
  isEmpty?: (data: T) => boolean;
  loading?: ReactNode;
  query: BoundaryQuery<T>;
}) {
  if (query.data === undefined) {
    if (query.error) {
      return <LoadError error={query.error} context={context} onRetry={query.refetch} />;
    }
    if (query.isPending) {
      return loading ? (
        <>
          <span className="visually-hidden" role="status">
            Loading…
          </span>
          {loading}
        </>
      ) : (
        <Loading />
      );
    }
    return null;
  }
  if (empty && isEmpty?.(query.data)) {
    return empty;
  }
  return children(query.data);
}
