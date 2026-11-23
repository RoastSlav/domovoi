//! The production host inspection adapter.

mod unix;

use std::fs::Metadata;
use std::os::unix::fs::{FileTypeExt, MetadataExt};

use domovoi_core::identity::{PathFacts, PathKind};

pub use unix::UnixHostInspect;

/// Type, mode and owner of `metadata`, as the path table describes them.
pub(crate) fn facts_from_metadata(metadata: &Metadata) -> PathFacts {
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        Some(PathKind::Dir)
    } else if file_type.is_file() {
        Some(PathKind::File)
    } else if file_type.is_socket() {
        Some(PathKind::Socket)
    } else {
        None
    };
    PathFacts {
        kind,
        symlink: file_type.is_symlink(),
        mode: metadata.mode() & 0o7777,
        uid: metadata.uid(),
        gid: metadata.gid(),
    }
}
