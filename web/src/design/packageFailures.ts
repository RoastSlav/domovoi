/**
 * What an install target's failure code means, in the owner's words. The codes come from the
 * hub and from the host's agent; a code from a newer version gets a generic sentence and the
 * raw code is never shown.
 */

const FAMILIES: [string[], string][] = [
  [["host_unavailable"], "The host was not connected when the hub tried to start."],
  [
    ["agent_incompatible"],
    "The host's agent speaks another protocol version. Upgrade the agent, then try again.",
  ],
  [
    ["not_advertised", "contract_unknown"],
    "The host's agent does not offer package installation. Turn it on in the host's agent.toml and restart the agent.",
  ],
  [
    ["artifact_missing"],
    "The package file is missing from the hub's store. Import the same package again.",
  ],
  [
    ["artifact_corrupt"],
    "The stored package no longer matches its recorded digest, so the hub did not send it. Delete it and import it again.",
  ],
  [
    ["transfer_failed", "transfer_unknown", "transfer_expired", "transfer_kind_mismatch"],
    "The package did not reach the host intact. Try again.",
  ],
  [
    ["session_changed", "handoff_failed", "hub_restarted"],
    "The install stopped before the host did anything: the connection changed, the hub could not hand it over or the hub restarted. Nothing ran. Try again.",
  ],
  [
    ["approval_expired", "authorization_expired"],
    "The approval window closed before the host was ready. Nothing ran. Start the install again.",
  ],
  [
    ["result_mismatch"],
    "The host answered, but its answer did not match what you approved. Check the host before trying again.",
  ],
  [
    ["agent_refused", "allow_risk_denied"],
    "The host's agent refused the request under its own rules.",
  ],
  [["rate_limited"], "The host's agent is limiting how many changes it accepts. Try again later."],
  [
    ["journal_corrupt"],
    "The host's agent cannot record changes right now, so it refused. Check the agent's log.",
  ],
  [["packages_disabled"], "The host has not turned on package installation."],
  [["untrusted_key"], "The host does not trust the key that signed this package."],
  [["revoked_key"], "The host has revoked the key that signed this package."],
  [
    ["bad_signature", "signature_file_invalid", "signature_invalid"],
    "The host could not verify the package signature.",
  ],
  [
    ["digest_mismatch", "identity_mismatch"],
    "What the host received does not match what was signed.",
  ],
  [
    [
      "too_large",
      "limit_entries",
      "limit_uncompressed",
      "limit_file_size",
      "limit_depth",
      "limit_path_length",
      "limit_time",
    ],
    "The package is larger, deeper or slower to unpack than the host allows.",
  ],
  [
    ["archive_invalid", "manifest_invalid", "incompatible"],
    "The package cannot be used on this host: it is damaged, its manifest is wrong or it needs another version.",
  ],
  [
    ["path_escape", "path_invalid", "link_entry", "special_entry", "duplicate_path"],
    "The archive holds an entry the host refuses, such as an unsafe path, a link or a duplicate. Nothing was extracted.",
  ],
  [
    ["no_skills", "skills_invalid", "skill_conflict", "skill_run_as_forbidden"],
    "The package's skills are not acceptable to this host.",
  ],
  [["version_conflict"], "This version conflicts with one already on the host."],
  [
    ["too_many_packages", "packages_dir_unsafe", "no_space", "store_corrupt", "storage"],
    "The host could not store the package: no room, too many packages, an unsafe directory or a damaged store.",
  ],
  [["not_run"], "Nothing was started on the host."],
];

/** Every failure code this page has words for. */
export const TARGET_FAILURE_CODES: string[] = FAMILIES.flatMap(([codes]) => codes);

const BY_CODE = new Map<string, string>(
  FAMILIES.flatMap(([codes, text]) => codes.map((code) => [code, text] as [string, string])),
);

/** The sentence for an unknown failure code. */
export const UNKNOWN_FAILURE = "The host reported a problem this page does not know.";

/** The family sentence for `code`, or the generic one. */
export function failureSentence(code: string): string {
  return BY_CODE.get(code) ?? UNKNOWN_FAILURE;
}

/** `message` as plain detail: a leading `"<code>: "` removed, empty or equal-to-title dropped. */
export function failureDetail(code: string, message: string, title: string): string | undefined {
  const prefix = `${code}: `;
  const text = (message.startsWith(prefix) ? message.slice(prefix.length) : message).trim();
  return text === "" || text === title ? undefined : text;
}
