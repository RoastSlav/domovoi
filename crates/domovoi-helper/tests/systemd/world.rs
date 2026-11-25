//! A scripted manager that is also the process runner: `systemctl restart` flips its state.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::identity::{PathFacts, PathKind};
use domovoi_core::ports::{ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};
use domovoi_core::unit::{
    ExecCommand, FileKind, FsError, LoadedProps, ProcessView, SystemdView, UnitFs, UnitId,
    UnitName, UnitPolicy, UnitProps, UnitRuntime, ViewError,
};
use domovoi_helper::systemd::UnitEnv;
use domovoi_testkit::{FakeClock, FakeHostInspect};
use jiff::{SignedDuration, Timestamp};

pub const FRAGMENT: &str = "/etc/systemd/system/example.service";
pub const DROP_IN: &str = "/etc/systemd/system/example.service.d/10-local.conf";

pub struct State {
    pub props: UnitProps,
    pub loaded: LoadedProps,
    pub before: UnitRuntime,
    pub after: UnitRuntime,
    pub process: Option<ProcessView>,
    pub restart_exit: ProcessOutput,
    pub restarted: bool,
    pub calls: Vec<ProcessSpec>,
    pub service_reads: u32,
    /// Names the manager answers with another unit's id: name to reported `Id`.
    pub renamed: BTreeMap<String, String>,
}

#[cfg(test)]
pub struct World {
    pub state: Mutex<State>,
}

fn runtime(invocation: &str, pid: u32) -> UnitRuntime {
    UnitRuntime {
        invocation_id: invocation.to_owned(),
        main_pid: pid,
        active_state: "active".to_owned(),
        result: "success".to_owned(),
        n_restarts: 0,
    }
}

#[cfg(test)]
impl World {
    pub fn new() -> Self {
        let loaded = LoadedProps {
            exec_start: vec![ExecCommand {
                path: "/usr/bin/example-daemon".to_owned(),
                argv: vec!["/usr/bin/example-daemon".to_owned(), "--serve".to_owned()],
                ignore_failure: false,
            }],
            environment: vec!["A=1".to_owned()],
            service_type: "simple".to_owned(),
            restart: "no".to_owned(),
            start_limit_interval_usec: 10_000_000,
            start_limit_burst: 5,
            ..LoadedProps::default()
        };
        let props = UnitProps {
            id: "example.service".to_owned(),
            names: BTreeSet::from(["example.service".to_owned()]),
            load_state: "loaded".to_owned(),
            unit_file_state: "enabled".to_owned(),
            transient: false,
            fragment_path: FRAGMENT.to_owned(),
            source_path: String::new(),
            drop_in_paths: vec![DROP_IN.to_owned()],
            need_daemon_reload: false,
            triggered_by: BTreeSet::new(),
            triggers: BTreeSet::new(),
        };
        Self {
            state: Mutex::new(State {
                props,
                loaded,
                before: runtime("aa", 100),
                after: runtime("bb", 200),
                process: Some(ProcessView {
                    environ: vec!["A=1".to_owned(), "PATH=/usr/bin".to_owned()],
                    cmdline: vec!["/usr/bin/example-daemon".to_owned(), "--serve".to_owned()],
                    uid: 0,
                }),
                restart_exit: ProcessOutput {
                    exit_code: Some(0),
                    ..ProcessOutput::default()
                },
                restarted: false,
                calls: Vec::new(),
                service_reads: 0,
                renamed: BTreeMap::new(),
            }),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.state.lock().expect("lock"))
    }

    pub fn calls(&self) -> Vec<ProcessSpec> {
        self.with(|s| s.calls.clone())
    }
}

#[async_trait]
impl SystemdView for World {
    async fn load_unit(&self, name: &UnitName) -> Result<UnitProps, ViewError> {
        Ok(self.with(|s| {
            let mut props = s.props.clone();
            if let Some(id) = s.renamed.get(name.as_str()) {
                props.id.clone_from(id);
                props.names.insert(id.clone());
            }
            props
        }))
    }
    async fn service_props(&self, _: &UnitId) -> Result<LoadedProps, ViewError> {
        Ok(self.with(|s| {
            s.service_reads += 1;
            s.loaded.clone()
        }))
    }
    async fn runtime(&self, _: &UnitId) -> Result<UnitRuntime, ViewError> {
        Ok(self.with(|s| {
            if s.restarted {
                s.after.clone()
            } else {
                s.before.clone()
            }
        }))
    }
    async fn process_view(&self, _: u32) -> Result<ProcessView, ViewError> {
        self.with(|s| s.process.clone())
            .ok_or_else(|| ViewError("gone".to_owned()))
    }
}

#[async_trait]
impl ProcessExecutor for World {
    async fn run(&self, spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
        Ok(self.with(|s| {
            s.calls.push(spec);
            s.restarted = true;
            s.restart_exit.clone()
        }))
    }
}

#[derive(Clone, Default)]
pub struct Disk {
    pub files: BTreeMap<String, Vec<u8>>,
    pub kinds: BTreeMap<String, FileKind>,
    pub disk_dropins: Vec<String>,
}

#[cfg(test)]
impl Disk {
    pub fn new() -> Self {
        let mut disk = Self::default();
        for path in [FRAGMENT, DROP_IN] {
            disk.files.insert(path.to_owned(), b"[Service]\n".to_vec());
            disk.kinds.insert(path.to_owned(), FileKind::Regular);
        }
        disk.disk_dropins = vec![DROP_IN.to_owned()];
        disk
    }
}

impl UnitFs for Disk {
    fn lstat(&self, path: &str) -> Result<FileKind, FsError> {
        self.kinds
            .get(path)
            .copied()
            .ok_or_else(|| FsError("missing".to_owned()))
    }
    fn read_once(&self, path: &str) -> Result<Vec<u8>, FsError> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| FsError("missing".to_owned()))
    }
    fn enumerate_dropins(&self, _: &UnitId, _: &BTreeSet<String>) -> Result<Vec<String>, FsError> {
        Ok(self.disk_dropins.clone())
    }
}

#[cfg(test)]
pub fn policy() -> UnitPolicy {
    UnitPolicy {
        allow: BTreeSet::from([UnitId::parse("example.service").expect("id")]),
        protected: BTreeSet::from(["ssh.service".to_owned()]),
    }
}

/// The host with `systemctl` installed as root, mode 0755.
#[cfg(test)]
pub fn inspect() -> FakeHostInspect {
    FakeHostInspect::new().ids(0, 0).path(
        "/usr/bin/systemctl",
        PathFacts {
            kind: Some(PathKind::File),
            symlink: false,
            mode: 0o755,
            uid: 0,
            gid: 0,
        },
    )
}

#[cfg(test)]
pub fn clock() -> FakeClock {
    FakeClock::new("2026-05-01T12:00:00Z".parse::<Timestamp>().expect("time"))
}

/// A state directory holding the lock file the restart takes.
#[cfg(test)]
pub fn state_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(dir.path().join("state.lock"), "").expect("lock file");
    dir
}

/// Everything a restart borrows, owned by the test.
#[cfg(test)]
pub struct Fixture {
    pub world: World,
    pub disk: Disk,
    pub inspect: FakeHostInspect,
    pub clock: FakeClock,
    pub dir: tempfile::TempDir,
    pub policy: UnitPolicy,
}

#[cfg(test)]
impl Fixture {
    pub fn new() -> Self {
        Self {
            world: World::new(),
            disk: Disk::new(),
            inspect: inspect(),
            clock: clock(),
            dir: state_dir(),
            policy: policy(),
        }
    }

    pub fn env(&self, mutations_per_hour: u32) -> UnitEnv<'_> {
        UnitEnv {
            sys: &self.world,
            fs: &self.disk,
            exec: &self.world,
            inspect: &self.inspect,
            clock: &self.clock,
            state_dir: Path::new(self.dir.path()),
            policy: &self.policy,
            timeout: SignedDuration::from_secs(30),
            mutations_per_hour,
        }
    }
}
