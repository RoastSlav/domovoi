//! Pinning and unpinning a helper key, with their audit rows. The caller has already checked
//! the session, the password and the typed fingerprint; nothing here pins on its own.

use domovoi_core::attest::PinnedKey;
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{HelperTrustWrite, PinWrite, StateWrite, StoreError, Transaction};
use domovoi_core::{Actor, HostId};
use serde_json::json;

use super::HelperTrustHub;
use super::accept::audit;
use super::request::store_error;

impl HelperTrustHub {
    /// Pins `public_key` for `host`: one commit that stores the pin, deletes the high-water
    /// row of that host and key, clears the latest outcome and verified state, and writes
    /// the audit rows (`helper.key_pinned`, or `helper.key_rotated` when another key was
    /// pinned, and `helper.high_water_reset` when a row was deleted).
    ///
    /// # Errors
    /// A store failure.
    pub(crate) async fn pin_key(
        &self,
        actor: Actor,
        host: HostId,
        public_key: [u8; 32],
    ) -> Result<PinnedKey, StoreError> {
        let stored = self.repo.stored(host).await?;
        let fingerprint = KeyFingerprint::of_public_key(&public_key);
        let at = self.clock.now();
        let pinned_by = actor.to_string();
        let mut tx =
            Transaction::new().write(StateWrite::HelperTrust(HelperTrustWrite::Pin(PinWrite {
                host,
                public_key,
                fingerprint,
                pinned_at: at,
                pinned_by: pinned_by.clone(),
            })));
        match &stored.view.pin {
            Some(old) if old.fingerprint != fingerprint => {
                tx = tx.audit(audit(
                    self,
                    actor.clone(),
                    "helper.key_rotated",
                    host,
                    json!({
                        "old_fingerprint": old.fingerprint.to_string(),
                        "new_fingerprint": fingerprint.to_string(),
                    }),
                ));
            }
            _ => {
                tx = tx.audit(audit(
                    self,
                    actor.clone(),
                    "helper.key_pinned",
                    host,
                    json!({"fingerprint": fingerprint.to_string()}),
                ));
            }
        }
        if self.repo.high_water(host, &fingerprint).await?.is_some() {
            tx = tx.audit(audit(
                self,
                actor,
                "helper.high_water_reset",
                host,
                json!({"fingerprint": fingerprint.to_string()}),
            ));
        }
        self.uow.commit(tx).await.map_err(store_error)?;
        self.note_attempt(host, None, None);
        Ok(PinnedKey::new(public_key, at, pinned_by))
    }

    /// Removes the pin and the verified state of `host`; the high-water rows stay. Does
    /// nothing, and writes no audit row, when no key is pinned.
    ///
    /// # Errors
    /// A store failure.
    pub(crate) async fn unpin_key(&self, actor: Actor, host: HostId) -> Result<(), StoreError> {
        let stored = self.repo.stored(host).await?;
        let Some(pin) = stored.view.pin else {
            return Ok(());
        };
        let tx = Transaction::new()
            .write(StateWrite::HelperTrust(HelperTrustWrite::Unpin { host }))
            .audit(audit(
                self,
                actor,
                "helper.key_unpinned",
                host,
                json!({"fingerprint": pin.fingerprint.to_string()}),
            ));
        self.uow.commit(tx).await.map(|_| ()).map_err(store_error)
    }
}
