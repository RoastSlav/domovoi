//! Which release build fits this machine.

/// The release target for an operating system and CPU, as `std::env::consts` names them.
/// Only Linux has release builds, and only for x86-64 and 64-bit ARM.
pub fn for_platform(os: &str, arch: &str) -> anyhow::Result<&'static str> {
    anyhow::ensure!(
        os == "linux",
        "release builds exist for Linux only; this is {os}"
    );
    match arch {
        "x86_64" => Ok("x86_64-unknown-linux-musl"),
        "aarch64" => Ok("aarch64-unknown-linux-musl"),
        other => anyhow::bail!("no release build for {other}; build from source instead"),
    }
}

/// The release target for the machine this binary runs on.
pub fn for_this_machine() -> anyhow::Result<&'static str> {
    for_platform(std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::for_platform;

    #[test]
    fn picks_the_musl_build_for_each_supported_cpu() {
        assert_eq!(
            for_platform("linux", "x86_64").unwrap(),
            "x86_64-unknown-linux-musl"
        );
        assert_eq!(
            for_platform("linux", "aarch64").unwrap(),
            "aarch64-unknown-linux-musl"
        );
    }

    #[test]
    fn refuses_other_cpus_and_other_systems() {
        assert!(for_platform("linux", "riscv64").is_err());
        assert!(for_platform("macos", "aarch64").is_err());
        assert!(for_platform("windows", "x86_64").is_err());
    }
}
