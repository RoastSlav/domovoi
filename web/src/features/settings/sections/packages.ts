import { fixed, type SectionSpec } from "./types";

/** The package section: whether this hub accepts and installs signed packages. */
export const PACKAGE_SECTION: SectionSpec = {
  id: "packages",
  title: "Packages",
  intro:
    "Turn package installation on to import owner-signed packages and install them on hosts. The owner key list and the revoked key list are file-only: edit hub.toml and restart the hub.",
  fields: fixed([
    { path: "packages.enabled", label: "Allow package installation", kind: "bool" },
    {
      path: "packages.max_artifact_bytes",
      label: "Largest package (bytes)",
      kind: "number",
      hint: "Largest archive accepted on import. 1 KiB to 256 MiB.",
    },
    {
      path: "packages.max_store_bytes",
      label: "Most bytes kept",
      kind: "number",
      hint: "Total size of the archives the hub keeps. 1 MiB to 16 GiB.",
    },
    { path: "packages.max_artifacts", label: "Most packages kept", kind: "number" },
    {
      path: "packages.job_history_days",
      label: "Keep finished installs for (days)",
      kind: "number",
      hint: "7 to 3650.",
    },
    {
      path: "packages.approval_window",
      label: "Approval window",
      kind: "text",
      hint: "How long a confirmed install waits for its host to be ready. 1 minute to 1 hour, for example 10m.",
    },
  ]),
};
