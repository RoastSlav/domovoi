//! A host inspector that answers from a script and records every call.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use domovoi_core::identity::{GroupEntry, PathFacts, UserEntry};
use domovoi_core::ports::{Access, HostInspect, InspectError, Probe, TreeEntry};

use crate::lock;

/// One call made on a [`FakeHostInspect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectCall {
    /// `supplementary_groups()`.
    SupplementaryGroups,
    /// `user(name)`.
    User(String),
    /// `group(name)`.
    Group(String),
    /// `groups_of(user)`, by account name.
    GroupsOf(String),
    /// `lstat(path)`.
    Lstat(PathBuf),
    /// `open_facts(path)`.
    OpenFacts(PathBuf),
    /// `list_dir(path)`.
    ListDir(PathBuf),
    /// `walk_tree(path)`.
    WalkTree(PathBuf),
    /// `probe(path, how)`.
    Probe(PathBuf, Probe),
    /// `read_text(path)`.
    ReadText(PathBuf),
    /// `kernel_release()`.
    KernelRelease,
    /// `state_write_timings(dir, cycles)`.
    StateWriteTimings(PathBuf, u32),
}

/// A [`HostInspect`] built from a script: accounts, groups, per-path facts and access results,
/// text files and timings. Anything not scripted is [`InspectError::NotFound`] (for probes,
/// [`Access::NotFound`]). Every call is recorded; there is no way to change the "host", which
/// mirrors the port.
#[derive(Debug, Default)]
pub struct FakeHostInspect {
    euid: u32,
    egid: u32,
    groups: Option<Result<Vec<u32>, InspectError>>,
    users: BTreeMap<String, UserEntry>,
    user_groups: BTreeMap<String, Vec<u32>>,
    group_entries: BTreeMap<String, GroupEntry>,
    lstats: BTreeMap<PathBuf, Result<PathFacts, InspectError>>,
    opens: BTreeMap<PathBuf, Result<PathFacts, InspectError>>,
    listings: BTreeMap<PathBuf, Vec<String>>,
    trees: BTreeMap<PathBuf, Vec<TreeEntry>>,
    access: BTreeMap<(PathBuf, Probe), Access>,
    path_access: BTreeMap<PathBuf, Access>,
    texts: BTreeMap<PathBuf, String>,
    timings: Option<Result<Vec<u64>, InspectError>>,
    kernel: Option<String>,
    calls: Mutex<Vec<InspectCall>>,
}

impl FakeHostInspect {
    /// A host with euid and egid 0 and nothing else.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the effective uid and gid of the "process".
    #[must_use]
    pub fn ids(mut self, euid: u32, egid: u32) -> Self {
        self.euid = euid;
        self.egid = egid;
        self
    }

    /// Sets `getgroups()`.
    #[must_use]
    pub fn groups(mut self, groups: &[u32]) -> Self {
        self.groups = Some(Ok(groups.to_vec()));
        self
    }

    /// Makes `getgroups()` fail.
    #[must_use]
    pub fn groups_error(mut self, err: InspectError) -> Self {
        self.groups = Some(Err(err));
        self
    }

    /// Adds an account whose `getgrouplist` is `groups_of`.
    #[must_use]
    pub fn user(mut self, entry: UserEntry, groups_of: &[u32]) -> Self {
        self.user_groups
            .insert(entry.name.clone(), groups_of.to_vec());
        self.users.insert(entry.name.clone(), entry);
        self
    }

    /// Adds a group entry.
    #[must_use]
    pub fn group(mut self, entry: GroupEntry) -> Self {
        self.group_entries.insert(entry.name.clone(), entry);
        self
    }

    /// Sets what `lstat` and `open_facts` both return for `path`.
    #[must_use]
    pub fn path(mut self, path: impl Into<PathBuf>, facts: PathFacts) -> Self {
        let path = path.into();
        self.lstats.insert(path.clone(), Ok(facts.clone()));
        self.opens.insert(path, Ok(facts));
        self
    }

    /// Sets what `lstat` returns for `path`, leaving `open_facts` as it was.
    #[must_use]
    pub fn lstat_result(
        mut self,
        path: impl Into<PathBuf>,
        result: Result<PathFacts, InspectError>,
    ) -> Self {
        self.lstats.insert(path.into(), result);
        self
    }

    /// Sets what `open_facts` returns for `path`, such as a lock that cannot be opened.
    #[must_use]
    pub fn open_result(
        mut self,
        path: impl Into<PathBuf>,
        result: Result<PathFacts, InspectError>,
    ) -> Self {
        self.opens.insert(path.into(), result);
        self
    }

    /// Sets the names `list_dir` returns for `path`.
    #[must_use]
    pub fn dir(mut self, path: impl Into<PathBuf>, names: &[&str]) -> Self {
        self.listings
            .insert(path.into(), names.iter().map(|n| (*n).to_owned()).collect());
        self
    }

    /// Sets the entries `walk_tree` returns for `path`.
    #[must_use]
    pub fn tree(mut self, path: impl Into<PathBuf>, entries: Vec<TreeEntry>) -> Self {
        self.trees.insert(path.into(), entries);
        self
    }

    /// Sets the result of `how` on `path`.
    #[must_use]
    pub fn access(mut self, path: impl Into<PathBuf>, how: Probe, access: Access) -> Self {
        self.access.insert((path.into(), how), access);
        self
    }

    /// Sets the result of every probe on `path` that has no more specific entry.
    #[must_use]
    pub fn any_access(mut self, path: impl Into<PathBuf>, access: Access) -> Self {
        self.path_access.insert(path.into(), access);
        self
    }

    /// Sets the text `read_text` returns for `path`.
    #[must_use]
    pub fn text(mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> Self {
        self.texts.insert(path.into(), text.into());
        self
    }

    /// Sets the microsecond timings `state_write_timings` returns.
    #[must_use]
    pub fn timings(mut self, micros: &[u64]) -> Self {
        self.timings = Some(Ok(micros.to_vec()));
        self
    }

    /// Makes `state_write_timings` fail.
    #[must_use]
    pub fn timings_error(mut self, err: InspectError) -> Self {
        self.timings = Some(Err(err));
        self
    }

    /// Sets the kernel release string.
    #[must_use]
    pub fn kernel_release_text(mut self, release: impl Into<String>) -> Self {
        self.kernel = Some(release.into());
        self
    }

    /// Every call so far, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<InspectCall> {
        lock(&self.calls).clone()
    }

    fn record(&self, call: InspectCall) {
        lock(&self.calls).push(call);
    }
}

fn answer<T: Clone>(
    map: &BTreeMap<PathBuf, Result<T, InspectError>>,
    path: &Path,
) -> Result<T, InspectError> {
    map.get(path)
        .cloned()
        .unwrap_or(Err(InspectError::NotFound))
}

impl HostInspect for FakeHostInspect {
    fn euid(&self) -> u32 {
        self.euid
    }

    fn egid(&self) -> u32 {
        self.egid
    }

    fn supplementary_groups(&self) -> Result<Vec<u32>, InspectError> {
        self.record(InspectCall::SupplementaryGroups);
        self.groups.clone().unwrap_or_else(|| Ok(Vec::new()))
    }

    fn user(&self, name: &str) -> Result<Option<UserEntry>, InspectError> {
        self.record(InspectCall::User(name.to_owned()));
        Ok(self.users.get(name).cloned())
    }

    fn group(&self, name: &str) -> Result<Option<GroupEntry>, InspectError> {
        self.record(InspectCall::Group(name.to_owned()));
        Ok(self.group_entries.get(name).cloned())
    }

    fn groups_of(&self, user: &UserEntry) -> Result<Vec<u32>, InspectError> {
        self.record(InspectCall::GroupsOf(user.name.clone()));
        Ok(self
            .user_groups
            .get(&user.name)
            .cloned()
            .unwrap_or_else(|| vec![user.gid]))
    }

    fn lstat(&self, path: &Path) -> Result<PathFacts, InspectError> {
        self.record(InspectCall::Lstat(path.to_path_buf()));
        answer(&self.lstats, path)
    }

    fn open_facts(&self, path: &Path) -> Result<PathFacts, InspectError> {
        self.record(InspectCall::OpenFacts(path.to_path_buf()));
        answer(&self.opens, path)
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<String>, InspectError> {
        self.record(InspectCall::ListDir(path.to_path_buf()));
        self.listings
            .get(path)
            .cloned()
            .ok_or(InspectError::NotFound)
    }

    fn walk_tree(&self, path: &Path) -> Result<Vec<TreeEntry>, InspectError> {
        self.record(InspectCall::WalkTree(path.to_path_buf()));
        self.trees.get(path).cloned().ok_or(InspectError::NotFound)
    }

    fn probe(&self, path: &Path, how: Probe) -> Access {
        self.record(InspectCall::Probe(path.to_path_buf(), how));
        self.access
            .get(&(path.to_path_buf(), how))
            .or_else(|| self.path_access.get(path))
            .copied()
            .unwrap_or(Access::NotFound)
    }

    fn read_text(&self, path: &Path) -> Result<String, InspectError> {
        self.record(InspectCall::ReadText(path.to_path_buf()));
        self.texts.get(path).cloned().ok_or(InspectError::NotFound)
    }

    fn kernel_release(&self) -> Result<String, InspectError> {
        self.record(InspectCall::KernelRelease);
        self.kernel.clone().ok_or(InspectError::NotFound)
    }

    fn state_write_timings(&self, dir: &Path, cycles: u32) -> Result<Vec<u64>, InspectError> {
        self.record(InspectCall::StateWriteTimings(dir.to_path_buf(), cycles));
        self.timings.clone().unwrap_or(Err(InspectError::NotFound))
    }
}
