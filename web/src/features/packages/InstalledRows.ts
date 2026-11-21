import type { HostPackage } from "../../api/types";

/** The wizard address that removes `pkg` from `host`, with everything preselected. */
export function removeHref(host: string, pkg: Pick<HostPackage, "name" | "version">): string {
  const query = new URLSearchParams({
    kind: "uninstall",
    package: pkg.name,
    version: pkg.version,
    host,
  });
  return `/capabilities/install?${query.toString()}`;
}

/** What a host's agent says about installing, when it said no: the sentence for the info note. */
export function installOffText(host: string): string {
  return `${host}'s agent has not turned on package installation, so an install there would not run.`;
}
