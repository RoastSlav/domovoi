//! The stable reason codes at the start of every package failure message.

use serde::{Deserialize, Serialize};

macro_rules! failure_codes {
    ($($variant:ident => $text:literal),+ $(,)?) => {
        /// Why a package transfer, install or removal was refused.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum PackageFailureCode {
            $(
                #[doc = $text]
                $variant,
            )+
        }

        impl PackageFailureCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [PackageFailureCode] = &[$(PackageFailureCode::$variant),+];

            /// The wire text of this code.
            #[must_use]
            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(PackageFailureCode::$variant => $text,)+
                }
            }
        }

        impl std::str::FromStr for PackageFailureCode {
            type Err = crate::error::ValidationError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok(PackageFailureCode::$variant),)+
                    _ => Err(crate::error::ValidationError::new("package failure code", "unknown code")),
                }
            }
        }
    };
}

failure_codes! {
    PackagesDisabled => "packages_disabled",
    TransferUnknown => "transfer_unknown",
    TransferExpired => "transfer_expired",
    TransferKindMismatch => "transfer_kind_mismatch",
    SignatureFileInvalid => "signature_file_invalid",
    UntrustedKey => "untrusted_key",
    RevokedKey => "revoked_key",
    BadSignature => "bad_signature",
    DigestMismatch => "digest_mismatch",
    IdentityMismatch => "identity_mismatch",
    TooLarge => "too_large",
    ArchiveInvalid => "archive_invalid",
    ManifestInvalid => "manifest_invalid",
    Incompatible => "incompatible",
    PathEscape => "path_escape",
    PathInvalid => "path_invalid",
    LinkEntry => "link_entry",
    SpecialEntry => "special_entry",
    DuplicatePath => "duplicate_path",
    LimitEntries => "limit_entries",
    LimitUncompressed => "limit_uncompressed",
    LimitFileSize => "limit_file_size",
    LimitDepth => "limit_depth",
    LimitPathLength => "limit_path_length",
    LimitTime => "limit_time",
    NoSkills => "no_skills",
    SkillsInvalid => "skills_invalid",
    SkillConflict => "skill_conflict",
    SkillRunAsForbidden => "skill_run_as_forbidden",
    VersionConflict => "version_conflict",
    TooManyPackages => "too_many_packages",
    PackagesDirUnsafe => "packages_dir_unsafe",
    NoSpace => "no_space",
    StoreCorrupt => "store_corrupt",
    Storage => "storage",
    TreeChanged => "tree_changed",
    NotInstalled => "not_installed",
}

impl std::fmt::Display for PackageFailureCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
