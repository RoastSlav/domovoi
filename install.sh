#!/bin/sh
# Installs Domovoi from a GitHub release.
#
#   curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
#
# It downloads the release archive for this machine, checks its SHA-256, installs the
# binary, and for --role hub or --role agent creates the users, directories and systemd
# units. It writes exactly one config file, /etc/domovoi/helper.toml, and only when it is
# absent. It never starts the hub or the agent; the only things it starts are the helper
# and skill runner sockets of an agent on a systemd host, the second one only after the
# self-test passes. Running it again upgrades the binary and leaves everything else alone.
#
# An agent on a systemd host with no hub gets three identities: the unprivileged
# domovoi-agent account runs the agent, the group-less domovoi-skill account is for skills,
# and root runs the sandboxed helper behind a socket. The installer creates only what is
# absent, never changes an account or a mode it finds, and reports what is wrong instead.
# An agent that already runs as root keeps running as root until the owner runs
# --migrate-identity.
set -eu

REPO="RoastSlav/domovoi"
RELEASE_BASE="${DOMOVOI_RELEASE_BASE:-https://github.com/$REPO/releases}"

# Every directory a shipped unit file lets a service write to, or names in a mask. Each
# one is created by a line in this script that comes before the first command that enables
# or starts a service, because a unit whose path is missing fails to start.
# BEGIN created-set
CREATED_PATHS="/etc/domovoi /etc/domovoi/skills /var/lib/domovoi /var/lib/domovoi-agent /var/lib/domovoi-packages /var/lib/domovoi-helper"
# END created-set

agent_unit_path="/etc/systemd/system/domovoi-agent.service"
record_dir="/etc/domovoi/identity-migration"

usage() {
    cat <<'EOF'
Usage: install.sh --role hub|agent [options]
       install.sh --rollback-identity

Options:
  --role hub|agent     what this machine runs (required)
  --version vX.Y.Z     the release to install (default: the latest)
  --prefix DIR         where the binary goes, as DIR/bin/domovoi (default: /usr/local)
  --no-service         install only the binary and examples: no user, directories or unit
  --print-unit NAME    print a systemd unit and exit: hub, agent, helper-socket,
                       helper-service, runner-socket, runner-service, or agent-legacy
                       (the root agent unit used where the privilege boundary cannot be
                       set up)
  --migrate-identity   with --role agent: move an agent that runs as root to the
                       domovoi-agent account; shows a plan and asks you to type "migrate"
  --rollback-identity  undo --migrate-identity from the record it saved
  -h, --help           show this help

Environment:
  DOMOVOI_RELEASE_BASE  download from this base URL instead of GitHub releases
EOF
}

say() { printf '%s\n' "$*"; }
warn() { printf 'install.sh: %s\n' "$*" >&2; }
fail() {
    printf 'install.sh: %s\n' "$*" >&2
    exit 1
}

hub_unit() {
    cat <<'EOF'
# The Domovoi hub. Installed by install.sh; start it with
#   systemctl enable --now domovoi-hub
# then open the setup link it prints once: journalctl -u domovoi-hub | grep setup
#
# No config file is needed. /etc/domovoi/hub.toml and /etc/domovoi/hub.env are read when
# they exist (config as code); anything set there is read-only in the web UI.
#
# A `cli` model provider (Claude Code, Codex) runs as this unit's user, with
# /var/lib/domovoi as its home, and ProtectHome=yes hides other home directories. Log
# the tool in as that user: sudo -u domovoi -H claude login
# Node-based tools such as Claude Code need a JIT, which MemoryDenyWriteExecute=yes
# blocks; add a drop-in (systemctl edit domovoi-hub) with MemoryDenyWriteExecute=no.

[Unit]
Description=Domovoi hub
Documentation=https://github.com/RoastSlav/domovoi
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=domovoi
Group=domovoi
EnvironmentFile=-/etc/domovoi/hub.env
ExecStart=/usr/local/bin/domovoi hub run
Restart=on-failure
RestartSec=5
# The hub stops within 15 seconds.
TimeoutStopSec=20

# hub.db, secret.key, hub.lock, tls/ and backups/ live here.
ReadWritePaths=/var/lib/domovoi
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
SystemCallArchitectures=native
CapabilityBoundingSet=

[Install]
WantedBy=multi-user.target
EOF
}

agent_unit() {
    cat <<'EOF'
# The Domovoi agent. Installed by install.sh; enroll it with the command from the hub's
# "Add host" dialog, then start it with
#   systemctl enable --now domovoi-agent
#
# It runs as the unprivileged domovoi-agent account (the primary group of the same name, plus
# systemd-journal when that group exists, never docker). Work that needs root goes through the
# local helper socket; the unit works without the helper, so it Wants the socket rather than
# Requires it. The unit still keeps /usr and /etc read-only, gives the agent a private /tmp,
# and only lets it write its own state and the package directory.
#
# RestrictSUIDSGID=no because a skill may run a setuid tool (for example `sudo -u` in a
# manifest's command, or a vendor CLI); blocking it would make those skills fail.
#
# Do not add drop-ins to this unit: `domovoi agent selftest` reports any drop-in as
# unit_modified.

[Unit]
Description=Domovoi agent
Documentation=https://github.com/RoastSlav/domovoi
Wants=network-online.target domovoi-helper.socket
After=network-online.target domovoi-helper.socket

[Service]
Type=simple
User=domovoi-agent
Group=domovoi-agent
ExecStart=/usr/local/bin/domovoi agent run
Restart=always
RestartSec=5

ReadWritePaths=/var/lib/domovoi-agent /var/lib/domovoi-packages
ProtectSystem=full
PrivateTmp=yes
ProtectKernelModules=yes
RestrictSUIDSGID=no

[Install]
WantedBy=multi-user.target
EOF
}

legacy_agent_unit() {
    cat <<'EOF'
# The Domovoi agent. Installed by install.sh; enroll it with the command from the hub's
# "Add host" dialog, then start it with
#   systemctl enable --now domovoi-agent
#
# It runs as root: SMART needs the disks, the journal checks read every unit's log, and
# skills run as the user their manifest names. The unit still keeps /usr and /etc
# read-only, gives the agent a private /tmp, and only lets it write its own state.
#
# RestrictSUIDSGID=no because a skill may run a setuid tool (for example `sudo -u` in a
# manifest's command, or a vendor CLI); blocking it would make those skills fail.
#
# To run it unprivileged instead, add a drop-in (systemctl edit domovoi-agent) with
# User=, Group= and ReadWritePaths= for its state directory. SMART, journal and skill
# checks that need root then report errors; the rest keep working.

[Unit]
Description=Domovoi agent
Documentation=https://github.com/RoastSlav/domovoi
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/domovoi agent run
Restart=always
RestartSec=5

ReadWritePaths=/var/lib/domovoi-agent
ProtectSystem=full
PrivateTmp=yes
ProtectKernelModules=yes
RestrictSUIDSGID=no

[Install]
WantedBy=multi-user.target
EOF
}

helper_socket_unit() {
    cat <<'EOF'
[Unit]
Description=Domovoi helper socket
After=nftables.service

[Socket]
ListenStream=/run/domovoi-helper.sock
SocketUser=root
SocketGroup=domovoi-agent
SocketMode=0660
Accept=yes
MaxConnections=8
RemoveOnStop=yes

[Install]
WantedBy=sockets.target
EOF
}

helper_service_unit() {
    cat <<'EOF'
[Unit]
Description=Domovoi helper (connection %i)
After=nftables.service

[Service]
Type=simple
# Root: no User= line on purpose.
StandardInput=socket
StandardOutput=journal
StandardError=journal
ExecStart=/usr/local/bin/domovoi helper serve
Restart=no
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/domovoi-helper
ProtectHome=yes
PrivateTmp=yes
RestrictNamespaces=yes
# Disks and /dev/kmsg are needed by the read operations (smartctl, dmesg); every other
# device stays closed. The capability set is the widened baseline of ADR 0036: the
# privileged rows of deploy/test/identities.sh measure it (decision OD-1 is unmeasured
# until they have run on a real host).
PrivateDevices=no
DevicePolicy=closed
DeviceAllow=block-sd r
DeviceAllow=block-nvme r
DeviceAllow=char-nvme r
DeviceAllow=char-sg r
CapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_RAWIO CAP_SYS_ADMIN CAP_SYSLOG CAP_SYS_PTRACE CAP_DAC_READ_SEARCH
AmbientCapabilities=
RuntimeMaxSec=60s
KillMode=control-group
TimeoutStopSec=5s
EOF
}

helper_selftest_unit() {
    cat <<'EOF'
[Unit]
Description=Domovoi helper firewall leak self-test
# Runs at every boot and after the installer; `--if-changed` exits at once unless the kernel, the
# nft version or the layout changed since the stored result. The helper's socket waits for it.
Before=domovoi-helper.socket
After=nftables.service

[Service]
Type=oneshot
RemainAfterExit=yes
# Root: no User= line on purpose. This unit and `domovoi helper containment-drill` are the only
# places that create a network namespace; the long-lived helper units never do.
ExecStart=/usr/local/bin/domovoi helper nft-selftest --if-changed
NoNewPrivileges=yes
ProtectSystem=full
ReadWritePaths=/var/lib/domovoi-helper
ProtectHome=yes
PrivateTmp=yes
# Namespaces and mounts need exactly these two; [validate] on a real host and record the
# result as the privilege statement.
CapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_ADMIN
AmbientCapabilities=
TimeoutStartSec=20min

[Install]
WantedBy=multi-user.target
EOF
}

runner_socket_unit() {
    cat <<'EOF'
[Unit]
Description=Domovoi skill runner socket

[Socket]
ListenStream=/run/domovoi-skill-runner.sock
SocketUser=root
SocketGroup=domovoi-agent
SocketMode=0660
Accept=yes
MaxConnections=16
RemoveOnStop=yes

[Install]
WantedBy=sockets.target
EOF
}

runner_service_unit() {
    cat <<'EOF'
[Unit]
Description=Domovoi skill runner (connection %i)
# No ConditionPathExists= for the bind sources: a condition would skip the unit silently. A plain bind of an absent
# path fails the start with 226/NAMESPACE, which is the wanted fail-closed behaviour.

[Service]
Type=simple
User=domovoi-skill
Group=domovoi-skill
StandardInput=socket
StandardOutput=journal
StandardError=journal
ExecStart=/usr/local/bin/domovoi skill-runner serve --skills-dir /etc/domovoi/skills --packages-dir /var/lib/domovoi-packages
Restart=no
NoNewPrivileges=yes
CapabilityBoundingSet=
AmbientCapabilities=
RestrictSUIDSGID=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectProc=invisible
ProcSubset=pid
TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro
BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages
RuntimeMaxSec=3720s
KillMode=control-group
TimeoutStopSec=5s
EOF
}

# The runner service with the skills and packages directories of this host in place of the
# defaults, everywhere they occur (the ExecStart arguments and the bind). The two names
# go through placeholders so that one value cannot be rewritten by the other. The values
# come from `domovoi agent dirs`, which refuses any path a unit cannot hold safely.
render_runner_service() {
    runner_service_unit | sed \
        -e 's|/etc/domovoi/skills|@SKILLS_DIR@|g' \
        -e 's|/var/lib/domovoi-packages|@PACKAGES_DIR@|g' \
        -e "s|@SKILLS_DIR@|$runner_skills_dir|g" \
        -e "s|@PACKAGES_DIR@|$runner_packages_dir|g"
}

# The default /etc/domovoi/helper.toml, written only when the file is absent.
helper_toml() {
    cat <<'EOF'
# /etc/domovoi/helper.toml (root:root 0600). Written by the installer only when absent.
groups = ["docker.read", "systemd.read", "smart.read", "kernel.read", "net.read", "safety.attest"]

[platform]
allow_write_groups_on_untested = false     # the per-host opt-in: off by default, edited by the owner

[smart]
devices = ["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]

[safety]                                    # parsed, not interpreted yet
management_cidrs = []
protected_units = ["sshd.service", "domovoi-agent.service", "domovoi-helper.socket", "domovoi-skill-runner.socket"]
protected_containers = []

[safety.containment]
max_ttl = "1h"
max_active = 0

[limits]
timeout = "30s"
mutations_per_hour = 0
EOF
}

# Creates DIR with MODE and OWNER only when it does not exist, so a second run keeps
# whatever was changed since.
make_dir() {
    if [ ! -d "$1" ]; then
        mkdir -p "$1"
        chmod "$2" "$1"
        chown "$3" "$1"
        say "Created $1"
    fi
}

# Copies each starter skill that is not in the agent's skills directory yet, so a new
# install gets all of them and an upgrade adds the ones a release has added. A folder
# that is already there is never touched, so edits to a skill are kept. With a second
# argument of "normalise" the copy also gets the modes of the three-identities layout.
add_starter_skills() {
    for skill in "$1"/*/; do
        [ -d "$skill" ] || continue
        dest="/etc/domovoi/skills/$(basename "$skill")"
        [ ! -e "$dest" ] || continue
        cp -R "$skill" "$dest"
        chown -R root:root "$dest"
        chmod -R go-w "$dest"
        if [ "${2:-}" = "normalise" ]; then
            normalise_tree "$dest"
        fi
        say "Added the starter skill $dest"
    done
}

# Gives a skills tree the modes the unprivileged runner needs: root-owned, directories
# 0755, files 0644, files that had an execute bit 0755, no group or other write bit.
normalise_tree() {
    chown -R -h root:root "$1"
    find "$1" -type d -exec chmod 0755 {} +
    find "$1" -type f \( -perm -100 -o -perm -010 -o -perm -001 \) -exec chmod 0755 {} +
    find "$1" -type f ! \( -perm -100 -o -perm -010 -o -perm -001 \) -exec chmod 0644 {} +
}

create_hub_layout() {
    if ! id domovoi >/dev/null 2>&1; then
        if command -v useradd >/dev/null 2>&1; then
            nologin="$(command -v nologin || echo /usr/sbin/nologin)"
            useradd --system --user-group --home-dir /var/lib/domovoi --no-create-home \
                --shell "$nologin" domovoi
        elif command -v adduser >/dev/null 2>&1; then
            addgroup -S domovoi 2>/dev/null || true
            adduser -S -D -H -h /var/lib/domovoi -s /sbin/nologin -G domovoi domovoi
        else
            fail "cannot create the domovoi user: neither useradd nor adduser is available"
        fi
        say "Created the domovoi system user."
    fi
    make_dir /etc/domovoi 0750 root:domovoi
    make_dir /var/lib/domovoi 0750 domovoi:domovoi
}

# Today's agent layout: everything owned by root. Used where the privilege boundary cannot
# be set up (no systemd, a hub on the same host) and for an agent that still runs as root.
create_legacy_agent_layout() {
    make_dir /etc/domovoi 0750 root:root
    make_dir /etc/domovoi/skills 0750 root:root
    make_dir /var/lib/domovoi-agent 0700 root:root
    add_starter_skills "$prefix/share/domovoi/examples/skills"
}

# Sets $layout to nosystemd, hub, legacy, fresh or migrated.
classify_host() {
    if [ ! -d /run/systemd/system ] || ! command -v systemctl >/dev/null 2>&1; then
        layout="nosystemd"
    elif id domovoi >/dev/null 2>&1 || [ -e /etc/systemd/system/domovoi-hub.service ] ||
        [ -e /etc/domovoi/hub.toml ]; then
        layout="hub"
    elif { [ -e "$agent_unit_path" ] && ! grep -qx 'User=domovoi-agent' "$agent_unit_path"; } ||
        { [ -d /var/lib/domovoi-agent ] && [ "$(stat -c %u /var/lib/domovoi-agent)" = 0 ]; }; then
        layout="legacy"
    elif [ ! -e "$agent_unit_path" ] && [ ! -e /var/lib/domovoi-agent ]; then
        layout="fresh"
    else
        layout="migrated"
    fi
}

# The label printed whenever the agent does not run behind the boundary.
boundary_notice() {
    say ""
    say "privilege boundary not in effect: $1"
    say "  The agent keeps working as before, but this host gets no unattended execution."
}

# Prints one line for each way the account $1 is not as the layout needs; $2 lists the
# groups besides its own that it may be in. Prints nothing when the account is right.
account_problems() {
    ap_name="$1"
    ap_extra="$2"
    ap_uid="$(id -u "$ap_name")"
    ap_primary="$(id -gn "$ap_name")"
    ap_shell="$(getent passwd "$ap_name" | cut -d: -f7)"
    if [ "$ap_uid" -eq 0 ]; then
        say "its uid is 0"
    fi
    if [ "$ap_primary" != "$ap_name" ]; then
        say "its primary group is $ap_primary, not $ap_name"
    fi
    case "$ap_shell" in
    */nologin | */false) ;;
    *) say "its login shell is ${ap_shell:-empty}; it must be nologin or false" ;;
    esac
    id -Gn "$ap_name" | tr ' ' '\n' | while read -r ap_group; do
        case " $ap_name $ap_extra " in
        *" $ap_group "*) ;;
        *) say "it is in the group $ap_group, but may only be in its own${ap_extra:+ and $ap_extra}" ;;
        esac
    done
    if getent group docker | cut -d: -f4 | tr ',' '\n' | grep -qx "$ap_name"; then
        say "it is a member of the docker group, which is root on a Docker host"
    fi
}

# Step 1 of the layout. An account that exists is verified and never changed; one that is
# missing is created. Nothing is created until every existing account has been checked.
ensure_accounts() {
    command -v useradd >/dev/null 2>&1 ||
        fail "useradd is needed to create the domovoi-agent and domovoi-skill accounts"
    ea_bad=""
    for ea_name in domovoi-agent domovoi-skill; do
        if id "$ea_name" >/dev/null 2>&1; then
            ea_extra=""
            if [ "$ea_name" = "domovoi-agent" ]; then
                ea_extra="systemd-journal"
            fi
            ea_found="$(account_problems "$ea_name" "$ea_extra")"
            if [ -n "$ea_found" ]; then
                ea_bad="$ea_bad
$(printf '%s\n' "$ea_found" | sed "s/^/  $ea_name: /")"
            fi
        elif getent group "$ea_name" >/dev/null 2>&1; then
            ea_bad="$ea_bad
  $ea_name: a group of that name exists but the account does not"
        fi
    done
    [ -z "$ea_bad" ] ||
        fail "an existing account is not as the privilege boundary needs, and install.sh never modifies an account. Fix or remove it, then run again:$ea_bad"
    ea_nologin="$(command -v nologin || echo /usr/sbin/nologin)"
    if id domovoi-agent >/dev/null 2>&1; then
        say "Verified the existing domovoi-agent account; it was not changed."
    else
        if getent group systemd-journal >/dev/null 2>&1; then
            useradd --system --user-group --no-create-home --home-dir /var/lib/domovoi-agent \
                --shell "$ea_nologin" -G systemd-journal domovoi-agent
        else
            useradd --system --user-group --no-create-home --home-dir /var/lib/domovoi-agent \
                --shell "$ea_nologin" domovoi-agent
        fi
        say "Created the domovoi-agent system user."
    fi
    if id domovoi-skill >/dev/null 2>&1; then
        say "Verified the existing domovoi-skill account; it was not changed."
    else
        useradd --system --user-group --no-create-home --home-dir /nonexistent \
            --shell "$ea_nologin" domovoi-skill
        say "Created the domovoi-skill system user."
    fi
}

# Records a problem found by check_path or the tree checks.
fault() {
    fault_lines="$fault_lines
  $*"
    faults=$((faults + 1))
}

# Compares one path with the layout: KIND (directory or file), PATH, MODE, OWNER:GROUP. A
# symbolic link is always a problem. Reads only; nothing is changed.
check_path() {
    cp_kind="$1"
    cp_path="$2"
    cp_mode="$3"
    cp_owner="$4"
    cp_want="expected mode $cp_mode owner $cp_owner"
    if [ -L "$cp_path" ]; then
        fault "$cp_path: found a symbolic link, $cp_want"
    elif [ ! -e "$cp_path" ]; then
        fault "$cp_path: missing, $cp_want"
    elif [ "$cp_kind" = "directory" ] && [ ! -d "$cp_path" ]; then
        fault "$cp_path: found something that is not a directory, $cp_want"
    elif [ "$cp_kind" = "file" ] && [ ! -f "$cp_path" ]; then
        fault "$cp_path: found something that is not a regular file, $cp_want"
    else
        cp_found="$(stat -c '%a %U:%G' "$cp_path")"
        if [ "$cp_found" != "$cp_mode $cp_owner" ]; then
            fault "$cp_path: found mode ${cp_found%% *} owner ${cp_found#* }, $cp_want"
        fi
    fi
}

# Checks every path of the three-identities layout and sets $faults and $fault_lines.
# Returns non-zero when anything is off. Never changes a path.
verify_layout() {
    faults=0
    fault_lines=""
    check_path directory /etc/domovoi 750 root:domovoi-agent
    check_path directory /etc/domovoi/skills 755 root:root
    check_path file /etc/domovoi/helper.toml 600 root:root
    if [ -e /etc/domovoi/agent.toml ] || [ -L /etc/domovoi/agent.toml ]; then
        check_path file /etc/domovoi/agent.toml 644 root:root
    fi
    check_path directory /var/lib/domovoi-agent 700 domovoi-agent:domovoi-agent
    check_path directory /var/lib/domovoi-packages 755 domovoi-agent:domovoi-agent
    if [ "${runner_skills_dir:-/etc/domovoi/skills}" != "/etc/domovoi/skills" ]; then
        check_path directory "$runner_skills_dir" 755 root:root
    fi
    if [ "${runner_packages_dir:-/var/lib/domovoi-packages}" != "/var/lib/domovoi-packages" ]; then
        check_path directory "$runner_packages_dir" 755 domovoi-agent:domovoi-agent
    fi
    check_path directory /var/lib/domovoi-helper 700 root:root
    check_path file /var/lib/domovoi-helper/state.lock 600 root:root
    for vl_name in attest.key state platform.json; do
        if [ -e "/var/lib/domovoi-helper/$vl_name" ] || [ -L "/var/lib/domovoi-helper/$vl_name" ]; then
            check_path file "/var/lib/domovoi-helper/$vl_name" 600 root:root
        fi
    done
    for vl_dir in /etc/domovoi/skills /var/lib/domovoi-packages \
        "${runner_skills_dir:-/etc/domovoi/skills}" "${runner_packages_dir:-/var/lib/domovoi-packages}"; do
        if [ -d "$vl_dir" ] && [ ! -L "$vl_dir" ]; then
            vl_bad="$(find "$vl_dir" ! -type l \( -perm -020 -o -perm -002 \) -print | head -n 1)"
            if [ -n "$vl_bad" ]; then
                fault "$vl_bad: has a group or other write bit, which nothing under $vl_dir may have"
            fi
        fi
    done
    if [ -d /etc/domovoi/skills ] && [ ! -L /etc/domovoi/skills ]; then
        vl_bad="$(find /etc/domovoi/skills ! -user root -print | head -n 1)"
        if [ -n "$vl_bad" ]; then
            fault "$vl_bad: is not owned by root, which everything under /etc/domovoi/skills must be"
        fi
    fi
    [ "$faults" -eq 0 ]
}

# Writes /etc/domovoi/helper.toml, only when it is absent.
write_helper_toml() {
    if [ ! -e /etc/domovoi/helper.toml ] && [ ! -L /etc/domovoi/helper.toml ]; then
        helper_toml >"$tmp/helper.toml"
        install -m 0600 -o root -g root "$tmp/helper.toml" /etc/domovoi/helper.toml
        say "Created /etc/domovoi/helper.toml (read groups only; no write group is enabled)"
    fi
}

# Installs one unit: NAME, the function that prints it, and FORCE (1 replaces a different
# unit that is already there; 0 keeps it and saves the new text beside the examples).
# ExecStart points at this --prefix.
place_unit() {
    pu_name="$1"
    pu_func="$2"
    pu_force="$3"
    pu_path="/etc/systemd/system/$pu_name"
    "$pu_func" | sed "s|/usr/local/bin/domovoi|$bin|" >"$tmp/$pu_name"
    if [ ! -e "$pu_path" ]; then
        mkdir -p /etc/systemd/system
        install -m 0644 "$tmp/$pu_name" "$pu_path"
        say "Installed $pu_path"
    elif cmp -s "$tmp/$pu_name" "$pu_path"; then
        say "$pu_path is up to date."
    elif [ "$pu_force" = "1" ]; then
        install -m 0644 "$tmp/$pu_name" "$pu_path"
        say "Replaced $pu_path; the old one is saved in $record_dir"
    else
        say "Kept $pu_path: it differs from this release's unit. The new one is"
        say "  $prefix/share/domovoi/$pu_name"
        mkdir -p "$prefix/share/domovoi"
        install -m 0644 "$tmp/$pu_name" "$prefix/share/domovoi/$pu_name"
        if [ "$pu_name" = "domovoi-helper@.service" ]; then
            say "  Replace the helper unit with it, then run systemctl daemon-reload: SMART, kernel-log"
            say "  and listening-port reads need its wider device and capability settings. Docker reads"
            say "  work without the change."
        fi
    fi
}

# The conversion rules of --migrate-identity: the only place that changes the owner or
# mode of a path that already exists.
convert_legacy_paths() {
    chgrp domovoi-agent /etc/domovoi
    chmod 0755 /etc/domovoi/skills
    normalise_tree /etc/domovoi/skills
    if [ -e /etc/domovoi/agent.toml ]; then
        chmod 0644 /etc/domovoi/agent.toml
    fi
    chown -R -h domovoi-agent:domovoi-agent /var/lib/domovoi-agent /var/lib/domovoi-packages
}

# Fails when the binary would sit where the skill runner cannot execute it: ProtectHome=yes
# hides /home and /root from the runner, and /run, /tmp and /dev/shm may be mounted noexec.
runner_prefix_ok() {
    case "$prefix" in
    /home | /home/* | /root | /root/* | /run | /run/* | /tmp | /tmp/* | /dev/shm | /dev/shm/*)
        fail "--prefix $prefix is not usable for the skill runner: its unit hides /home and /root, and /run and /tmp may be mounted noexec. Use a prefix such as /usr/local, or pass --no-service"
        ;;
    esac
}

# Sets $runner_skills_dir and $runner_packages_dir from the agent's own configuration, so the
# runner unit names the same directories the agent uses. Stops the install, before anything
# is created or replaced, when the binary refuses a path (a unit cannot hold it safely).
runner_dirs() {
    rd_out="$("$bin" agent dirs)" ||
        fail "could not read the skills and packages directories from the agent configuration ($bin agent dirs failed); nothing was changed"
    runner_skills_dir="$(printf '%s\n' "$rd_out" | sed -n 's/^skills_dir=//p')"
    runner_packages_dir="$(printf '%s\n' "$rd_out" | sed -n 's/^packages_dir=//p')"
    if [ -z "$runner_skills_dir" ] || [ -z "$runner_packages_dir" ]; then
        fail "$bin agent dirs did not print skills_dir and packages_dir; nothing was changed"
    fi
    # Configured directories join the created-set, so they are made before any socket starts.
    CREATED_PATHS="$CREATED_PATHS $runner_skills_dir $runner_packages_dir"
}

# Names the self-test checks that failed in the report $1, for the failure message.
failed_checks() {
    sed -n 's/^\(ST-[0-9][0-9]*\) fail.*/\1/p' "$1" | tr '\n' ' '
}

# The three-identities install. MODE is fresh, migrated (a rerun on a host that already has
# the layout) or migrate (the conversion of a root agent). Steps, in order: the runner's
# directories, accounts, directories and helper.toml, helper init, the five units, the
# helper socket, the self-test, and only after it passes the skill runner socket. These
# two sockets are the only services this starts.
install_identities() {
    ii_mode="$1"
    runner_prefix_ok
    runner_dirs
    ensure_accounts
    make_dir /etc/domovoi 0750 root:domovoi-agent
    make_dir /etc/domovoi/skills 0755 root:root
    make_dir /var/lib/domovoi-agent 0700 domovoi-agent:domovoi-agent
    make_dir /var/lib/domovoi-packages 0755 domovoi-agent:domovoi-agent
    # Directories the agent configuration moved elsewhere; the defaults above stay either way.
    make_dir "$runner_skills_dir" 0755 root:root
    make_dir "$runner_packages_dir" 0755 domovoi-agent:domovoi-agent
    if [ "$ii_mode" = "migrate" ]; then
        convert_legacy_paths
    fi
    write_helper_toml
    add_starter_skills "$prefix/share/domovoi/examples/skills" normalise
    make_dir /var/lib/domovoi-helper 0700 root:root
    ii_init=1
    "$bin" helper init || ii_init=0 # adds state.lock and refuses a directory that is not as expected
    if ! verify_layout || [ "$ii_init" -eq 0 ]; then
        say ""
        say "The layout is not as the privilege boundary needs. install.sh never repairs a path:"
        printf '%s\n' "$fault_lines"
        say ""
        say "Fix each path by hand, or run: sudo $bin helper repair"
        fail "stopped before enabling the helper socket; no mode or owner was changed"
    fi
    for ii_path in $CREATED_PATHS; do
        # The hub's data directory belongs to the hub role.
        [ "$ii_path" != "/var/lib/domovoi" ] || continue
        [ -d "$ii_path" ] || fail "$ii_path was not created"
    done
    place_unit domovoi-helper.socket helper_socket_unit 0
    place_unit domovoi-helper@.service helper_service_unit 0
    place_unit domovoi-helper-selftest.service helper_selftest_unit 0
    place_unit domovoi-skill-runner.socket runner_socket_unit 0
    place_unit domovoi-skill-runner@.service render_runner_service 0
    if [ "$ii_mode" = "migrate" ]; then
        place_unit domovoi-agent.service agent_unit 1
    else
        place_unit domovoi-agent.service agent_unit 0
    fi
    systemctl daemon-reload
    start_sockets
}

# Starts the two sockets of the three-identities install. The helper socket starts first and
# the self-test judges it; the skill runner socket starts only after that check has passed,
# when its units are installed and the daemon has reloaded. The check runs once more with
# the runner socket up, so the runner's own start checks and isolation probe report real
# results; a failure there stops the socket again and names the check. This function stays
# textually after the code that creates the directories and places the units.
start_sockets() {
    # The leak self-test judges the firewall write group. It takes minutes, so it starts without
    # blocking; the helper socket is ordered after it. A failure leaves the firewall group
    # refused and never stops the install.
    systemctl enable domovoi-helper-selftest.service || true
    systemctl start --no-block domovoi-helper-selftest.service || say "The firewall self-test did not start; run: sudo $bin helper nft-selftest"
    systemctl enable --now domovoi-helper.socket
    say "Started domovoi-helper.socket"
    if "$bin" agent selftest --installer --record >"$tmp/selftest.out" 2>&1; then
        cat "$tmp/selftest.out"
    else
        cat "$tmp/selftest.out"
        systemctl disable --now domovoi-helper.socket || true
        ss_failed="$(failed_checks "$tmp/selftest.out")"
        fail "the self-test failed (${ss_failed:-see the report above}); the helper socket is disabled, the skill runner socket was not started and nothing else was started"
    fi
    systemctl enable --now domovoi-skill-runner.socket
    say "Started domovoi-skill-runner.socket"
    if "$bin" agent selftest --installer >"$tmp/runner-selftest.out" 2>&1; then
        cat "$tmp/runner-selftest.out"
    else
        cat "$tmp/runner-selftest.out"
        systemctl disable --now domovoi-skill-runner.socket || true
        ss_failed="$(failed_checks "$tmp/runner-selftest.out")"
        fail "the self-test failed with the skill runner socket up (${ss_failed:-see the report above}); the skill runner socket is disabled, so skills stay unavailable, and the helper socket keeps running"
    fi
}

# Prints one line for a path --migrate-identity will create or change.
plan_path() {
    pp_path="$1"
    pp_mode="$2"
    pp_owner="$3"
    pp_note="${4:-}"
    if [ ! -e "$pp_path" ]; then
        say "  $pp_path: created, mode $pp_mode owner $pp_owner"
    else
        pp_found="$(stat -c '%a %U:%G' "$pp_path")"
        if [ "$pp_found" != "$pp_mode $pp_owner" ]; then
            say "  $pp_path: mode ${pp_found%% *} owner ${pp_found#* } -> mode $pp_mode owner $pp_owner$pp_note"
        fi
    fi
}

# Stops the migration before anything changes when the host cannot be migrated safely.
migrate_preconditions() {
    classify_host
    case "$layout" in
    legacy) ;;
    nosystemd) fail "--migrate-identity needs a systemd host" ;;
    hub) fail "--migrate-identity cannot run here: the hub is installed on this host and shares /etc/domovoi (root:domovoi) with the agent, which the three-identities layout cannot satisfy" ;;
    fresh) fail "nothing to migrate: no agent is installed here. Run install.sh --role agent for a new install" ;;
    *) fail "nothing to migrate: the agent already runs as domovoi-agent. Check it with: $bin agent selftest" ;;
    esac
    runner_prefix_ok
    [ -e "$agent_unit_path" ] ||
        fail "there is no agent unit at $agent_unit_path to migrate; install the agent first"
    for mp_file in "$agent_unit_path.d"/*.conf; do
        [ -f "$mp_file" ] || continue
        if grep -Eq '^[[:space:]]*(User|Group|SupplementaryGroups)=' "$mp_file"; then
            fail "$mp_file sets User=, Group= or SupplementaryGroups= on the agent unit. Remove it (this installer never deletes your drop-ins) and run the migration again. An agent run as another user stays 'privilege boundary not in effect'"
        fi
    done
    for mp_dir in /var/lib/domovoi-agent /var/lib/domovoi-packages; do
        [ -e "$mp_dir" ] || continue
        if [ -L "$mp_dir" ] || [ ! -d "$mp_dir" ]; then
            fail "$mp_dir is not a plain directory"
        fi
        mp_link="$(find "$mp_dir" -type l -print | head -n 1)"
        [ -z "$mp_link" ] || fail "$mp_link is a symbolic link inside $mp_dir; remove it and run the migration again"
        mp_owner="$(stat -c %u "$mp_dir")"
        mp_other="$(find "$mp_dir" ! -uid "$mp_owner" -print | head -n 1)"
        [ -z "$mp_other" ] ||
            fail "$mp_other inside $mp_dir is owned by a different user than $mp_dir itself; fix it and run the migration again"
    done
}

# Prints everything --migrate-identity will do.
migrate_plan() {
    say "Moving the agent on this host from root to the domovoi-agent account."
    say ""
    say "Accounts:"
    for mpl_name in domovoi-agent domovoi-skill; do
        if id "$mpl_name" >/dev/null 2>&1; then
            say "  $mpl_name: exists; it will be checked and not changed"
        else
            say "  $mpl_name: will be created"
        fi
    done
    say ""
    say "Paths that change (found -> expected):"
    plan_path /etc/domovoi 750 root:domovoi-agent
    plan_path /etc/domovoi/skills 755 root:root " (and the tree under it is normalised: root-owned, no group or other write bit)"
    plan_path /etc/domovoi/agent.toml 644 root:root
    plan_path /var/lib/domovoi-agent 700 domovoi-agent:domovoi-agent " (and everything inside it)"
    plan_path /var/lib/domovoi-packages 755 domovoi-agent:domovoi-agent " (and everything inside it)"
    plan_path /var/lib/domovoi-helper 700 root:root
    plan_path /etc/domovoi/helper.toml 600 root:root
    say ""
    say "The agent unit is replaced; your edits to it are lost, and the old one is saved in"
    say "$record_dir. Difference:"
    agent_unit | sed "s|/usr/local/bin/domovoi|$bin|" >"$tmp/new-agent.service"
    if command -v diff >/dev/null 2>&1; then
        diff -u "$agent_unit_path" "$tmp/new-agent.service" || true
    else
        say "  (diff is not installed; the new unit is the one printed by --print-unit agent)"
    fi
    say ""
    say "What stops working:"
    say "  Docker, SMART, the kernel-log fallback and the owning process names of listening ports"
    say "  are read through the helper and keep working."
    say "  The check security.fail2ban reads a root-only socket and reports an error until a later"
    say "  release."
    say "  Skills run as domovoi-skill from now on. Skills whose manifest names a run_as user have"
    say "  no runner until the skill runner is installed."
    say ""
    say "Nothing is deleted. To undo this later: sudo sh install.sh --rollback-identity"
    say ""
}

# Asks for the typed word on the terminal. There is no --yes.
migrate_confirm() {
    ( : </dev/tty ) 2>/dev/null ||
        fail "--migrate-identity asks for confirmation on the terminal and has no --yes; run it from an interactive shell"
    printf 'Type migrate to continue, anything else to stop: ' >/dev/tty
    IFS= read -r mc_answer </dev/tty || mc_answer=""
    [ "$mc_answer" = "migrate" ] || fail "not confirmed; nothing was changed"
}

# Waits up to 30 seconds for the converted agent to be active and for the self-test to
# pass ST-01 and ST-02 for the domovoi-agent account.
wait_for_agent() {
    wa_i=0
    while [ "$wa_i" -lt 30 ]; do
        if systemctl is-active --quiet domovoi-agent; then
            "$bin" agent selftest --installer >"$tmp/wait.out" 2>&1 || true
            if grep -q '^ST-01 pass' "$tmp/wait.out" && grep -q '^ST-02 pass' "$tmp/wait.out"; then
                return 0
            fi
        fi
        wa_i=$((wa_i + 1))
        sleep 1
    done
    cat "$tmp/wait.out" 2>/dev/null || true
    return 1
}

# Puts back what --migrate-identity saved: the legacy unit and the owner, group and mode of
# every recorded path. The accounts, the helper directory, helper.toml and the helper units
# stay, unused, and are listed. The record is renamed, never deleted.
rollback_apply() {
    ra_unit="$record_dir/legacy-domovoi-agent.service"
    [ -f "$ra_unit" ] || fail "there is no migration record in $record_dir, so there is nothing to roll back"
    systemctl stop domovoi-agent || true
    install -m 0644 -o root -g root "$ra_unit" "$agent_unit_path"
    while IFS='|' read -r ra_path ra_uid ra_gid ra_mode; do
        [ -n "$ra_path" ] || continue
        case "$ra_path" in
        /var/lib/domovoi-agent | /var/lib/domovoi-packages) chown -R -h "$ra_uid:$ra_gid" "$ra_path" ;;
        *) chown -h "$ra_uid:$ra_gid" "$ra_path" ;;
        esac
        chmod "$ra_mode" "$ra_path"
    done <"$record_dir/owners"
    systemctl disable --now domovoi-helper.socket || true
    systemctl disable --now domovoi-skill-runner.socket || true
    systemctl daemon-reload
    if [ "$(cat "$record_dir/was-active" 2>/dev/null || echo 0)" = "1" ]; then
        systemctl start domovoi-agent || warn "the legacy agent did not start; check: systemctl status domovoi-agent"
    fi
    ra_saved="$record_dir.rolled-back-$(date +%s)"
    mv "$record_dir" "$ra_saved"
    say "Rolled back: the legacy agent unit and the recorded owners and modes are restored."
    say "These stay, unused and never deleted automatically: the domovoi-agent and"
    say "domovoi-skill accounts, /var/lib/domovoi-helper, /etc/domovoi/helper.toml,"
    say "/var/lib/domovoi-packages and the helper and skill runner units. The record is now in $ra_saved"
}

# The migration itself, after the owner confirmed and the new binary is installed.
migrate_apply() {
    [ ! -e "$record_dir/legacy-domovoi-agent.service" ] ||
        fail "$record_dir already holds a migration record; run install.sh --rollback-identity first"
    mkdir -p "$record_dir"
    chmod 0700 "$record_dir"
    cp -p "$agent_unit_path" "$record_dir/legacy-domovoi-agent.service"
    : >"$record_dir/owners"
    for ma_path in /etc/domovoi /etc/domovoi/skills /etc/domovoi/agent.toml /var/lib/domovoi-agent /var/lib/domovoi-packages; do
        [ -e "$ma_path" ] || continue
        printf '%s|%s|%s|%s\n' "$ma_path" "$(stat -c %u "$ma_path")" "$(stat -c %g "$ma_path")" \
            "$(stat -c %a "$ma_path")" >>"$record_dir/owners"
    done
    if systemctl is-active --quiet domovoi-agent; then
        echo 1 >"$record_dir/was-active"
    else
        echo 0 >"$record_dir/was-active"
    fi
    say "Saved the legacy unit and the owners in $record_dir"
    systemctl stop domovoi-agent || true
    ma_i=0
    while systemctl is-active --quiet domovoi-agent && [ "$ma_i" -lt 30 ]; do
        ma_i=$((ma_i + 1))
        sleep 1
    done
    set +e
    (
        set -e
        install_identities migrate
    )
    ma_rc=$?
    set -e
    if [ "$ma_rc" -ne 0 ]; then
        say ""
        say "The conversion failed; rolling back."
        rollback_apply
        fail "the migration failed and was rolled back; the legacy agent is as it was"
    fi
    if [ "$(cat "$record_dir/was-active")" = "1" ]; then
        if ! systemctl start domovoi-agent || ! wait_for_agent; then
            say ""
            say "The converted agent did not become active and pass ST-01 and ST-02; rolling back."
            rollback_apply
            fail "the migration failed and was rolled back; the legacy agent is as it was"
        fi
    fi
    say ""
    say "Migrated. The agent now runs as domovoi-agent behind the privilege boundary."
    say "  Saved legacy state: $record_dir (kept)"
    say "  To undo: sudo sh install.sh --rollback-identity"
    say "  Enroll or re-enroll with sudo; the files are handed to domovoi-agent automatically."
    say "  Skills now run as domovoi-skill through the skill runner socket."
}

role=""
version="latest"
prefix="/usr/local"
service=1
migrate=0
rollback=0
layout=""
upgrade=0

while [ $# -gt 0 ]; do
    case "$1" in
    --role)
        [ $# -ge 2 ] || fail "--role needs hub or agent"
        role="$2"
        shift 2
        ;;
    --role=*)
        role="${1#--role=}"
        shift
        ;;
    --version)
        [ $# -ge 2 ] || fail "--version needs a tag such as v0.1.0"
        version="$2"
        shift 2
        ;;
    --version=*)
        version="${1#--version=}"
        shift
        ;;
    --prefix)
        [ $# -ge 2 ] || fail "--prefix needs a directory"
        prefix="$2"
        shift 2
        ;;
    --prefix=*)
        prefix="${1#--prefix=}"
        shift
        ;;
    --no-service)
        service=0
        shift
        ;;
    --migrate-identity)
        migrate=1
        shift
        ;;
    --rollback-identity)
        rollback=1
        shift
        ;;
    --print-unit)
        [ $# -ge 2 ] || fail "--print-unit needs hub, agent, helper-socket, helper-service, runner-socket, runner-service or agent-legacy"
        case "$2" in
        hub) hub_unit ;;
        agent) agent_unit ;;
        helper-socket) helper_socket_unit ;;
        helper-service) helper_service_unit ;;
        runner-socket) runner_socket_unit ;;
        runner-service) runner_service_unit ;;
        agent-legacy) legacy_agent_unit ;;
        *) fail "--print-unit takes hub, agent, helper-socket, helper-service, runner-socket, runner-service or agent-legacy, not $2" ;;
        esac
        exit 0
        ;;
    -h | --help)
        usage
        exit 0
        ;;
    *)
        usage >&2
        fail "unknown option: $1"
        ;;
    esac
done

if [ "$rollback" -eq 1 ]; then
    [ "$migrate" -eq 0 ] || fail "--rollback-identity and --migrate-identity cannot be used together"
    [ "$(id -u)" -eq 0 ] || fail "--rollback-identity must run as root (sudo)"
    if [ ! -d /run/systemd/system ] || ! command -v systemctl >/dev/null 2>&1; then
        fail "--rollback-identity needs a systemd host"
    fi
    rollback_apply
    exit 0
fi

case "$role" in
hub | agent) ;;
"")
    usage >&2
    fail "--role is required"
    ;;
*) fail "--role takes hub or agent, not $role" ;;
esac

if [ "$migrate" -eq 1 ]; then
    [ "$role" = "agent" ] || fail "--migrate-identity goes with --role agent"
    [ "$service" -eq 1 ] || fail "--migrate-identity cannot be combined with --no-service"
fi

case "$version" in
latest | v*) ;;
*) version="v$version" ;;
esac

bin_dir="$prefix/bin"
bin="$bin_dir/domovoi"

if [ "$(id -u)" -ne 0 ]; then
    if [ "$service" -eq 1 ]; then
        fail "run as root (sudo), or pass --no-service with a --prefix you can write to"
    fi
    mkdir -p "$bin_dir" 2>/dev/null || true
    [ -w "$bin_dir" ] || fail "cannot write to $bin_dir; run as root or choose another --prefix"
fi

# The release archive for this machine.
case "$(uname -s)" in
Linux) ;;
*) fail "only Linux is supported; this is $(uname -s)" ;;
esac
case "$(uname -m)" in
x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
*) fail "no release build for $(uname -m); build from source instead" ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL --proto '=https,http' -o "$2" "$1"; }
    resolve_latest() { curl -fsSLI -o /dev/null -w '%{url_effective}' "$RELEASE_BASE/latest"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q -O "$2" "$1"; }
    resolve_latest() {
        wget -S --spider "$RELEASE_BASE/latest" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1 | tr -d '\r'
    }
else
    fail "curl or wget is needed"
fi
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is needed"
command -v tar >/dev/null 2>&1 || fail "tar is needed"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' INT TERM

# The migration checks the host, shows its plan and asks before anything is downloaded
# or changed.
if [ "$migrate" -eq 1 ]; then
    migrate_preconditions
    migrate_plan
    migrate_confirm
fi

if [ "$version" = "latest" ]; then
    url="$(resolve_latest)" || fail "could not find the latest release at $RELEASE_BASE/latest"
    url="${url%/}"
    version="${url##*/}"
    case "$version" in
    v*) ;;
    *) fail "could not find the latest release at $RELEASE_BASE/latest (got $url)" ;;
    esac
fi

name="domovoi-$version-$target"

say "Downloading $name.tar.gz"
fetch "$RELEASE_BASE/download/$version/$name.tar.gz" "$tmp/$name.tar.gz" ||
    fail "could not download $RELEASE_BASE/download/$version/$name.tar.gz"
fetch "$RELEASE_BASE/download/$version/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" ||
    fail "could not download the checksum for $name.tar.gz"
(cd "$tmp" && sha256sum -c "$name.tar.gz.sha256" >/dev/null 2>&1) ||
    fail "the SHA-256 of $name.tar.gz does not match its .sha256 file; nothing was installed"
say "Checksum verified. To check where the archive was built, download it and run"
say "  gh attestation verify $name.tar.gz --repo $REPO"

tar -C "$tmp" -xzf "$tmp/$name.tar.gz"
[ -f "$tmp/$name/domovoi" ] || fail "$name.tar.gz has no domovoi binary"
"$tmp/$name/domovoi" version >/dev/null 2>&1 || fail "the downloaded binary does not run on this machine"

mkdir -p "$bin_dir"
install -m 0755 "$tmp/$name/domovoi" "$bin"
say "Installed $("$bin" version | head -n 1) to $bin"

if [ -d "$tmp/$name/examples" ]; then
    examples="$prefix/share/domovoi/examples"
    mkdir -p "$examples"
    cp -R "$tmp/$name/examples/." "$examples/"
fi

if [ "$service" -eq 0 ]; then
    say "Skipped the user, directories and systemd unit (--no-service)."
    exit 0
fi

if [ "$migrate" -eq 1 ]; then
    migrate_apply
    exit 0
fi

unit_name="domovoi-$role.service"
if [ -e "/etc/systemd/system/$unit_name" ]; then
    upgrade=1
fi

if [ "$role" = "hub" ]; then
    create_hub_layout
    place_unit "$unit_name" hub_unit 0
    if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
        systemctl daemon-reload
    fi
else
    classify_host
    case "$layout" in
    fresh | migrated) install_identities "$layout" ;;
    *)
        create_legacy_agent_layout
        place_unit "$unit_name" legacy_agent_unit 0
        if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
            systemctl daemon-reload
        fi
        ;;
    esac
fi

case "$layout" in
nosystemd) boundary_notice "no systemd on this host" ;;
hub) boundary_notice "the hub and the agent share /etc/domovoi (root:domovoi), which the three-identities layout cannot satisfy; run the agent on its own host to get the boundary" ;;
legacy)
    boundary_notice "this agent still runs as root"
    say "  To run it as the unprivileged domovoi-agent account instead, run:"
    say "    sudo sh install.sh --role agent --migrate-identity"
    ;;
esac

say ""
say "Next steps:"
if [ "$upgrade" -eq 1 ]; then
    say "  systemctl restart domovoi-$role    # runs the new binary; config and data are kept"
elif [ "$role" = "hub" ]; then
    say "  systemctl enable --now domovoi-hub"
    say "  journalctl -u domovoi-hub | grep setup     # open this link to create the admin account"
    say "  sudo -u domovoi $bin selftest --offline    # optional: checks the pipeline end to end"
    say ""
    say "No config file is needed: everything is set in the web UI."
elif [ "$layout" = "fresh" ] || [ "$layout" = "migrated" ]; then
    say "  1. In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here,"
    say "     with sudo: the files it writes are handed to the domovoi-agent account automatically."
    say "  2. systemctl enable --now domovoi-agent"
    say ""
    say "The helper and skill runner sockets are running. After a runner failure, clear it with:"
    say "  systemctl reset-failed 'domovoi-skill-runner@*'"
else
    say "  1. In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here."
    say "  2. systemctl enable --now domovoi-agent"
fi
