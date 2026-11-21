/** What every install is not: the install puts files on a host and enables nothing. */
export const INSTALL_NOT_ENABLE =
  "Installing puts the package's files on the host. It does not enable anything.";

/** What the hub's signature check does and does not mean. */
export const SIGNATURE_NOTE =
  "This hub checked the signature against its owner key list. Each host checks it again against the keys in its own configuration, and refuses a package it does not trust. The hub cannot add or change those keys.";

/** Why an install always asks for a password and the typed package name. */
export const HUMAN_ONLY =
  "Every install and removal needs your password and the package name typed by you, each time. Nothing installs on its own.";

/** How long each host has to be ready after the person confirms; `window` is words like "10 minutes". */
export const approvalWindow = (window: string) =>
  `After you confirm, each host has ${window} to be ready. A host that is not ready in time is skipped and nothing runs on it.`;

/** What deleting a package from the store leaves alone. */
export const DELETE_CONSEQUENCE =
  "Hosts that already have it keep it. You can import it again later.";

/** The approval window in words: seconds rounded to whole minutes ("10 minutes"). */
export function windowWords(seconds: number): string {
  const minutes = Math.max(1, Math.round(seconds / 60));
  return minutes === 1 ? "1 minute" : `${minutes} minutes`;
}
