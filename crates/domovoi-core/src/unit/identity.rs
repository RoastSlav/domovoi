//! The unit identity a person approves and a restart re-checks, and its hash.

use super::encode::Frames;
use super::name::{IdentityHash, UnitId};
use super::props::{ExecCommand, LoadedProps, UnitFileState};

/// A file's path and the SHA-256 of the bytes read in one open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePin {
    /// The absolute path.
    pub path: String,
    /// Lowercase hex SHA-256 of the contents.
    pub sha256: String,
}

/// Everything a restart is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitIdentityV1 {
    /// The canonical id.
    pub id: UnitId,
    /// The unit file state.
    pub unit_file_state: UnitFileState,
    /// The unit file.
    pub fragment: FilePin,
    /// The drop-ins, in application order.
    pub drop_ins: Vec<FilePin>,
    /// The drop-in paths found on disk; equal to the paths of `drop_ins`.
    pub disk_drop_ins: Vec<String>,
    /// SHA-256 of the encoded loaded properties.
    pub loaded_hash: [u8; 32],
}

fn exec(frames: &mut Frames, name: &str, commands: &[ExecCommand]) {
    frames.count(name, commands.len());
    for command in commands {
        frames.value("path", &command.path);
        frames.value("ignore", if command.ignore_failure { "1" } else { "0" });
        frames.list("argv", &command.argv);
    }
}

/// The loaded properties as the text that is hashed. Set-valued properties are `BTreeSet`s,
/// so ordering cannot leak in; `Environment` and every `Exec*` argv keep their order.
#[must_use]
pub fn encode_loaded(props: &LoadedProps) -> Vec<u8> {
    let mut f = Frames::new("unit-loaded:v1");
    exec(&mut f, "exec_start", &props.exec_start);
    exec(&mut f, "exec_start_pre", &props.exec_start_pre);
    exec(&mut f, "exec_start_post", &props.exec_start_post);
    exec(&mut f, "exec_stop", &props.exec_stop);
    f.count("exec_reload", props.exec_reload.len());
    for argv in &props.exec_reload {
        f.list("argv", argv);
    }
    f.list("environment", &props.environment);
    f.set("environment_files", &props.environment_files);
    f.value("user", &props.user);
    f.value("group", &props.group);
    f.value("working_directory", &props.working_directory);
    f.value("restart", &props.restart);
    f.value("type", &props.service_type);
    f.value(
        "start_limit_interval_usec",
        &props.start_limit_interval_usec.to_string(),
    );
    f.value("start_limit_burst", &props.start_limit_burst.to_string());
    f.set("requires", &props.requires);
    f.set("binds_to", &props.binds_to);
    f.set("part_of", &props.part_of);
    f.set("wants", &props.wants);
    f.finish()
}

impl UnitIdentityV1 {
    /// The text that is hashed: the domain tag first, every field framed `name:len:value`,
    /// lists as a count then framed elements.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut f = Frames::new("unit-identity:v1");
        f.value("id", self.id.as_str());
        f.value("unit_file_state", self.unit_file_state.as_str());
        f.value("fragment_path", &self.fragment.path);
        f.value("fragment_sha256", &self.fragment.sha256);
        f.count("drop_ins", self.drop_ins.len());
        for pin in &self.drop_ins {
            f.value("path", &pin.path);
            f.value("sha256", &pin.sha256);
        }
        f.list("disk_drop_ins", &self.disk_drop_ins);
        f.value("loaded", &hex::encode(self.loaded_hash));
        f.finish()
    }

    /// The identity hash: SHA-256 of [`Self::encode`].
    #[must_use]
    pub fn hash(&self) -> IdentityHash {
        IdentityHash::of_encoded(&self.encode())
    }

    /// The name of the first field that differs from `other`, in the order `id`,
    /// `unit_file_state`, `fragment`, `drop_in:<path>`, `drop_ins`, `loaded`; `None` when
    /// they are equal.
    #[must_use]
    pub fn first_difference(&self, other: &Self) -> Option<String> {
        if self.id != other.id {
            return Some("id".to_owned());
        }
        if self.unit_file_state != other.unit_file_state {
            return Some("unit_file_state".to_owned());
        }
        if self.fragment != other.fragment {
            return Some("fragment".to_owned());
        }
        for (a, b) in self.drop_ins.iter().zip(&other.drop_ins) {
            if a != b {
                let path = if a.path == b.path { &a.path } else { &b.path };
                return Some(format!("drop_in:{path}"));
            }
        }
        if self.drop_ins.len() != other.drop_ins.len() || self.disk_drop_ins != other.disk_drop_ins
        {
            return Some("drop_ins".to_owned());
        }
        if self.loaded_hash != other.loaded_hash {
            return Some("loaded".to_owned());
        }
        None
    }
}
