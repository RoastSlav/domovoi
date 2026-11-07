# power_status

Prints the state files (`type online status capacity health energy_full energy_full_design
charge_control_end_threshold`, whatever exists) of every `/sys/class/power_supply/*` unit,
then `upower -d` and the UPS list via `upsc` (NUT). A tool that is missing prints
"not installed". It reads only, so triage can run it on its own while it investigates a
power incident.

No parameters.
