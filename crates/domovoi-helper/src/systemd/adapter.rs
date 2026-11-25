//! The real `SystemdView` (over `busctl` through the injected executor) and `UnitFs` (over the
//! disk). Neither parses `systemctl show` text, neither passes `--json` to `systemctl`, and
//! neither uses `GetUnit`, which fails for a unit that is not loaded.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{HostInspect, ProcessExecutor};
use domovoi_core::unit::{
    FileKind, FsError, LoadedProps, ProcessView, SystemdView, UnitFs, UnitId, UnitName, UnitProps,
    UnitRuntime, ViewError,
};

use super::parse::{self, Props};
use crate::reads::tools::{Tool, locate, spec};

const BUS: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const UNIT: &str = "org.freedesktop.systemd1.Unit";
const SERVICE: &str = "org.freedesktop.systemd1.Service";

/// Reads the service manager over its bus by running `busctl`.
pub struct BusctlView {
    exec: Arc<dyn ProcessExecutor>,
    inspect: Arc<dyn HostInspect>,
}

impl BusctlView {
    /// A view that runs `busctl` through `exec`, vetting the program with `inspect`.
    #[must_use]
    pub fn new(exec: Arc<dyn ProcessExecutor>, inspect: Arc<dyn HostInspect>) -> Self {
        Self { exec, inspect }
    }

    async fn call(&self, args: Vec<String>) -> Result<String, ViewError> {
        let program = locate(self.inspect.as_ref(), Tool::Busctl)
            .ok_or_else(|| ViewError("busctl is missing or not trusted".to_owned()))?;
        let output = self
            .exec
            .run(spec(program, args))
            .await
            .map_err(|e| ViewError(e.to_string()))?;
        if output.timed_out {
            return Err(ViewError("busctl timed out".to_owned()));
        }
        if output.exit_code != Some(0) {
            return Err(ViewError(format!("busctl exited {:?}", output.exit_code)));
        }
        if output.truncated {
            return Err(ViewError("busctl output was cut".to_owned()));
        }
        Ok(output.stdout)
    }

    /// `LoadUnit` for `name`: loads the unit if needed and answers its object path.
    async fn load(&self, name: &str) -> Result<String, ViewError> {
        let reply = self
            .call(strings(&[
                "--json=short",
                "call",
                BUS,
                MANAGER_PATH,
                MANAGER,
                "LoadUnit",
                "s",
                name,
            ]))
            .await?;
        parse::object_path(&reply)
    }

    async fn all(&self, path: &str, interface: &str) -> Result<Props, ViewError> {
        let reply = self
            .call(strings(&[
                "--json=short",
                "call",
                BUS,
                path,
                PROPERTIES,
                "GetAll",
                "s",
                interface,
            ]))
            .await?;
        parse::get_all(&reply)
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

#[async_trait]
impl SystemdView for BusctlView {
    async fn load_unit(&self, name: &UnitName) -> Result<UnitProps, ViewError> {
        let path = self.load(name.as_str()).await?;
        parse::unit_props(&self.all(&path, UNIT).await?)
    }

    async fn service_props(&self, id: &UnitId) -> Result<LoadedProps, ViewError> {
        let path = self.load(id.as_str()).await?;
        let unit = self.all(&path, UNIT).await?;
        let service = self.all(&path, SERVICE).await?;
        parse::loaded_props(&unit, &service)
    }

    async fn runtime(&self, id: &UnitId) -> Result<UnitRuntime, ViewError> {
        let path = self.load(id.as_str()).await?;
        let unit = self.all(&path, UNIT).await?;
        let service = self.all(&path, SERVICE).await?;
        parse::runtime(&unit, &service)
    }

    async fn process_view(&self, pid: u32) -> Result<ProcessView, ViewError> {
        read_process(pid)
    }
}

/// `environ`, `cmdline` and the owner of `/proc/<pid>`; NUL-separated, so no value is split.
fn read_process(pid: u32) -> Result<ProcessView, ViewError> {
    let dir = format!("/proc/{pid}");
    let read = |file: &str| {
        std::fs::read(format!("{dir}/{file}")).map_err(|e| ViewError(format!("{file}: {e}")))
    };
    let split = |bytes: Vec<u8>| -> Vec<String> {
        bytes
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect()
    };
    let uid = std::fs::metadata(&dir)
        .map_err(|e| ViewError(format!("stat: {e}")))?
        .uid();
    Ok(ProcessView {
        environ: split(read("environ")?),
        cmdline: split(read("cmdline")?),
        uid,
    })
}

/// The search path systemd reads units from, in priority order (the research replica).
pub const SEARCH_DIRS: [&str; 10] = [
    "/etc/systemd/system.control",
    "/run/systemd/system.control",
    "/run/systemd/transient",
    "/run/systemd/generator.early",
    "/etc/systemd/system",
    "/run/systemd/system",
    "/run/systemd/generator",
    "/usr/local/lib/systemd/system",
    "/usr/lib/systemd/system",
    "/run/systemd/generator.late",
];

/// Reads unit files from the real disk.
#[derive(Debug, Clone, Default)]
pub struct DiskFs;

/// The `.d` directory names systemd would consult for `id` and `names`: each name's own, and
/// the dash-prefix truncations (`a-b-.service.d`, `a-.service.d`). Template instances are
/// refused before this runs, so no name here has an instance part.
#[must_use]
pub fn dropin_dir_names(id: &UnitId, names: &BTreeSet<String>) -> BTreeSet<String> {
    let mut all: BTreeSet<String> = names.iter().cloned().collect();
    all.insert(id.as_str().to_owned());
    let mut out = BTreeSet::new();
    for name in &all {
        let Some(stem) = name.strip_suffix(".service") else {
            continue;
        };
        out.insert(format!("{name}.d"));
        for (index, _) in stem.match_indices('-') {
            out.insert(format!("{}-.service.d", &stem[..index]));
        }
    }
    out
}

impl UnitFs for DiskFs {
    fn lstat(&self, path: &str) -> Result<FileKind, FsError> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| FsError(e.to_string()))?;
        Ok(if meta.file_type().is_symlink() {
            FileKind::Symlink
        } else if meta.is_file() {
            FileKind::Regular
        } else {
            FileKind::Other
        })
    }

    fn read_once(&self, path: &str) -> Result<Vec<u8>, FsError> {
        std::fs::read(path).map_err(|e| FsError(e.to_string()))
    }

    fn enumerate_dropins(
        &self,
        id: &UnitId,
        names: &BTreeSet<String>,
    ) -> Result<Vec<String>, FsError> {
        let dirs = dropin_dir_names(id, names);
        // The first directory wins per file name, over every search directory, in order.
        let mut by_name: std::collections::BTreeMap<String, String> = Default::default();
        for base in SEARCH_DIRS {
            for dir in &dirs {
                let path = Path::new(base).join(dir);
                let entries = match std::fs::read_dir(&path) {
                    Ok(entries) => entries,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(FsError(format!("{}: {e}", path.display()))),
                };
                for entry in entries {
                    let entry = entry.map_err(|e| FsError(e.to_string()))?;
                    let file = entry.file_name().to_string_lossy().into_owned();
                    if file.ends_with(".conf") {
                        by_name
                            .entry(file)
                            .or_insert_with(|| entry.path().to_string_lossy().into_owned());
                    }
                }
            }
        }
        Ok(by_name.into_values().collect())
    }
}
