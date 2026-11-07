# hardware_health

Prints `sensors` output or the raw `thermal_zone*` readings, kernel-priority errors from
the last two hours, EDAC counters and `smartctl -H` for SATA and NVMe disks. It runs
`smartctl -n standby`, so a sleeping disk is reported as "standby, not checked" and is
never woken; only `/dev/sd?` and `/dev/nvme?n1` are looked at, other disk names (`sdaa`
and later, second NVMe namespace, `/dev/disk/by-id`) are not. A tool that is missing
prints "not installed". It reads only, so triage can run it on its own for temperature,
kernel-error and SMART incidents; for mains, battery and UPS state use `power_status`.

No parameters.
