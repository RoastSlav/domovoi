import { Link } from "react-router";

const ROUTES: Readonly<Record<string, string>> = {
  note: "/notes",
  incident: "/incidents",
  operation: "/operations",
};
const SAFE_ID = /^[A-Za-z0-9-]+$/;

/**
 * An evidence reference (`<kind>:<id>`). A note, incident or operation with a plain id is a link
 * to its page; anything else is shown as text, never built into a link.
 */
export function EvidenceRef({ evidence }: { evidence: string }) {
  const colon = evidence.indexOf(":");
  const kind = colon > 0 ? evidence.slice(0, colon) : "";
  const id = colon > 0 ? evidence.slice(colon + 1) : "";
  if (Object.hasOwn(ROUTES, kind) && SAFE_ID.test(id)) {
    return <Link to={`${ROUTES[kind]}/${id}`}>{evidence}</Link>;
  }
  return <span>{evidence}</span>;
}
