# hardware_health

Prints `sensors` output or the raw `thermal_zone*` readings, kernel-priority errors from
the last two hours, EDAC counters and `smartctl -H` for SATA and NVMe disks. A tool that
is missing prints "not installed". It reads only, so triage can run it on its own for
hardware incidents.

No parameters.
