//! [`HostInspect`] over std and `nix`: the one adapter that looks at a real host.

use std::ffi::CString;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use domovoi_core::identity::{GroupEntry, PathFacts, UserEntry};
use domovoi_core::ports::{Access, HostInspect, InspectError, Probe, TreeEntry};
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::unistd::{Gid, Group, Uid, User};

use super::facts_from_metadata;

/// The most entries a tree walk returns before it gives up, so a hostile tree cannot exhaust memory.
const MAX_TREE_ENTRIES: usize = 200_000;
/// The most bytes `read_text` returns.
const MAX_TEXT_BYTES: u64 = 1 << 20;
/// Where the running kernel's release string lives.
const KERNEL_RELEASE_FILE: &str = "/proc/sys/kernel/osrelease";

/// Reads accounts, file metadata and kernel facts from the running host.
///
/// It performs no change except the short-lived probe files described on [`Probe`] and the
/// timing files of `state_write_timings`, each removed before the call returns.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnixHostInspect;

impl UnixHostInspect {
    /// An adapter over the running host.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

fn map_io(err: &std::io::Error) -> InspectError {
    match err.kind() {
        std::io::ErrorKind::NotFound => InspectError::NotFound,
        std::io::ErrorKind::PermissionDenied => InspectError::PermissionDenied,
        _ => InspectError::Io(err.to_string()),
    }
}

fn map_errno(err: Errno) -> InspectError {
    InspectError::Io(err.to_string())
}

fn access_of(err: &std::io::Error) -> Access {
    match err.raw_os_error().map(Errno::from_raw) {
        Some(Errno::ENOENT) => Access::NotFound,
        Some(Errno::EACCES | Errno::EPERM) => Access::Denied,
        Some(Errno::EROFS) => Access::ReadOnly,
        _ => Access::Other,
    }
}

fn is_errno(err: &std::io::Error, wanted: Errno) -> bool {
    err.raw_os_error() == Some(wanted as i32)
}

fn c_name(name: &str) -> Result<CString, InspectError> {
    CString::new(name).map_err(|_| InspectError::Io("name contains a NUL byte".to_owned()))
}

impl HostInspect for UnixHostInspect {
    fn euid(&self) -> u32 {
        Uid::effective().as_raw()
    }

    fn egid(&self) -> u32 {
        Gid::effective().as_raw()
    }

    fn supplementary_groups(&self) -> Result<Vec<u32>, InspectError> {
        let groups = nix::unistd::getgroups().map_err(map_errno)?;
        Ok(groups.into_iter().map(Gid::as_raw).collect())
    }

    fn user(&self, name: &str) -> Result<Option<UserEntry>, InspectError> {
        let found = User::from_name(name).map_err(map_errno)?;
        Ok(found.map(|user| UserEntry {
            name: user.name,
            uid: user.uid.as_raw(),
            gid: user.gid.as_raw(),
            shell: user.shell.to_string_lossy().into_owned(),
        }))
    }

    fn group(&self, name: &str) -> Result<Option<GroupEntry>, InspectError> {
        let found = Group::from_name(name).map_err(map_errno)?;
        Ok(found.map(|group| GroupEntry {
            name: group.name,
            gid: group.gid.as_raw(),
            members: group.mem,
        }))
    }

    fn groups_of(&self, user: &UserEntry) -> Result<Vec<u32>, InspectError> {
        let name = c_name(&user.name)?;
        let groups =
            nix::unistd::getgrouplist(&name, Gid::from_raw(user.gid)).map_err(map_errno)?;
        Ok(groups.into_iter().map(Gid::as_raw).collect())
    }

    fn lstat(&self, path: &Path) -> Result<PathFacts, InspectError> {
        std::fs::symlink_metadata(path)
            .map(|m| facts_from_metadata(&m))
            .map_err(|e| map_io(&e))
    }

    fn open_facts(&self, path: &Path) -> Result<PathFacts, InspectError> {
        let flags = (OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK).bits();
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(path);
        match opened {
            Ok(file) => file
                .metadata()
                .map(|m| facts_from_metadata(&m))
                .map_err(|e| map_io(&e)),
            // A symlink or a socket node cannot be opened this way; its own facts say why.
            Err(e) if is_errno(&e, Errno::ELOOP) || is_errno(&e, Errno::ENXIO) => self.lstat(path),
            Err(e) => Err(map_io(&e)),
        }
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<String>, InspectError> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path).map_err(|e| map_io(&e))? {
            let entry = entry.map_err(|e| map_io(&e))?;
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    fn walk_tree(&self, path: &Path) -> Result<Vec<TreeEntry>, InspectError> {
        let mut out = Vec::new();
        let mut pending: Vec<PathBuf> = vec![path.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).map_err(|e| map_io(&e))? {
                let entry = entry.map_err(|e| map_io(&e))?;
                let metadata = std::fs::symlink_metadata(entry.path()).map_err(|e| map_io(&e))?;
                let facts = facts_from_metadata(&metadata);
                if metadata.file_type().is_dir() {
                    pending.push(entry.path());
                }
                out.push(TreeEntry {
                    path: entry.path(),
                    facts,
                });
                if out.len() > MAX_TREE_ENTRIES {
                    return Err(InspectError::Io("tree has too many entries".to_owned()));
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn probe(&self, path: &Path, how: Probe) -> Access {
        match how {
            Probe::Stat => result_access(std::fs::metadata(path).map(drop)),
            Probe::Open => result_access(std::fs::File::open(path).map(drop)),
            Probe::List => result_access(std::fs::read_dir(path).map(drop)),
            Probe::Connect => {
                result_access(std::os::unix::net::UnixStream::connect(path).map(drop))
            }
            Probe::Append => result_access(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .map(drop),
            ),
            Probe::Create | Probe::Rename | Probe::Unlink => probe_directory_write(path, how),
        }
    }

    fn read_text(&self, path: &Path) -> Result<String, InspectError> {
        let file = std::fs::File::open(path).map_err(|e| map_io(&e))?;
        let mut text = String::new();
        file.take(MAX_TEXT_BYTES)
            .read_to_string(&mut text)
            .map_err(|e| map_io(&e))?;
        Ok(text)
    }

    fn kernel_release(&self) -> Result<String, InspectError> {
        let text = self.read_text(Path::new(KERNEL_RELEASE_FILE))?;
        Ok(text.trim().to_owned())
    }

    fn state_write_timings(&self, dir: &Path, cycles: u32) -> Result<Vec<u64>, InspectError> {
        let pid = std::process::id();
        let temp = dir.join(format!(".domovoi-timing-{pid}.tmp"));
        let target = dir.join(format!(".domovoi-timing-{pid}"));
        let result = time_cycles(dir, &temp, &target, cycles.max(1));
        let _ = std::fs::remove_file(&temp);
        let _ = std::fs::remove_file(&target);
        result.map_err(|e| map_io(&e))
    }
}

fn result_access(result: std::io::Result<()>) -> Access {
    match result {
        Ok(()) => Access::Allowed,
        Err(e) => access_of(&e),
    }
}

/// `Create`, `Rename` and `Unlink` all need a file the probe made itself; if making it fails, that
/// error is the answer (a read-only mount says `EROFS` here), and what was made is removed.
fn probe_directory_write(dir: &Path, how: Probe) -> Access {
    let pid = std::process::id();
    let first = dir.join(format!(".domovoi-probe-{pid}"));
    let second = dir.join(format!(".domovoi-probe-{pid}.moved"));
    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&first);
    if let Err(e) = created {
        return access_of(&e);
    }
    let answer = match how {
        Probe::Rename => result_access(std::fs::rename(&first, &second)),
        Probe::Unlink => result_access(std::fs::remove_file(&first)),
        _ => Access::Allowed,
    };
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
    answer
}

fn time_cycles(dir: &Path, temp: &Path, target: &Path, cycles: u32) -> std::io::Result<Vec<u64>> {
    use std::io::Write;
    let block = [0x5a_u8; 4096];
    let mut micros = Vec::new();
    for _ in 0..cycles {
        let started = Instant::now();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(temp)?;
        file.write_all(&block)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(temp, target)?;
        std::fs::File::open(dir)?.sync_all()?;
        micros.push(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
    }
    Ok(micros)
}
