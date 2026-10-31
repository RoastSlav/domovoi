//! Skill folders on disk and a request context around them.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_agent::requests::RequestCtx;
use domovoi_agent::scheduler::CheckEnv;
use domovoi_agent::store::CheckStore;
use domovoi_checks::commands::Commands;
use domovoi_checks::containers::ContainerSource;
use domovoi_checks::framework::Check;
use domovoi_checks::system::{FsStats, FsUsage};
use domovoi_core::ports::FsError;
use domovoi_core::{HostId, Risk};
use domovoi_runner::SkillCatalog;
use domovoi_testkit::build::T0;
use domovoi_testkit::{FakeClock, FakeProcessExecutor, FixtureHostFs, ListRedactor};

/// The secret the test redactor scrubs.
pub const SECRET: &str = "hunter2-secret";

/// Writes a skill folder `name` with `risk` and one string param `service`.
pub fn install(dir: &Path, name: &str, risk: &str) {
    let folder = dir.join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = format!(
        "name = \"{name}\"\ndescription = \"Test skill\"\nrisk = \"{risk}\"\nentry = \"run\"\n\
         timeout = \"10s\"\nmax_output_bytes = 4096\n\n[[params]]\nname = \"service\"\n\
         description = \"A service\"\nkind = {{ type = \"string\", pattern = '^[a-z]{{1,16}}$', max_len = 16 }}\n"
    );
    std::fs::write(folder.join("skill.toml"), manifest).unwrap();
    std::fs::write(folder.join("run"), "#!/bin/sh\necho ran\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(folder.join("run"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Disk usage that reports nothing.
pub struct NoStats;

#[async_trait]
impl FsStats for NoStats {
    async fn usage(&self, _mount_point: &str) -> Result<FsUsage, FsError> {
        Err(FsError::NotFound)
    }
}

/// What a test builds a request context from.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub exec: Arc<FakeProcessExecutor>,
    pub clock: Arc<FakeClock>,
    pub host_id: HostId,
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            exec: Arc::new(FakeProcessExecutor::new()),
            clock: Arc::new(FakeClock::new(T0)),
            host_id: HostId::new(),
        }
    }

    pub fn skills(&self) -> std::path::PathBuf {
        let dir = self.dir.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn ctx(
        &self,
        allow_risk: Vec<Risk>,
        containers: Option<Arc<dyn ContainerSource>>,
        checks: Vec<Box<dyn Check>>,
    ) -> RequestCtx {
        let (catalog, _issues) = SkillCatalog::load(&self.skills());
        let fs = Arc::new(FixtureHostFs::new(self.dir.path().join("host")));
        let commands = Arc::new(Commands::default());
        let check_env = CheckEnv {
            host: self.host_id,
            fs: fs.clone(),
            exec: self.exec.clone(),
            commands: commands.clone(),
            clock: self.clock.clone(),
            settings: Arc::new(BTreeMap::new()),
        };
        RequestCtx {
            host_id: self.host_id,
            catalog: Arc::new(catalog),
            allow_risk,
            exec: self.exec.clone(),
            redactor: Arc::new(ListRedactor::new(vec![SECRET.to_owned()])),
            clock: self.clock.clone(),
            fs,
            fs_stats: Arc::new(NoStats),
            commands,
            containers,
            checks: Arc::new(checks),
            check_env,
            store: Arc::new(CheckStore::open(self.dir.path()).unwrap()),
        }
    }
}
