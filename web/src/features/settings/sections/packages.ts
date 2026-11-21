import { fixed, type SectionSpec } from "./types";

/** The package section: whether this hub accepts and installs signed packages. */
export const PACKAGE_SECTION: SectionSpec = {
  id: "packages",
  title: "Packages",
  intro:
    "Turn package installation on to import owner-signed packages and install them on hosts. The owner key list and the size limits are file-only: edit hub.toml and restart the hub.",
  fields: fixed([
    { path: "packages.enabled", label: "Allow package installation", kind: "bool" },
    {
      path: "packages.approval_window",
      label: "Approval window",
      kind: "text",
      hint: "How long a confirmed install waits for its host to be ready. 1 minute to 1 hour, for example 10m.",
    },
  ]),
};
