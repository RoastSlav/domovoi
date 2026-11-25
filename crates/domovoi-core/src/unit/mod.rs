//! Pure unit identity: names, the loaded and on-disk properties a restart is bound to, the
//! refusal tokens, and the algorithm that turns them into one hash. All I/O sits behind two
//! read-only ports the helper implements.

mod call;
pub mod card;
mod encode;
mod identity;
pub mod keys;
mod name;
mod ports;
mod post;
mod props;
mod refusal;
mod resolve;

pub use call::{UnitCall, UnitDispatch, UnitOutputs};
pub use encode::Frames;
pub use identity::{FilePin, UnitIdentityV1, encode_loaded};
pub use name::{IdentityHash, MAX_UNIT_NAME, UnitId, UnitName, canonical_unit_identity};
pub use ports::{FileKind, FsError, SystemdView, UnitFs, ViewError};
pub use post::{PostCheck, post_restart_check};
pub use props::{
    ALLOWED_DIRS, ExecCommand, LoadedProps, ProcessView, UnitFileState, UnitPolicy, UnitProps,
    UnitRuntime,
};
pub use refusal::{UnitRefusal, UnsupportedReason};
pub use resolve::{compare_identity, compare_to_bound, resolve_identity};
