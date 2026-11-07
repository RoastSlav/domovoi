# power_status

Prints the state files of every `/sys/class/power_supply/*` unit (type, online, status,
capacity, health, energy and charge-threshold files, whatever exists), then `upower -d`
and the UPS list and details via `upsc` (NUT). A missing tool prints "not installed", and
a failed UPS listing says why. Each `upower` and `upsc` call is cut off after 5 seconds
when `timeout` is installed, and lines naming serial numbers are left out. It reads only,
so triage can run it on its own while it investigates a power incident; for temperatures
and disk health use `hardware_health`.

No parameters.
