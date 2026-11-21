/** The error codes the package endpoints add, and what each says. */

/** Every error code the packages endpoints return. */
export const PACKAGE_HUB_CODES = [
  "packages_disabled",
  "package_conflict",
  "package_in_use",
  "store_full",
  "unknown_key",
  "revoked_key",
  "bad_signature",
  "digest_mismatch",
  "signature_file_invalid",
  "not_zstd",
  "too_large",
  "upload_failed",
  "plan_changed",
  "host_busy",
  "no_such_package",
  "no_such_job",
] as const;

/** One package error code in words: the reason, and whether the hub's own text adds to it. */
export const PACKAGE_CATALOGUE: Record<
  (typeof PACKAGE_HUB_CODES)[number],
  { reason: string; specific: boolean }
> = {
  packages_disabled: {
    reason:
      "Package installation is turned off on this hub. Turn it on in Settings, Packages, then try again.",
    specific: false,
  },
  package_conflict: {
    reason: "That does not fit what the hub has stored for this package or install.",
    specific: true,
  },
  package_in_use: {
    reason: "A running install still needs this package. Wait for it to finish, then delete it.",
    specific: false,
  },
  store_full: {
    reason: "The package store is full. Delete a package you no longer need, then import again.",
    specific: false,
  },
  unknown_key: {
    reason:
      "The package is signed with a key that is not in this hub's owner key list. Add the key to hub.toml, restart the hub and import again.",
    specific: true,
  },
  revoked_key: {
    reason: "The key that signed this package is on the revoked list, so the hub refuses it.",
    specific: true,
  },
  bad_signature: {
    reason:
      "The signature does not match the archive. The file may be damaged, or it was not signed by that key.",
    specific: false,
  },
  digest_mismatch: {
    reason: "The archive does not match the digest in its signature file. Get both files again.",
    specific: false,
  },
  signature_file_invalid: {
    reason: "The signature file is not in the expected format.",
    specific: false,
  },
  not_zstd: { reason: "The archive is not a .tar.zst file.", specific: false },
  too_large: { reason: "The file is larger than this hub accepts.", specific: false },
  upload_failed: {
    reason: "The upload could not be read. Choose the files again and try once more.",
    specific: false,
  },
  plan_changed: {
    reason: "What you reviewed has changed since. Review the plan again.",
    specific: false,
  },
  host_busy: {
    reason: "Another install is still running on one of these hosts. Wait for it, then try again.",
    specific: true,
  },
  no_such_package: {
    reason: "That package is not in the store. Import it first.",
    specific: false,
  },
  no_such_job: { reason: "That install no longer exists.", specific: false },
};
