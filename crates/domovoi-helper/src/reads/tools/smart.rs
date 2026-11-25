//! `smart.read`: scan for devices and report on one, filtered by the owner's allowlist.

use domovoi_core::helper::read::{
    DeviceRef, MAX_DEVICES, ReadFailure, ReadToken, SmartReportOutput,
};

use super::{Tool, ToolEnv, command_failed, run, smart_glob, smart_report_args, smart_scan_args};

/// The devices `smartctl --scan-open` finds that pass the device grammar and `patterns`, sorted
/// and at most [`MAX_DEVICES`].
pub async fn scan(env: ToolEnv<'_>, patterns: &[String]) -> Result<Vec<DeviceRef>, ReadFailure> {
    let output = run(env, Tool::Smartctl, smart_scan_args()).await?;
    let json: serde_json::Value = match serde_json::from_str(&output.stdout) {
        Ok(json) => json,
        Err(_) if output.exit_code != Some(0) && output.stdout.trim().is_empty() => {
            return Err(command_failed(&output));
        }
        Err(_) => return Err(ReadFailure::bare(ReadToken::OutputMalformed)),
    };
    let names = json
        .get("devices")
        .and_then(serde_json::Value::as_array)
        .ok_or(ReadFailure::bare(ReadToken::OutputMalformed))?;
    let mut devices: Vec<DeviceRef> = names
        .iter()
        .filter_map(|entry| entry.get("name")?.as_str())
        .filter_map(|name| DeviceRef::parse(name).ok())
        .filter(|device| smart_glob::allows(patterns, device))
        .collect();
    devices.sort();
    devices.dedup();
    devices.truncate(MAX_DEVICES);
    Ok(devices)
}

/// The report of `device`. A device outside `patterns` is refused before anything is run. A
/// non-zero exit with output is a normal report: `smartctl`'s exit code is a bit mask and a
/// failing disk is the point.
pub async fn report(
    env: ToolEnv<'_>,
    patterns: &[String],
    device: &DeviceRef,
) -> Result<SmartReportOutput, ReadFailure> {
    if !smart_glob::allows(patterns, device) {
        return Err(ReadFailure::bare(ReadToken::DeviceNotAllowed));
    }
    let output = run(env, Tool::Smartctl, smart_report_args(device)).await?;
    if output.exit_code != Some(0) && output.stdout.trim().is_empty() {
        return Err(command_failed(&output));
    }
    Ok(SmartReportOutput {
        json: output.stdout,
        exit_code: output.exit_code,
        truncated: output.truncated,
    })
}
