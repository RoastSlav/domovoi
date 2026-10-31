//! Parsers and checks must never panic, whatever the host files contain.

use std::path::Path;

use domovoi_checks::framework::Check;
use domovoi_checks::system::{
    CpuTemperature, MemoryAvailable, PowerSupply, Reboot, RebootRequired, disk, kernel, memory,
    power, reboot, reboot_required, temperature,
};
use proptest::prelude::*;

use crate::harness::Harness;

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, text).unwrap();
}

proptest! {
    #[test]
    fn checks_never_panic_on_garbage_input(text in any::<String>(), lines in proptest::collection::vec(".*", 0..8)) {
        let joined = lines.join("\n");
        for input in [text.as_str(), joined.as_str()] {
            let _ = disk::parse_mounts(input);
            let _ = disk::is_pseudo(input);
            let _ = memory::parse_meminfo(input);
            let _ = temperature::parse_millidegrees(input);
            let _ = reboot::parse_uptime(input);
            let _ = reboot_required::parse_packages(input);
            let _ = power::parse_number(input);
            let _ = kernel::classify(input);
            let _ = kernel::io_device(input);
            let _ = kernel::oom_process(input);
            let _ = kernel::parse_cursor_line(input);
        }
        let _ = power::health_pct(f64::NAN, 0.0);

        // Whole checks over a tree whose files all hold the generated text.
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "proc/meminfo",
            "proc/uptime",
            "proc/sys/kernel/random/boot_id",
            "sys/class/hwmon/hwmon0/name",
            "sys/class/hwmon/hwmon0/temp1_input",
            "sys/class/thermal/thermal_zone0/temp",
            "sys/class/power_supply/BAT0/type",
            "sys/class/power_supply/BAT0/capacity",
            "sys/class/power_supply/BAT0/status",
            "var/run/reboot-required",
            "var/run/reboot-required.pkgs",
        ] {
            write(dir.path(), path, &joined);
        }
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let checks: Vec<Box<dyn Check>> = vec![
                Box::new(MemoryAvailable::new().unwrap()),
                Box::new(CpuTemperature::new().unwrap()),
                Box::new(Reboot::new().unwrap()),
                Box::new(RebootRequired::new().unwrap()),
                Box::new(PowerSupply::new().unwrap()),
            ];
            let mut h = Harness::new("healthy").await;
            h.fs = domovoi_testkit::FixtureHostFs::new(dir.path());
            for check in &checks {
                let _ = h.try_run(check.as_ref()).await;
            }
        });
    }
}
