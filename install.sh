#!/bin/sh
# Installs Domovoi from a GitHub release.
#
#   curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
#
# It downloads the release archive for this machine, checks its SHA-256, installs the
# binary, and for --role hub, agent or both creates the users, directories and systemd
# units. A hub on a systemd host is enabled and started, and the installer waits until it is
# ready (--no-start skips that). For the hub it can write /etc/domovoi/hub.env (only the keys
# named by --public-url, --bind and --agent-url) and create the admin account
# (--admin-password-file).
#
# --role both is for the machine that runs the hub and should also be watched by it: it
# installs the hub exactly as --role hub does and starts it (which creates its TLS
# certificate), stops it for a moment to make a 15 minute enrollment token with
# `domovoi hub token create` as the hub user (the command needs the database), starts it
# again, then enrolls and starts an agent on wss://127.0.0.1:7421 and waits until it is
# connected. The agent keeps the root layout a host with a hub gets. The token is held in a
# file in the installer's private temporary directory, removed when the script ends, and is
# never printed, logged or put in an argument list.
#
# An agent is started only when it is enrolled in the same run: --hub, --fingerprint and a
# token (--token-file FILE or DOMOVOI_ENROLL_TOKEN; never a flag value, so it stays out of
# the shell history and the process list) make the installer run `domovoi agent enroll`,
# enable the unit and wait until `domovoi agent status` shows the link connected. Without
# them the agent is not started; the only things started for it are the helper and skill
# runner sockets on a systemd host, the second one only after the self-test passes. For an
# agent it writes exactly one config file, /etc/domovoi/helper.toml, and only when it is
# absent (enrolling writes agent.toml). Running it again upgrades the binary and leaves
# everything else alone.
#
# The 0.1.0 release replaces the whole database schema, so a hub database written by an
# older release cannot be opened by it. Installing 0.1.0 or later over a hub that has such a
# database is refused (exit 10) until --accept-data-reset says the old data is to be moved
# aside by hand; the installer never moves or deletes it.
#
# An agent on a systemd host with no hub gets three identities: the unprivileged
# domovoi-agent account runs the agent, the group-less domovoi-skill account is for skills,
# and root runs the sandboxed helper behind a socket. The installer creates only what is
# absent, never changes an account or a mode it finds, and reports what is wrong instead.
# An agent that already runs as root keeps running as root until the owner runs
# --migrate-identity.
#
# A fresh install that fails before anything is started is undone: every account, directory,
# file and unit that this run created is removed again, newest first, and nothing that was
# there before is touched (--keep-on-failure turns that off). An upgrade, a migration and a
# failure after the hub, the agent or a helper socket was started are never rolled back.
#
# --uninstall takes the installation off the machine: it stops and disables every Domovoi
# unit, removes the unit files, the binary and DIR/share/domovoi, and keeps the configuration,
# the data, the skills, the accounts and the identity-migration record, saying where they are.
# --purge adds the removal of those too, after a typed confirmation or --yes.
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
agent_toml="/etc/domovoi/agent.toml"
record_dir="/etc/domovoi/identity-migration"

usage() {
    cat <<'USAGE'
Usage: install.sh --role hub|agent|both [options]
       install.sh --uninstall [--purge] [--yes] [--prefix DIR] [--dry-run] [--json]
       install.sh --rollback-identity

Options:
  --role hub|agent|both
                       what this machine runs (required). both sets up the hub and an agent
                       that watches this same machine, and enrolls it: the hub is started,
                       stopped for a moment to make a 15 minute enrollment token for the agent,
                       started again, and the agent is enrolled, started and waited for. All the
                       hub flags apply, --name names the host, and --no-start is refused
  --version vX.Y.Z     the release to install (default: the latest)
  --prefix DIR         where the binary goes, as DIR/bin/domovoi (default: /usr/local);
                       an absolute path
  --no-service         install only the binary and examples: no user, directories or unit
  --no-start           with --role hub: install the unit but do not start the hub. Without it
                       a hub on a systemd host is enabled, started, and waited for until
                       GET /readyz answers; an upgrade restarts a hub that was running
  --public-url URL     with --role hub: the exact address people open, such as
                       https://domovoi.lan or http://192.168.1.50:7420 (writes
                       DOMOVOI_WEB__PUBLIC_URL to /etc/domovoi/hub.env)
  --bind lan|loopback|HOST:PORT
                       with --role hub: where the web UI listens (DOMOVOI_WEB__BIND). lan is
                       0.0.0.0:7420 and, without --public-url, derives
                       http://<this machine's IPv4>:7420; loopback is 127.0.0.1:7420
  --agent-url URL      with --role hub or both: the wss://HOST:7421 address agents dial
                       (DOMOVOI_AGENT_SERVER__PUBLIC_URL); with --bind lan it defaults to the
                       host of the public URL. With --role both the agent of this machine dials
                       it too (default: wss://127.0.0.1:7421)
  --admin-password-file FILE
                       with --role hub, on a fresh install only: create the admin account
                       before the first start from the first line of FILE (a regular file,
                       mode 0600 or 0400, owned by you; at least 12 characters), so no setup
                       link is needed. An existing admin or an upgrade is never changed
  --admin-password-stdin
                       the same, reading the first line of standard input (not with curl | sh)
  --hub wss://HOST[:PORT]
                       with --role agent: the hub's agent server address. With --fingerprint and
                       a token, the installer enrolls the agent, starts it and waits until it
                       is connected. The three go together or not at all
  --fingerprint FP     with --role agent: the hub certificate's SHA-256 as `hub token create`
                       or the Add host dialog prints it (64 hex digits, colons allowed)
  --token-file FILE    with --role agent: the enrollment token, read from the first line of FILE
                       (a regular file, mode 0600 or 0400, owned by you). The token can also
                       come from DOMOVOI_ENROLL_TOKEN. A token is never taken from a flag:
                       --token VALUE is refused, because it would land in the shell history
                       and the process list
  --name NAME          with --role agent or both: the host's name in Domovoi, as in the Add host dialog
                       (lower-case letters, digits and dashes; default: this machine's short
                       host name, cleaned up)
  --accept-data-reset  with --role hub or both: go on although this hub's database is from before
                       0.1.0, which cannot be upgraded (0.1.0 replaces the database schema).
                       The old data directory is left untouched and the new hub refuses to open
                       it, so the installer does not restart the hub and prints how to move the
                       directory aside and set the hub up again
  --yes                never ask a question (also DOMOVOI_NONINTERACTIVE=1). The typed word
                       of --migrate-identity is not skipped by it: that step needs a terminal
                       by design. The typed word of --purge is, because --purge is explicit
  --dry-run            run the preflight, print every file, user, unit and directory the
                       install would create or change, and exit 0 without changing anything
                       or downloading the archive (the latest version is still looked up)
  --json               machine mode: stdout carries only JSON lines, one per step event and
                       one final {"event":"result",...} object; the text goes to the log file
  --quiet              print errors and the final summary only
  --verbose            print the details of each step
  --force-downgrade    allow installing an older version than the one found
  --keep-on-failure    do not undo a failed fresh install: leave what it created in place, for
                       debugging. Without it, a fresh install that fails before the first
                       service is started removes everything that run created (accounts,
                       directories, files, units, the binary) and prints "Rolled back"
  --uninstall          remove what the installer put in place (--role is not used): stop and
                       disable the hub, agent, helper and skill runner units, delete the unit
                       files, DIR/bin/domovoi and DIR/share/domovoi. The configuration
                       (/etc/domovoi), the data (/var/lib/domovoi*), the skills, the accounts
                       and the identity-migration record are KEPT, and the summary lists them.
                       Works with --prefix, --dry-run, --json, --quiet and --yes
  --purge              with --uninstall: also delete /etc/domovoi, /var/lib/domovoi,
                       /var/lib/domovoi-agent, /var/lib/domovoi-packages, /var/lib/domovoi-helper,
                       the install log, any skills or packages directory this installer made
                       elsewhere, the systemd drop-ins /etc/systemd/system/domovoi-*.d and the
                       domovoi, domovoi-agent and domovoi-skill accounts and groups, but only
                       the accounts the installer created (it records them in
                       /etc/domovoi/installer-created; without that record, only an account
                       that looks like a system account). Destructive: it lists what it will
                       delete with the data sizes, then needs --yes or the word purge typed on
                       the terminal
  --print-unit NAME    print a systemd unit and exit: hub, agent, helper-socket,
                       helper-service, runner-socket, runner-service, or agent-legacy
                       (the root agent unit used where the privilege boundary cannot be
                       set up)
  --migrate-identity   with --role agent: move an agent that runs as root to the
                       domovoi-agent account; shows a plan and asks you to type "migrate"
  --rollback-identity  undo --migrate-identity from the record it saved
  -h, --help           show this help

Environment:
  DOMOVOI_RELEASE_BASE     download from this base URL instead of GitHub releases; plain
                           http is refused unless it is loopback or DOMOVOI_ALLOW_HTTP=1
  DOMOVOI_ALLOW_HTTP=1     allow a plain-http DOMOVOI_RELEASE_BASE
  DOMOVOI_NONINTERACTIVE=1 same as --yes
  DOMOVOI_INSTALL_LOG      the log file (default: /var/log/domovoi-install.log when it is
                           writable, else ~/.cache/domovoi-install.log)
  DOMOVOI_MIN_FREE_MB      free disk space needed in the temp directory and the prefix
                           (default: 200)
  DOMOVOI_READY_TIMEOUT    seconds to wait for the hub to answer /readyz after the start
                           (default: 60)
  DOMOVOI_ENROLL_TOKEN     the enrollment token for --role agent (instead of --token-file);
                           only the one `domovoi agent enroll` command sees it
  DOMOVOI_AGENT_TIMEOUT    seconds to wait for an enrolled agent to connect (default: 30)
  NO_COLOR                set it to turn colours off

Exit codes:
  0   done
  2   usage: a bad flag or value (including a bad URL, a literal --token, an incomplete
      --hub, --fingerprint and token set, a password or token file that is not a
      regular file with mode 0600 or 0400 owned by you, --purge without --uninstall, or
      --purge with neither --yes nor a terminal to type the confirmation on)
  10  preflight: this machine cannot take the install, an older version without
      --force-downgrade, or a hub database from before 0.1.0 without --accept-data-reset.
      With --uninstall: nothing is installed, a path it would remove is a symbolic link, or
      it was not run as root
  20  download or checksum
  30  install: the binary or its files
  40  service: users, directories, units or sockets; the hub did not become ready; the
      admin account could not be created; the agent could not be enrolled or did not connect.
      With --uninstall: a service could not be stopped or an account could not be removed
  50  self-test failed
  1   anything else
USAGE
}

# --- Output -------------------------------------------------------------------------------
# say prints a line (not with --quiet); say_always prints it regardless; both append it to
# the log file. With --json stdout is closed to the text and fd 3 carries the JSON lines.
quiet=0
verbose=0
json=0
yes=0
dry_run=0
force_downgrade=0
log_file=""
log_keep=1
tmp=""
step_n=0
step_total=0
step_name=""
step_text=""
last_error=""
fail_code=2
result_pending=0
result_done=0
next_list=""
role=""
version="latest"
start=1
public_url=""
bind_opt=""
agent_url=""
admin_pw_file=""
admin_pw_stdin=0
enroll_hub=""
enroll_fp=""
enroll_name=""
token_file=""
enroll_on=0
hub_part=0
hub_stopped=0
both_wait=0
accept_reset=0
reset_pending=0
hub_data_dir="/var/lib/domovoi"
enrolled_host=""
agent_started=0
agent_timeout=30
hub_env="/etc/domovoi/hub.env"
set_public_url=""
set_bind=""
set_agent_url=""
have_systemd=0
started=0
admin_created=0
hub_fresh_data=1
web_url=""
setup_url=""
bin=""
layout=""
uninstall=0
purge=0
keep_on_failure=0
rollback_armed=0
rolled_back=0
un_errors=0
un_has_marker=0
installer_marker="/etc/domovoi/installer-created"
c_bold=""
c_red=""
c_green=""
c_yellow=""
c_off=""

log_line() { [ -z "${log_file:-}" ] || printf '%s\n' "$*" >>"$log_file" 2>/dev/null || true; }
say() { log_line "$*"; if [ "${quiet:-0}" -eq 0 ]; then printf '%s\n' "$*"; fi; }
say_always() { log_line "$*"; printf '%s\n' "$*"; }
say_c() { log_line "$2"; if [ "${quiet:-0}" -eq 0 ]; then printf '%s%s%s\n' "$1" "$2" "${c_off:-}"; fi; }
vsay() { log_line "$*"; if [ "${verbose:-0}" -eq 1 ] && [ "${quiet:-0}" -eq 0 ]; then printf '%s\n' "$*"; fi; }
err() {
    printf 'install.sh: %s\n' "$*" >&2
    if [ "${json:-0}" -eq 0 ]; then log_line "install.sh: $*"; fi
}
# Shows a captured report: always logged, printed unless --quiet.
show_file() { log_line "$(cat "$1")"; if [ "${quiet:-0}" -eq 0 ]; then cat "$1"; fi; }
# The same for a failure: always printed.
show_file_err() { log_line "$(cat "$1")"; cat "$1"; }

# Escapes a string for a JSON value: control characters other than newline are dropped (a tab
# becomes a space), then backslash and quote are escaped and newlines become \n.
json_escape() {
    printf '%s' "$1" | tr -d '\000-\010\013-\037' | tr '\t' ' ' |
        sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' | awk 'NR > 1 { printf "\\n" } { printf "%s", $0 }'
}
# Turns the lines of stdin into a JSON array of strings.
lines_to_json() {
    lj_out=""
    while IFS= read -r lj_line; do
        [ -n "$lj_line" ] || continue
        lj_out="$lj_out${lj_out:+,}\"$(json_escape "$lj_line")\""
    done
    printf '[%s]' "$lj_out"
}
emit_json() { if [ "${json:-0}" -eq 1 ]; then printf '%s\n' "$1" >&3; fi; }
json_step() { # STATUS MESSAGE
    emit_json "$(printf '{"event":"step","n":%s,"total":%s,"name":"%s","status":"%s","message":"%s"}' \
        "$step_n" "$step_total" "$(json_escape "$step_name")" "$1" "$(json_escape "$2")")"
}
# Starts a numbered step; the previous one, if still open, is closed as ok.
step() { # NAME TEXT
    step_end
    step_n=$((step_n + 1))
    step_name="$1"
    step_text="$2"
    say_c "$c_bold" "[$step_n/$step_total] $2"
    json_step start "$2"
}
step_end() {
    if [ -n "${step_name:-}" ]; then
        json_step ok "$step_text"
        step_name=""
    fi
}
warn() {
    err "$*"
    if [ -n "${step_name:-}" ]; then json_step warn "$*"; fi
}
fail_with() { # CODE MESSAGE
    fw_code="$1"
    shift
    last_error="$*"
    err "$*"
    exit "$fw_code"
}
# Stops with the exit code of the phase that is running ($fail_code).
fail() { fail_with "${fail_code:-1}" "$@"; }

init_colors() {
    if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "$json" -eq 0 ] && [ "${TERM:-dumb}" != "dumb" ]; then
        c_bold="$(printf '\033[1m')"
        c_red="$(printf '\033[31m')"
        c_green="$(printf '\033[32m')"
        c_yellow="$(printf '\033[33m')"
        c_off="$(printf '\033[0m')"
    fi
}

# Chooses the log file: DOMOVOI_INSTALL_LOG, /var/log, ~/.cache, or a temporary file. A dry
# run writes nothing outside the temp directory unless a log file is named.
setup_log() {
    if [ -n "${DOMOVOI_INSTALL_LOG:-}" ]; then
        log_file="$DOMOVOI_INSTALL_LOG"
    elif [ "$dry_run" -eq 1 ]; then
        log_file="$tmp/install.log"
        log_keep=0
    elif ( : >>/var/log/domovoi-install.log ) 2>/dev/null; then
        log_file="/var/log/domovoi-install.log"
    elif [ -n "${HOME:-}" ] && mkdir -p "$HOME/.cache" 2>/dev/null &&
        ( : >>"$HOME/.cache/domovoi-install.log" ) 2>/dev/null; then
        log_file="$HOME/.cache/domovoi-install.log"
    else
        log_file="$(mktemp "${TMPDIR:-/tmp}/domovoi-install.XXXXXX")"
    fi
    if ! ( : >>"$log_file" ) 2>/dev/null; then
        log_file="$tmp/install.log"
        log_keep=0
    fi
    log_line "== install.sh $(date -u '+%Y-%m-%dT%H:%M:%SZ') =="
}

# Builds and prints the one JSON result object, once.
emit_result() { # OK CODE
    if [ "$result_done" -eq 1 ]; then return 0; fi
    result_done=1
    er_log=""
    if [ "$log_keep" -eq 1 ]; then er_log="$log_file"; fi
    er_plan="[]"
    if [ -n "$tmp" ] && [ -f "$tmp/plan" ]; then er_plan="$(lines_to_json <"$tmp/plan")"; fi
    er_urls="{"
    if [ -n "$web_url" ]; then er_urls="$er_urls\"web\":\"$(json_escape "$web_url")\""; fi
    if [ -n "$setup_url" ]; then er_urls="$er_urls${web_url:+,}\"setup\":\"$(json_escape "$setup_url")\""; fi
    er_urls="$er_urls}"
    er_admin=false
    er_started=false
    er_agent=false
    if [ "$admin_created" -eq 1 ]; then er_admin=true; fi
    if [ "$started" -eq 1 ]; then er_started=true; fi
    if [ "$agent_started" -eq 1 ]; then er_agent=true; fi
    er_mode=install
    er_purge=false
    er_rolled=false
    er_removed="[]"
    er_kept="[]"
    if [ "$uninstall" -eq 1 ]; then er_mode=uninstall; fi
    if [ "$purge" -eq 1 ]; then er_purge=true; fi
    if [ "$rolled_back" -eq 1 ]; then er_rolled=true; fi
    if [ -n "$tmp" ] && [ -f "$tmp/removed" ]; then er_removed="$(lines_to_json <"$tmp/removed")"; fi
    if [ -n "$tmp" ] && [ -f "$tmp/kept" ]; then er_kept="$(lines_to_json <"$tmp/kept")"; fi
    emit_json "$(printf '{"event":"result","ok":%s,"exit_code":%s,"role":"%s","version":"%s","layout":"%s","paths":{"binary":"%s","log":"%s"},"urls":%s,"admin_created":%s,"started":%s,"enrolled_host":"%s","agent_started":%s,"next_steps":%s,"plan":%s,"error":"%s","mode":"%s","purge":%s,"rolled_back":%s,"removed":%s,"kept":%s}' \
        "$1" "$2" "$(json_escape "$role")" "$(json_escape "$version")" \
        "$(json_escape "${layout:-$role}")" "$(json_escape "$bin")" "$(json_escape "$er_log")" \
        "$er_urls" "$er_admin" "$er_started" "$(json_escape "$enrolled_host")" "$er_agent" \
        "$(printf '%s\n' "$next_list" | lines_to_json)" "$er_plan" "$(json_escape "$last_error")" \
        "$er_mode" "$er_purge" "$er_rolled" "$er_removed" "$er_kept")"
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

# --- What this run created: the rollback manifest and the installer record ------------------
# A fresh install that fails before the first service is started is undone from the manifest,
# $tmp/created, one KIND|VALUE line per item this run created (file, dir, tree, user or group),
# appended as each is made and removed again newest first. Nothing that was already there is
# ever written to it, so the undo cannot touch it. A tree is removed whole; a dir only when it
# is empty. The record /etc/domovoi/installer-created lives on: it lists the accounts and the
# unusual directories the installer created, for --uninstall --purge.
manifest_add() { # KIND VALUE
    if [ "$rollback_armed" -eq 1 ]; then printf '%s|%s\n' "$1" "$2" >>"$tmp/created"; fi
}

# Starts recording, only for a run that can be undone as a whole: not a migration, not an
# upgrade (a binary is there already), and not where a hub or an agent unit is installed. For
# an agent that also needs a fresh layout, so one that shares the machine with a hub is not.
arm_rollback() {
    [ "$migrate" -eq 0 ] || return 0
    [ "$state" = "fresh" ] || return 0
    [ ! -e /etc/systemd/system/domovoi-hub.service ] && [ ! -e "$agent_unit_path" ] || return 0
    if [ "$role" = "agent" ] && [ "$service" -eq 1 ]; then
        classify_host
        case "$layout" in
        fresh | nosystemd) ;;
        *) return 0 ;;
        esac
    fi
    rollback_armed=1
    : >"$tmp/created"
}

# Stops recording and forgets the manifest: a service is about to be started, so a failure
# from here on is handled by hand, as it always was.
rollback_disarm() {
    rollback_armed=0
    if [ -n "$tmp" ]; then : >"$tmp/created"; fi
}

# Creates DIR and the parents that are missing, recording each one that was not there.
mkdir_rec() {
    mr_missing=""
    mr_dir="$1"
    while [ ! -e "$mr_dir" ] && [ ! -L "$mr_dir" ] && [ "$mr_dir" != "/" ] && [ "$mr_dir" != "." ]; do
        mr_missing="$mr_dir
$mr_missing"
        mr_dir="$(dirname "$mr_dir")"
    done
    mkdir -p "$1"
    printf '%s\n' "$mr_missing" | while IFS= read -r mr_path; do
        if [ -n "$mr_path" ]; then manifest_add dir "$mr_path"; fi
    done
}

group_exists() { getent group "$1" >/dev/null 2>&1 || grep -q "^$1:" /etc/group 2>/dev/null; }

# Notes an account this run created (GROUP_EXISTED is 1 when the group was there before).
record_account() { # NAME GROUP_EXISTED
    manifest_add user "$1"
    printf 'user %s\n' "$1" >>"$tmp/marker-new"
    if [ "$2" -eq 0 ]; then
        manifest_add group "$1"
        printf 'group %s\n' "$1" >>"$tmp/marker-new"
    fi
}

# Appends what is waiting in $tmp/marker-new to the installer record, once /etc/domovoi exists.
flush_marker() {
    [ -s "$tmp/marker-new" ] && [ -d /etc/domovoi ] && [ ! -L "$installer_marker" ] || return 0
    if [ ! -e "$installer_marker" ]; then
        : >"$installer_marker"
        chmod 0600 "$installer_marker"
        chown root:root "$installer_marker"
        manifest_add file "$installer_marker"
    fi
    while IFS= read -r fm_line; do
        grep -qxF "$fm_line" "$installer_marker" || printf '%s\n' "$fm_line" >>"$installer_marker"
    done <"$tmp/marker-new"
    : >"$tmp/marker-new"
}

# Creates DIR with MODE and OWNER only when it does not exist, so a second run keeps
# whatever was changed since.
make_dir() {
    if [ ! -d "$1" ]; then
        mkdir_rec "$1"
        manifest_add tree "$1"
        chmod "$2" "$1"
        chown "$3" "$1"
        say "Created $1"
        case "$1" in
        /etc/domovoi | /etc/domovoi/skills | /var/lib/domovoi | /var/lib/domovoi-agent | /var/lib/domovoi-packages | /var/lib/domovoi-helper) ;;
        *) printf 'dir %s\n' "$1" >>"$tmp/marker-new" ;;
        esac
    fi
    flush_marker
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
        manifest_add tree "$dest"
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
        chl_group=0
        if group_exists domovoi; then chl_group=1; fi
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
        record_account domovoi "$chl_group"
        say "Created the domovoi system user."
    fi
    make_dir /etc/domovoi 0750 root:domovoi
    make_dir /var/lib/domovoi 0750 domovoi:domovoi
    flush_marker
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
        printf '%s\n' "its uid is 0"
    fi
    if [ "$ap_primary" != "$ap_name" ]; then
        printf '%s\n' "its primary group is $ap_primary, not $ap_name"
    fi
    case "$ap_shell" in
    */nologin | */false) ;;
    *) printf '%s\n' "its login shell is ${ap_shell:-empty}; it must be nologin or false" ;;
    esac
    id -Gn "$ap_name" | tr ' ' '\n' | while read -r ap_group; do
        case " $ap_name $ap_extra " in
        *" $ap_group "*) ;;
        *) printf '%s\n' "it is in the group $ap_group, but may only be in its own${ap_extra:+ and $ap_extra}" ;;
        esac
    done
    if getent group docker | cut -d: -f4 | tr ',' '\n' | grep -qx "$ap_name"; then
        printf '%s\n' "it is a member of the docker group, which is root on a Docker host"
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
        record_account domovoi-agent 0
        say "Created the domovoi-agent system user."
    fi
    if id domovoi-skill >/dev/null 2>&1; then
        say "Verified the existing domovoi-skill account; it was not changed."
    else
        useradd --system --user-group --no-create-home --home-dir /nonexistent \
            --shell "$ea_nologin" domovoi-skill
        record_account domovoi-skill 0
        say "Created the domovoi-skill system user."
    fi
    flush_marker
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
        manifest_add file /etc/domovoi/helper.toml
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
        mkdir_rec /etc/systemd/system
        install -m 0644 "$tmp/$pu_name" "$pu_path"
        manifest_add file "$pu_path"
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
    rollback_disarm
    systemctl enable domovoi-helper-selftest.service || true
    systemctl start --no-block domovoi-helper-selftest.service || say "The firewall self-test did not start; run: sudo $bin helper nft-selftest"
    systemctl enable --now domovoi-helper.socket
    say "Started domovoi-helper.socket"
    if "$bin" agent selftest --installer --record >"$tmp/selftest.out" 2>&1; then
        show_file "$tmp/selftest.out"
    else
        show_file_err "$tmp/selftest.out"
        systemctl disable --now domovoi-helper.socket || true
        ss_failed="$(failed_checks "$tmp/selftest.out")"
        fail_with 50 "the self-test failed (${ss_failed:-see the report above}); the helper socket is disabled, the skill runner socket was not started and nothing else was started"
    fi
    systemctl enable --now domovoi-skill-runner.socket
    say "Started domovoi-skill-runner.socket"
    if "$bin" agent selftest --installer >"$tmp/runner-selftest.out" 2>&1; then
        show_file "$tmp/runner-selftest.out"
    else
        show_file_err "$tmp/runner-selftest.out"
        systemctl disable --now domovoi-skill-runner.socket || true
        ss_failed="$(failed_checks "$tmp/runner-selftest.out")"
        fail_with 50 "the self-test failed with the skill runner socket up (${ss_failed:-see the report above}); the skill runner socket is disabled, so skills stay unavailable, and the helper socket keeps running"
    fi
}

# Undoes what a failed fresh install created, newest first, from $tmp/created. It removes only
# what the manifest names, never a path that is a symbolic link's target, and it carries on
# when one removal fails. INT and TERM are ignored while it runs, so an interrupted rollback
# still finishes; it never changes the exit code of the script.
rollback_run() {
    set +e
    trap '' INT TERM
    : >"$tmp/removed"
    : >"$tmp/rb-failed"
    sed '1!G;h;$!d' "$tmp/created" >"$tmp/created.rev"
    while IFS='|' read -r rb_kind rb_val; do
        case "$rb_val" in
        /?*) ;;
        *) continue ;;
        esac
        case "$rb_kind" in
        file)
            if [ -f "$rb_val" ] || [ -L "$rb_val" ]; then
                if rm -f -- "$rb_val"; then printf '%s\n' "$rb_val" >>"$tmp/removed"; else printf '%s\n' "$rb_val" >>"$tmp/rb-failed"; fi
            fi
            ;;
        dir)
            if [ -d "$rb_val" ] && [ ! -L "$rb_val" ] && rmdir -- "$rb_val" 2>/dev/null; then
                printf '%s\n' "$rb_val" >>"$tmp/removed"
            fi
            ;;
        tree)
            if [ -L "$rb_val" ]; then
                if rm -f -- "$rb_val"; then printf '%s\n' "$rb_val" >>"$tmp/removed"; else printf '%s\n' "$rb_val" >>"$tmp/rb-failed"; fi
            elif [ -e "$rb_val" ]; then
                if rm -rf -- "$rb_val"; then printf '%s\n' "$rb_val" >>"$tmp/removed"; else printf '%s\n' "$rb_val" >>"$tmp/rb-failed"; fi
            fi
            ;;
        user)
            if id "$rb_val" >/dev/null 2>&1; then
                if userdel "$rb_val" >/dev/null 2>&1 || deluser "$rb_val" >/dev/null 2>&1; then
                    printf 'user %s\n' "$rb_val" >>"$tmp/removed"
                else
                    printf 'user %s\n' "$rb_val" >>"$tmp/rb-failed"
                fi
            fi
            ;;
        group)
            if group_exists "$rb_val"; then
                if groupdel "$rb_val" >/dev/null 2>&1 || delgroup "$rb_val" >/dev/null 2>&1; then
                    printf 'group %s\n' "$rb_val" >>"$tmp/removed"
                else
                    printf 'group %s\n' "$rb_val" >>"$tmp/rb-failed"
                fi
            fi
            ;;
        esac
    done <"$tmp/created.rev"
    if [ "$have_systemd" -eq 1 ] && grep -q '^/etc/systemd/system/' "$tmp/removed"; then
        systemctl daemon-reload >/dev/null 2>&1
    fi
    trap - INT TERM
}

# The failure handling of a run that recorded what it created: undoes it, or with
# --keep-on-failure lists it and leaves it. Prints what was done and how to try again.
rollback_finish() {
    [ -s "$tmp/created" ] || return 0
    if [ "$keep_on_failure" -eq 1 ]; then
        say_always ""
        say_always "Left in place (--keep-on-failure); nothing was undone. This run had created:"
        cut -d'|' -f2 "$tmp/created" | while IFS= read -r rf_item; do say_always "  $rf_item"; done
        return 0
    fi
    rollback_run
    if [ -s "$tmp/removed" ]; then
        rolled_back=1
        say_always ""
        say_always "Rolled back: this run had created the following, and it is removed again:"
        while IFS= read -r rf_item; do say_always "  $rf_item"; done <"$tmp/removed"
        say_always "Nothing that was on this machine before the run was touched."
        record_next "Fix the cause above, then run install.sh again with the same options (--keep-on-failure leaves a failed install in place for debugging)"
        say_always "Fix the cause above, then run install.sh again with the same options."
        say_always "Add --keep-on-failure to leave a failed install in place for debugging."
    fi
    if [ -s "$tmp/rb-failed" ]; then
        say_always ""
        say_always "These could not be removed; remove them by hand:"
        while IFS= read -r rf_item; do say_always "  $rf_item"; done <"$tmp/rb-failed"
    fi
}

# The EXIT trap: undoes a failed fresh install, closes the open step, prints the JSON result,
# points at the log, and removes the temp directory. A script that dies without calling fail
# still ends with a documented code. It stays textually after the code that creates the paths
# of the created-set, because it holds the one `systemctl start` that is not a service start.
on_exit() {
    oe_rc=$?
    trap - EXIT INT TERM
    if [ "$oe_rc" -ne 0 ] && [ -z "$last_error" ]; then
        last_error="stopped unexpectedly; see the log"
        oe_rc="${fail_code:-1}"
    fi
    if [ "$oe_rc" -ne 0 ] && [ "$rollback_armed" -eq 1 ]; then
        rollback_finish
    fi
    if [ "$result_pending" -eq 1 ]; then
        if [ "$oe_rc" -eq 0 ]; then
            step_end
            emit_result true 0
        else
            if [ -n "$step_name" ]; then json_step fail "$last_error"; fi
            emit_result false "$oe_rc"
        fi
    fi
    if [ "$oe_rc" -ne 0 ] && [ "$log_keep" -eq 1 ] && [ -n "$log_file" ]; then
        printf 'install.sh: the log is %s\n' "$log_file" >&2
    fi
    if [ "$hub_stopped" -eq 1 ]; then
        systemctl start domovoi-hub >/dev/null 2>&1 || true
    fi
    if [ -n "$tmp" ]; then rm -rf "$tmp"; fi
    exit "$oe_rc"
}

# --- Uninstall ----------------------------------------------------------------------------
# --uninstall removes what install.sh puts in place and keeps what the owner made or the
# running system filled: configuration, data, skills, accounts and the identity-migration
# record. --purge removes those too. Only the fixed set below is ever touched, no path is
# followed through a symbolic link, and every path is checked before the first change.
un_units="domovoi-agent.service domovoi-hub.service domovoi-skill-runner.socket domovoi-helper.socket domovoi-helper-selftest.service domovoi-skill-runner@.service domovoi-helper@.service"
un_data_dirs="/var/lib/domovoi /var/lib/domovoi-agent /var/lib/domovoi-packages /var/lib/domovoi-helper"
un_accounts="domovoi domovoi-agent domovoi-skill"
un_log="/var/log/domovoi-install.log"

un_exists() { [ -e "$1" ] || [ -L "$1" ]; }
un_keep() { printf '%s\n' "$1" >>"$tmp/kept"; plan_add keep "$1"; }
un_removed() { printf '%s\n' "$1" >>"$tmp/removed"; say "Removed $1"; }
un_marker_has() { [ "$un_has_marker" -eq 1 ] && grep -qxF "$1 $2" "$tmp/un-marker"; }

# The disk space under PATH, for the list of what a purge deletes.
un_size() {
    us_kb="$(du -sk "$1" 2>/dev/null | awk 'NR == 1 { print $1 }')" || us_kb=""
    case "$us_kb" in
    "" | *[!0-9]*) printf 'size unknown\n' ;;
    *)
        if [ "$us_kb" -ge 1024 ]; then printf '%s MB\n' $((us_kb / 1024)); else printf '%s KB\n' "$us_kb"; fi
        ;;
    esac
}

# Whether PATH, a directory the installer recorded outside its own set, may be deleted: an
# absolute path at least two levels deep that is not a system directory.
un_safe_dir() {
    case "$1" in
    /?*) ;;
    *) return 1 ;;
    esac
    case "$1" in
    *..* | *[[:space:]]* | *"'"* | *'"'*) return 1 ;;
    /bin | /boot | /dev | /etc | /home | /lib | /lib64 | /mnt | /media | /opt | /proc | /root | /run | /sbin | /srv | /sys | /tmp | /usr | /var) return 1 ;;
    /usr/bin | /usr/lib | /usr/local | /usr/local/bin | /usr/sbin | /usr/share | /var/lib | /var/log | /var/tmp | /var/cache | /etc/systemd | /etc/systemd/system) return 1 ;;
    esac
    [ "$(printf '%s' "$1" | tr -cd '/' | wc -c)" -ge 2 ]
}

# Decides whether the account $1 may be removed by --purge. Sets $uac_ok to 1 or 0 and the
# reason to $uac_why. With the installer record, only an account it lists; without one, only
# an account that looks like a system account made for this program.
un_account_check() {
    uac_ok=0
    uac_why=""
    uac_uid="$(id -u "$1")"
    if [ "$uac_uid" -eq 0 ]; then
        uac_why="its uid is 0"
        return 0
    fi
    if [ "$un_has_marker" -eq 1 ]; then
        if un_marker_has user "$1"; then
            uac_ok=1
        else
            uac_why="not created by the installer (it is not in $installer_marker)"
        fi
        return 0
    fi
    uac_line="$(getent passwd "$1")"
    uac_shell="$(printf '%s\n' "$uac_line" | cut -d: -f7)"
    uac_home="$(printf '%s\n' "$uac_line" | cut -d: -f6)"
    uac_style=1
    if [ "$uac_uid" -ge 1000 ]; then uac_style=0; fi
    case "$uac_shell" in
    */nologin | */false) ;;
    *) uac_style=0 ;;
    esac
    case "$uac_home" in
    /var/lib/domovoi | /var/lib/domovoi-agent | /nonexistent) ;;
    *) uac_style=0 ;;
    esac
    if [ "$uac_style" -eq 1 ]; then
        uac_ok=1
    else
        uac_why="no installer record, and it does not look like a system account (uid $uac_uid, shell ${uac_shell:-none}, home ${uac_home:-none})"
    fi
}

# Looks at the machine and writes the plan: what is stopped and removed, what is kept (and
# where), and, with --purge, what is deleted with its size. Reads only. Stops with exit 10
# when a path it would touch is a symbolic link, or when nothing is installed.
un_detect() {
    : >"$tmp/plan"
    : >"$tmp/removed"
    : >"$tmp/kept"
    : >"$tmp/un-units"
    : >"$tmp/un-dropins"
    : >"$tmp/un-purge-dirs"
    un_refuse=""
    un_found=0
    un_kept_any=0
    un_helper=0
    un_has_marker=0
    if [ -f "$installer_marker" ] && [ ! -L "$installer_marker" ] && cp "$installer_marker" "$tmp/un-marker" 2>/dev/null; then
        un_has_marker=1
    fi
    for ud_unit in $un_units; do
        ud_path="/etc/systemd/system/$ud_unit"
        un_exists "$ud_path" || continue
        printf '%s\n' "$ud_unit" >>"$tmp/un-units"
        un_found=1
        if [ -L "$ud_path" ]; then un_refuse="$un_refuse $ud_path"; fi
        case "$ud_unit" in
        domovoi-helper*) un_helper=1 ;;
        esac
    done
    if [ "$have_systemd" -eq 1 ]; then
        while IFS= read -r ud_unit; do
            case "$ud_unit" in
            *@.service) plan_add stop "every running instance of $ud_unit" ;;
            *) plan_add stop "$ud_unit (systemctl disable --now)" ;;
            esac
        done <"$tmp/un-units"
    elif [ -s "$tmp/un-units" ]; then
        plan_add note "no systemd on this host: no service is stopped; stop a hub or agent you started by hand yourself"
    fi
    while IFS= read -r ud_unit; do
        plan_add remove "/etc/systemd/system/$ud_unit"
    done <"$tmp/un-units"
    if [ -s "$tmp/un-units" ] && [ "$have_systemd" -eq 1 ]; then plan_add reload "systemctl daemon-reload"; fi
    : >"$tmp/un-custom"
    if un_exists "$bin"; then
        un_found=1
        if [ -L "$bin" ]; then un_refuse="$un_refuse $bin"; fi
        plan_add remove "$bin"
    fi
    if un_exists "$prefix/share/domovoi"; then
        un_found=1
        if [ -L "$prefix/share/domovoi" ]; then un_refuse="$un_refuse $prefix/share/domovoi"; fi
        plan_add remove "$prefix/share/domovoi (the release examples and any saved unit text)"
    fi
    if [ -d /var/lib/domovoi-helper ] || [ "$un_helper" -eq 1 ]; then
        un_helper=1
        if [ -x "$bin" ]; then
            plan_add cleanup "$bin helper nft-recreate (deletes the helper's own firewall table if it holds no block)"
        fi
    fi
    ud_dropins=""
    for ud_path in /etc/systemd/system/domovoi-*.d; do
        if un_exists "$ud_path"; then
            ud_dropins="$ud_dropins $ud_path"
            if [ -L "$ud_path" ]; then un_refuse="$un_refuse $ud_path"; fi
        fi
    done
    # What stays, or goes with --purge.
    ud_keepers="/etc/domovoi $un_data_dirs"
    ud_custom=""
    if [ "$purge" -eq 1 ] && [ -x "$bin" ] && [ -d /etc/domovoi ]; then
        ud_dirs_out="$("$bin" agent dirs 2>/dev/null </dev/null)" || ud_dirs_out=""
        ud_custom="$(printf '%s\n' "$ud_dirs_out" | sed -n -e 's/^skills_dir=//p' -e 's/^packages_dir=//p')"
    fi
    if [ "$un_has_marker" -eq 1 ]; then
        ud_custom="$ud_custom
$(sed -n 's/^dir //p' "$tmp/un-marker")"
    fi
    for ud_path in $ud_keepers; do
        un_exists "$ud_path" || continue
        un_kept_any=1
        if [ "$purge" -eq 1 ]; then
            if [ -L "$ud_path" ]; then un_refuse="$un_refuse $ud_path"; fi
            un_found=1
            printf '%s\n' "$ud_path" >>"$tmp/un-purge-dirs"
            plan_add delete "$ud_path ($(un_size "$ud_path"))"
        else
            case "$ud_path" in
            /etc/domovoi) un_keep "$ud_path (configuration, skills, the installer record and the identity-migration record; --purge deletes it)" ;;
            *) un_keep "$ud_path (data, $(un_size "$ud_path"); --purge deletes it)" ;;
            esac
        fi
    done
    printf '%s\n' "$ud_custom" | sort -u | while IFS= read -r ud_path; do
        [ -n "$ud_path" ] || continue
        case "$ud_path" in
        /etc/domovoi/skills | /var/lib/domovoi-packages) continue ;;
        esac
        printf '%s\n' "$ud_path"
    done >"$tmp/un-custom"
    while IFS= read -r ud_path; do
        un_exists "$ud_path" || continue
        if [ "$purge" -eq 1 ] && un_marker_has dir "$ud_path" && un_safe_dir "$ud_path" && [ ! -L "$ud_path" ]; then
            un_found=1
            printf '%s\n' "$ud_path" >>"$tmp/un-purge-dirs"
            plan_add delete "$ud_path ($(un_size "$ud_path")), made by the installer for the skills or packages"
        elif [ "$purge" -eq 1 ]; then
            un_keep "$ud_path (a skills or packages directory from the agent configuration; the installer did not make it, so it is not deleted)"
        else
            un_kept_any=1
            un_keep "$ud_path (a skills or packages directory from the installer record; --purge deletes it)"
        fi
    done <"$tmp/un-custom"
    if [ "$purge" -eq 1 ]; then
        for ud_path in $ud_dropins; do
            printf '%s\n' "$ud_path" >>"$tmp/un-dropins"
            un_found=1
            plan_add delete "$ud_path (systemd drop-in)"
        done
        if un_exists "$un_log"; then plan_add delete "$un_log"; fi
    else
        for ud_path in $ud_dropins; do
            un_keep "$ud_path (a systemd drop-in; --purge deletes it)"
        done
    fi
    # Accounts.
    : >"$tmp/un-accounts"
    for ud_name in $un_accounts; do
        id "$ud_name" >/dev/null 2>&1 || continue
        un_account_check "$ud_name"
        if [ "$purge" -eq 1 ]; then
            if [ "$uac_ok" -eq 1 ]; then
                un_found=1
                printf '%s\n' "$ud_name" >>"$tmp/un-accounts"
                plan_add delete "account $ud_name and its group (created by the installer)"
            else
                un_keep "account $ud_name ($uac_why); it is left as it is"
            fi
        else
            un_kept_any=1
            un_keep "account $ud_name (--purge deletes the ones the installer created)"
        fi
    done
    if [ -n "$un_refuse" ]; then
        fail_with 10 "refusing to go on: these are symbolic links, and --uninstall never follows a link:$un_refuse. Nothing was changed. Remove or replace the link by hand and run again"
    fi
    if [ "$un_found" -eq 0 ]; then
        if [ "$un_kept_any" -eq 1 ]; then
            fail_with 10 "nothing to uninstall: no unit, binary or examples of Domovoi are left under $prefix or /etc/systemd/system. The configuration, data and accounts are still there; run with --purge to delete them"
        fi
        fail_with 10 "nothing to uninstall: no Domovoi installation was found (looked for the units in /etc/systemd/system, $bin, $prefix/share/domovoi, /etc/domovoi and /var/lib/domovoi*)"
    fi
}

# Prints the plan of the uninstall (dry run, or the list before a purge asks). With a second
# argument of "always" it is shown even with --quiet.
print_un_plan() { # HEADING [always]
    pu_say=say
    if [ "${2:-}" = "always" ]; then pu_say=say_always; fi
    "$pu_say" "$1"
    while IFS= read -r pl_line; do
        "$pu_say" "  $pl_line"
    done <"$tmp/plan"
}

# Asks for the typed word on the terminal. --yes skips it: --purge was passed on purpose.
purge_confirm() {
    [ "$yes" -eq 0 ] || return 0
    ( : </dev/tty ) 2>/dev/null ||
        fail_with 2 "--purge deletes data and asks for confirmation on the terminal; pass --yes to confirm without one"
    printf 'Type purge to delete all of the above, anything else to stop: ' >/dev/tty
    IFS= read -r pc_answer </dev/tty || pc_answer=""
    [ "$pc_answer" = "purge" ] || fail_with 2 "not confirmed; nothing was changed"
}

# Stops and disables every unit that exists, sockets before the instances they spawned.
un_stop_services() {
    if [ "$have_systemd" -eq 0 ]; then
        say "No systemd on this host, so no service was stopped."
        return 0
    fi
    while IFS= read -r us_unit; do
        case "$us_unit" in
        *@.service) continue ;;
        esac
        if systemctl disable --now "$us_unit" >"$tmp/un.out" 2>&1; then
            say "Stopped and disabled $us_unit"
        else
            show_file_err "$tmp/un.out"
            warn "could not stop and disable $us_unit"
            un_errors=$((un_errors + 1))
        fi
    done <"$tmp/un-units"
    while IFS= read -r us_unit; do
        case "$us_unit" in
        *@.service)
            us_glob="${us_unit%.service}*.service"
            systemctl stop "$us_glob" >/dev/null 2>&1 || true
            systemctl reset-failed "$us_glob" >/dev/null 2>&1 || true
            say "Stopped every running instance of $us_unit"
            ;;
        esac
    done <"$tmp/un-units"
}

# Deletes the helper's own firewall table when it holds no block. The command is the helper's
# `nft-recreate`; it stops with a message when a block is live, which is only reported.
un_nft_cleanup() {
    [ "$un_helper" -eq 1 ] && [ -x "$bin" ] || return 0
    if "$bin" helper nft-recreate >"$tmp/nft.out" 2>&1 </dev/null; then
        say "The helper's firewall table was cleaned up (a table that held no block is deleted)."
    else
        show_file_err "$tmp/nft.out"
        warn "the helper's firewall table was not cleaned up (the message is above)"
    fi
}

# Removes the unit files, the wants links enabling them made, and reloads systemd.
un_remove_units() {
    while IFS= read -r ur_unit; do
        rm -f -- "/etc/systemd/system/$ur_unit"
        for ur_wants in multi-user.target.wants sockets.target.wants; do
            if [ -L "/etc/systemd/system/$ur_wants/$ur_unit" ]; then rm -f -- "/etc/systemd/system/$ur_wants/$ur_unit"; fi
        done
        un_removed "/etc/systemd/system/$ur_unit"
    done <"$tmp/un-units"
    while IFS= read -r ur_dropin; do
        if [ -d "$ur_dropin" ] && [ ! -L "$ur_dropin" ]; then
            rm -rf -- "$ur_dropin"
            un_removed "$ur_dropin"
        fi
    done <"$tmp/un-dropins"
    if [ "$have_systemd" -eq 1 ] && [ -s "$tmp/un-units" ]; then
        if ! systemctl daemon-reload; then
            warn "systemctl daemon-reload failed"
            un_errors=$((un_errors + 1))
        fi
    fi
}

# Removes the binary and the examples directory.
un_remove_files() {
    if un_exists "$bin"; then
        rm -f -- "$bin"
        un_removed "$bin"
    fi
    if [ -d "$prefix/share/domovoi" ] && [ ! -L "$prefix/share/domovoi" ]; then
        rm -rf -- "$prefix/share/domovoi"
        un_removed "$prefix/share/domovoi"
    fi
}

# Removes one account and its group: userdel, then groupdel for a group that is left empty and
# was made for the account.
un_remove_account() {
    ura_name="$1"
    if userdel "$ura_name" >"$tmp/un.out" 2>&1 || deluser "$ura_name" >>"$tmp/un.out" 2>&1; then
        un_removed "account $ura_name"
    else
        show_file_err "$tmp/un.out"
        warn "could not remove the account $ura_name"
        un_errors=$((un_errors + 1))
        return 0
    fi
    if [ "$un_has_marker" -eq 1 ] && ! un_marker_has group "$ura_name"; then return 0; fi
    ura_gid="$(getent group "$ura_name" | cut -d: -f3)"
    if [ "$un_has_marker" -eq 0 ] && { [ -z "$ura_gid" ] || [ "$ura_gid" -ge 1000 ]; }; then return 0; fi
    if group_exists "$ura_name" && [ -z "$(getent group "$ura_name" | cut -d: -f4)" ]; then
        if groupdel "$ura_name" >/dev/null 2>&1 || delgroup "$ura_name" >/dev/null 2>&1; then
            un_removed "group $ura_name"
        else
            warn "could not remove the group $ura_name"
            un_errors=$((un_errors + 1))
        fi
    fi
}

# The destructive part of --purge: configuration, data, drop-ins, the install log, accounts.
un_purge() {
    if [ -s "$tmp/un-purge-dirs" ]; then
        while IFS= read -r up_path; do
            if [ -d "$up_path" ] && [ ! -L "$up_path" ]; then
                rm -rf -- "$up_path"
                un_removed "$up_path"
            fi
        done <"$tmp/un-purge-dirs"
    fi
    while IFS= read -r up_name; do
        un_remove_account "$up_name"
    done <"$tmp/un-accounts"
    if un_exists "$un_log"; then
        # Nothing is written to the log once it is gone, or this run would make it again.
        if [ "$log_file" = "$un_log" ]; then
            log_file=""
            log_keep=0
        fi
        rm -f -- "$un_log"
        printf '%s\n' "$un_log" >>"$tmp/removed"
        say "Removed $un_log"
    fi
}

# The whole of --uninstall.
run_uninstall() {
    fail_code=10
    version=""
    bin_dir="$prefix/bin"
    bin="$bin_dir/domovoi"
    case "$prefix" in
    /?*) ;;
    *) fail_with 2 "--prefix must be an absolute path, not $prefix" ;;
    esac
    case "$prefix" in
    *..* | *[[:space:]]*) fail_with 2 "--prefix must not contain .. or spaces" ;;
    esac
    if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then have_systemd=1; fi
    if [ "$dry_run" -eq 1 ]; then
        step_total=2
    else
        step_total=$((4 + purge))
    fi
    if [ "$dry_run" -eq 0 ] && [ "$(id -u)" -ne 0 ]; then
        fail_with 10 "--uninstall must run as root (sudo); --dry-run shows the plan without it"
    fi
    step detect "Looking for a Domovoi installation"
    un_detect
    if [ "$dry_run" -eq 1 ]; then
        step plan "Planning the uninstall"
        print_un_plan "Plan for the uninstall$(if [ "$purge" -eq 1 ]; then printf ' with --purge'; fi) (dry run; nothing is changed):"
        say ""
        say "Dry run: nothing was changed."
        if [ "$log_keep" -eq 1 ]; then say_always "Log file: $log_file"; fi
        return 0
    fi
    if [ "$purge" -eq 1 ]; then
        print_un_plan "This will delete everything below, and the data cannot be brought back:" always
        purge_confirm
    else
        print_un_plan "Uninstalling:"
    fi
    step stop "Stopping the services"
    fail_code=40
    un_stop_services
    un_nft_cleanup
    step units "Removing the unit files"
    un_remove_units
    step files "Removing the binary and the examples"
    un_remove_files
    if [ "$purge" -eq 1 ]; then
        step purge "Deleting the configuration, the data and the accounts"
        un_purge
    fi
    say_always ""
    if [ "$purge" -eq 1 ]; then
        say_always "Purged: Domovoi is removed from this machine."
    else
        say_always "Uninstalled the program. These were kept on purpose:"
        while IFS= read -r uk_line; do
            say_always "  $uk_line"
        done <"$tmp/kept"
        record_next "sudo sh install.sh --uninstall --purge    # also deletes the configuration, the data and the accounts the installer created"
    fi
    if [ "$un_helper" -eq 1 ]; then
        say_always ""
        say_always "The helper's firewall table is deleted only when it holds no block. Check what is left with:"
        ns "nft list tables    # a table named domovoi that remains holds an active block: nft delete table inet domovoi"
    fi
    if [ "$un_errors" -gt 0 ]; then
        fail_with 40 "the uninstall finished with $un_errors problem(s); see the messages above"
    fi
    if [ "$log_keep" -eq 1 ]; then say_always "Log file: $log_file"; fi
    say_always "Done."
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

# Asks for the typed word on the terminal. --yes does not skip it, and nothing else does.
migrate_confirm() {
    ( : </dev/tty ) 2>/dev/null ||
        fail_with 2 "--migrate-identity asks for confirmation on the terminal and --yes does not skip it; run it from an interactive shell"
    printf 'Type migrate to continue, anything else to stop: ' >/dev/tty
    IFS= read -r mc_answer </dev/tty || mc_answer=""
    [ "$mc_answer" = "migrate" ] || fail_with 2 "not confirmed; nothing was changed"
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
    if [ -f "$tmp/wait.out" ]; then show_file_err "$tmp/wait.out"; fi
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

# --- Preflight ----------------------------------------------------------------------------
# One row per check, in $tmp/preflight as STATUS|NAME|DETAIL where STATUS is ok, warn or fail.
pf_failed=0
pf_add() { # STATUS NAME DETAIL
    printf '%s|%s|%s\n' "$1" "$2" "$3" >>"$tmp/preflight"
    if [ "$1" = "fail" ]; then pf_failed=$((pf_failed + 1)); fi
}

# The closest directory at or above $1 that exists.
nearest_dir() {
    nd_dir="$1"
    while [ ! -d "$nd_dir" ]; do
        nd_dir="$(dirname "$nd_dir")"
    done
    printf '%s\n' "$nd_dir"
}

# Adds a row for the free space on the file system that holds $2.
check_free() { # LABEL DIR
    cf_dir="$(nearest_dir "$2")"
    cf_kb="$(df -Pk "$cf_dir" 2>/dev/null | awk 'NR == 2 { print $4 }')" || cf_kb=""
    case "$cf_kb" in
    "" | *[!0-9]*) pf_add warn "$1" "could not read the free space of $cf_dir" ;;
    *)
        cf_mb=$((cf_kb / 1024))
        if [ "$cf_mb" -lt "$min_free_mb" ]; then
            pf_add fail "$1" "$cf_mb MB free in $cf_dir; at least $min_free_mb MB is needed"
        else
            pf_add ok "$1" "$cf_mb MB free in $cf_dir"
        fi
        ;;
    esac
}

# Prints 0 when something listens on TCP port $1, 1 when nothing does, 2 when this host has
# neither ss nor /proc/net/tcp to tell.
port_state() {
    ps_port="$1"
    if command -v ss >/dev/null 2>&1 && ps_out="$(ss -ltn 2>/dev/null)"; then
        if printf '%s\n' "$ps_out" | awk -v p="$ps_port" 'NR > 1 { n = split($4, a, ":"); if (a[n] == p) f = 1 } END { exit !f }'; then
            echo 0
        else
            echo 1
        fi
        return 0
    fi
    if [ -r /proc/net/tcp ]; then
        ps_hex="$(printf '%04X' "$ps_port")"
        if cat /proc/net/tcp /proc/net/tcp6 2>/dev/null |
            awk -v h="$ps_hex" '$4 == "0A" { n = split($2, a, ":"); if (a[n] == h) f = 1 } END { exit !f }'; then
            echo 0
        else
            echo 1
        fi
        return 0
    fi
    echo 2
}

# Collects every local check of the machine into $tmp/preflight. Reads only.
preflight_local() {
    : >"$tmp/preflight"
    pf_failed=0
    pf_os="$(uname -s)"
    if [ "$pf_os" = "Linux" ]; then
        pf_add ok os "Linux"
    else
        pf_add fail os "only Linux is supported; this is $pf_os"
    fi
    case "$(uname -m)" in
    x86_64 | amd64)
        target="x86_64-unknown-linux-musl"
        pf_add ok arch "x86_64"
        ;;
    aarch64 | arm64)
        target="aarch64-unknown-linux-musl"
        pf_add ok arch "aarch64"
        ;;
    *) pf_add fail arch "no release build for $(uname -m); build from source instead" ;;
    esac
    if [ "$(id -u)" -eq 0 ]; then
        pf_add ok root "running as root"
    elif [ "$service" -eq 0 ]; then
        pf_add ok root "not root: installing for this user (--no-service)"
    else
        pf_add fail root "run as root (sudo), or pass --no-service with a --prefix you can write to"
    fi
    if [ "$(id -u)" -ne 0 ]; then
        pf_dir="$(nearest_dir "$bin_dir")"
        if [ -w "$pf_dir" ]; then
            pf_add ok prefix "$bin_dir is writable"
        else
            pf_add fail prefix "cannot write to $pf_dir; run as root or choose another --prefix"
        fi
    fi
    if command -v curl >/dev/null 2>&1; then
        dl="curl"
        pf_add ok download "curl"
    elif command -v wget >/dev/null 2>&1; then
        dl="wget"
        pf_add ok download "wget"
    else
        pf_add fail download "curl or wget is needed"
    fi
    for pf_tool in sha256sum tar; do
        if command -v "$pf_tool" >/dev/null 2>&1; then
            pf_add ok "$pf_tool" "found"
        else
            pf_add fail "$pf_tool" "$pf_tool is needed"
        fi
    done
    pf_stat=0
    if stat -c %a / >/dev/null 2>&1; then
        pf_stat=1
        pf_add ok stat "GNU stat"
    else
        pf_add fail stat "GNU stat (stat -c) is needed; BSD stat is not enough"
    fi
    check_free disk-temp "$tmp"
    check_free disk-prefix "$prefix"
    if [ "$service" -eq 1 ]; then
        pf_systemd=0
        if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
            pf_systemd=1
            pf_add ok systemd "present"
        else
            pf_add warn systemd "not present: units are skipped and an agent keeps the root layout"
            if [ "$enroll_on" -eq 1 ] && [ "$role" = "agent" ]; then
                pf_add fail enroll "enrolling starts the agent through systemd, which this machine does not run; leave out --hub, --fingerprint and the token and enroll by hand"
            fi
        fi
        pf_need_accounts=0
        pf_adduser_ok=0
        if [ "$role" = "both" ] && [ "$pf_systemd" -eq 0 ]; then
            pf_add fail systemd "--role both runs the hub and the agent through systemd, which this machine does not run"
        fi
        if [ "$role" = "both" ] && [ "$pf_systemd" -eq 1 ] && [ "$pf_stat" -eq 1 ]; then
            classify_host
            if [ "$layout" = "migrated" ]; then
                pf_add fail agent "an agent that runs as domovoi-agent is installed here; it cannot share /etc/domovoi with the hub. Install the hub alone with --role hub, or move the agent to another machine"
            fi
        fi
        if [ "$hub_part" -eq 1 ]; then
            pf_adduser_ok=1
            if ! id domovoi >/dev/null 2>&1; then pf_need_accounts=1; fi
        elif [ "$pf_systemd" -eq 1 ] && [ "$pf_stat" -eq 1 ]; then
            classify_host
            if [ "$layout" = "fresh" ] || [ "$layout" = "migrated" ]; then
                if ! command -v getent >/dev/null 2>&1; then
                    pf_add fail getent "getent is needed to check the domovoi-agent and domovoi-skill accounts"
                fi
                if ! id domovoi-agent >/dev/null 2>&1 || ! id domovoi-skill >/dev/null 2>&1; then
                    pf_need_accounts=1
                fi
            fi
        fi
        if [ "$pf_need_accounts" -eq 1 ]; then
            if command -v useradd >/dev/null 2>&1; then
                pf_add ok useradd "found"
            elif [ "$pf_adduser_ok" -eq 1 ] && command -v adduser >/dev/null 2>&1; then
                pf_add ok adduser "found"
            else
                pf_add fail useradd "useradd is needed to create the system accounts"
            fi
        fi
        if [ "$hub_part" -eq 1 ]; then
            for pf_port in 7420 7421; do
                pf_busy="$(port_state "$pf_port")"
                case "$pf_busy" in
                0)
                    if [ -e /etc/systemd/system/domovoi-hub.service ]; then
                        pf_add warn "port-$pf_port" "in use, probably by the hub that is installed"
                    else
                        pf_add fail "port-$pf_port" "already in use by another program"
                    fi
                    ;;
                1) pf_add ok "port-$pf_port" "free" ;;
                esac
            done
        fi
    fi
}

# Prints the preflight rows as a table; a failed preflight is printed even with --quiet.
print_preflight() {
    pp_quiet="$quiet"
    if [ "$pf_failed" -gt 0 ]; then quiet=0; fi
    say "Preflight:"
    while IFS='|' read -r pp_status pp_name pp_detail; do
        case "$pp_status" in
        ok) pp_color="$c_green" ;;
        warn) pp_color="$c_yellow" ;;
        *) pp_color="$c_red" ;;
        esac
        say_c "$pp_color" "$(printf '  %-5s %-12s %s' "$pp_status" "$pp_name" "$pp_detail")"
    done <"$tmp/preflight"
    quiet="$pp_quiet"
}

# A one-line text of every failed row, for the error message.
preflight_failures() {
    sed -n 's/^fail|\([^|]*\)|\(.*\)$/\1: \2/p' "$tmp/preflight" | tr '\n' ';' | sed 's/;$//; s/;/; /g'
}

# The fetch and resolve_latest functions of the downloader that preflight found.
define_downloader() {
    if [ "$dl" = "curl" ]; then
        fetch() { curl -fsSL --proto '=https,http' -o "$2" "$1"; }
        resolve_latest() { curl -fsSLI -o /dev/null -w '%{url_effective}' "$RELEASE_BASE/latest"; }
    else
        fetch() { wget -q -O "$2" "$1"; }
        resolve_latest() {
            wget -S --spider "$RELEASE_BASE/latest" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1 | tr -d '\r'
        }
    fi
}

# Turns "latest" into the tag it points at, and sets $name.
resolve_version() {
    if [ "$version" = "latest" ]; then
        url="$(resolve_latest)" || fail_with 20 "could not find the latest release at $RELEASE_BASE/latest"
        url="${url%/}"
        version="${url##*/}"
        case "$version" in
        v*) ;;
        *) fail_with 20 "could not find the latest release at $RELEASE_BASE/latest (got $url)" ;;
        esac
    fi
    name="domovoi-$version-$target"
}

# Compares two versions on their first four dot-separated numbers; prints lt, eq or gt.
ver_cmp() {
    vc_a="${1#v}"
    vc_b="${2#v}"
    vc_a="${vc_a%%[-+]*}"
    vc_b="${vc_b%%[-+]*}"
    vc_i=1
    while [ "$vc_i" -le 4 ]; do
        vc_x="$(printf '%s' "$vc_a" | cut -d. -f"$vc_i")"
        vc_y="$(printf '%s' "$vc_b" | cut -d. -f"$vc_i")"
        case "$vc_x" in "" | *[!0-9]*) vc_x=0 ;; esac
        case "$vc_y" in "" | *[!0-9]*) vc_y=0 ;; esac
        if [ "$vc_x" -lt "$vc_y" ]; then
            echo lt
            return 0
        elif [ "$vc_x" -gt "$vc_y" ]; then
            echo gt
            return 0
        fi
        vc_i=$((vc_i + 1))
    done
    echo eq
}

# Sets $state to fresh, upgrade or downgrade from the binary that is installed, and adds the
# row of it to the preflight.
detect_state() {
    state="fresh"
    installed_version=""
    if [ -x "$bin" ]; then
        installed_version="$("$bin" version 2>/dev/null | head -n 1 |
            sed -n 's/^[^0-9]*\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*[-0-9A-Za-z.]*\).*/\1/p' | sed 's/\.$//')" || installed_version=""
        state="upgrade"
        if [ -n "$installed_version" ] && [ "$(ver_cmp "$version" "$installed_version")" = "lt" ]; then
            state="downgrade"
        fi
    fi
    case "$state" in
    fresh) pf_add ok state "fresh install of $version" ;;
    upgrade)
        if [ -z "$installed_version" ]; then
            pf_add ok state "upgrade: a binary is installed at $bin; it will be replaced by $version"
        elif [ "$(ver_cmp "$version" "$installed_version")" = "eq" ]; then
            pf_add ok state "upgrade: $installed_version is installed; $version is the same version"
        else
            pf_add ok state "upgrade: $installed_version -> $version"
        fi
        ;;
    downgrade)
        if [ "$force_downgrade" -eq 1 ]; then
            pf_add warn state "downgrade: $installed_version -> $version (--force-downgrade)"
        else
            pf_add fail state "downgrade: $installed_version is installed and $version is older; pass --force-downgrade to go back"
        fi
        ;;
    esac
}

# The 0.1.0 release replaces the whole database schema with one baseline migration. A
# database written by an older release has migration 1 recorded with different contents, so
# the hub's start stops with "database migration failed ... previously applied but has been
# modified"; it takes no backup and migrates nothing. Fails the preflight when an installed
# hub older than 0.1.0 has such a database and the target is 0.1.0 or later, unless
# --accept-data-reset was given; then it warns and sets $reset_pending.
check_schema_reset() {
    [ "$hub_part" -eq 1 ] || return 0
    [ -n "$installed_version" ] || return 0
    [ "$(ver_cmp "$installed_version" 0.1.0)" = "lt" ] || return 0
    [ "$(ver_cmp "$version" 0.1.0)" != "lt" ] || return 0
    [ -e "$hub_data_dir/hub.db" ] || return 0
    if [ "$accept_reset" -eq 1 ]; then
        reset_pending=1
        pf_add warn schema "going on (--accept-data-reset): the hub database in $hub_data_dir is from before 0.1.0 and is left untouched; $version cannot open it, so the hub is not restarted. Move $hub_data_dir aside and set the hub up again"
    else
        pf_add fail schema "the hub database in $hub_data_dir is from $installed_version, and $version replaces the whole database schema: it cannot be upgraded, and the hub from $version would stop at start with a migration error (it changes none of the data). Move $hub_data_dir aside or delete it and set the hub up again, or pass --accept-data-reset to install anyway and do that yourself"
    fi
}

# --- Plan (--dry-run) ---------------------------------------------------------------------
plan_add() { # ACTION TEXT
    printf '%-8s %s\n' "$1" "$2" >>"$tmp/plan"
}
plan_dir() { # PATH MODE OWNER
    if [ -d "$1" ]; then plan_add keep "$1 (exists; mode and owner are not changed)"; else plan_add create "$1 (mode $2, owner $3)"; fi
}
plan_account() {
    if id "$1" >/dev/null 2>&1; then plan_add keep "user $1 (exists; checked, not changed)"; else plan_add create "system user $1"; fi
}
plan_unit() { # NAME FUNCTION
    pn_path="/etc/systemd/system/$1"
    if [ ! -e "$pn_path" ]; then
        plan_add create "$pn_path"
    else
        "$2" | sed "s|/usr/local/bin/domovoi|$bin|" >"$tmp/plan-unit"
        if cmp -s "$tmp/plan-unit" "$pn_path"; then
            plan_add keep "$pn_path (up to date)"
        else
            plan_add keep "$pn_path (differs from this release; the new text goes to $prefix/share/domovoi/$1)"
        fi
    fi
}

# Writes what the install would do to $tmp/plan, from the state preflight found.
build_plan() {
    : >"$tmp/plan"
    plan_add download "$RELEASE_BASE/download/$version/$name.tar.gz and its .sha256"
    plan_add verify "the SHA-256 of the archive"
    if [ "$state" = "fresh" ]; then
        plan_add create "$bin (domovoi $version)"
    else
        plan_add replace "$bin (${installed_version:-unknown version} -> $version)"
    fi
    plan_add update "$prefix/share/domovoi/examples (the release examples)"
    if [ "$state" = "fresh" ]; then
        plan_add note "if this fails before the first service is started, what this run created is removed again (--keep-on-failure keeps it)"
    fi
    if [ "$service" -eq 0 ]; then
        plan_add skip "user, directories and systemd units (--no-service)"
    elif [ "$hub_part" -eq 1 ]; then
        plan_account domovoi
        plan_dir /etc/domovoi 0750 root:domovoi
        plan_dir /var/lib/domovoi 0750 domovoi:domovoi
        plan_unit domovoi-hub.service hub_unit
        plan_add reload "systemctl daemon-reload (when systemd is present)"
        if [ -n "$set_public_url$set_bind$set_agent_url" ]; then
            plan_add write "$hub_env (mode 0640, owner root:domovoi when created); only these keys, other lines kept:"
            if [ -n "$set_bind" ]; then plan_add set "  DOMOVOI_WEB__BIND=$set_bind"; fi
            if [ -n "$set_public_url" ]; then plan_add set "  DOMOVOI_WEB__PUBLIC_URL=$set_public_url"; fi
            if [ -n "$set_agent_url" ]; then plan_add set "  DOMOVOI_AGENT_SERVER__PUBLIC_URL=$set_agent_url"; fi
        fi
        if [ -s "$tmp/admin.pw" ]; then
            if [ -e /etc/systemd/system/domovoi-hub.service ] || [ -n "$(ls -A /var/lib/domovoi 2>/dev/null)" ]; then
                plan_add skip "the admin account (this hub already has data; no password is reset)"
            else
                plan_add create "the admin account via 'hub set-password --password-stdin' as domovoi, before the first start"
            fi
        fi
        if [ "$start" -eq 0 ]; then
            plan_add note "the hub is not started (--no-start); the next steps are printed at the end"
        elif [ "$reset_pending" -eq 1 ]; then
            plan_add note "the hub is not restarted: its database from before 0.1.0 has to be moved aside first (--accept-data-reset); the steps are printed at the end"
        elif [ "$have_systemd" -eq 0 ]; then
            plan_add note "the hub is not started: this machine has no systemd; the next steps are printed at the end"
        elif [ -e /etc/systemd/system/domovoi-hub.service ]; then
            plan_add restart "domovoi-hub if it is running, then wait up to ${DOMOVOI_READY_TIMEOUT:-60} s for GET /readyz"
        else
            plan_add start "systemctl enable --now domovoi-hub, then wait up to ${DOMOVOI_READY_TIMEOUT:-60} s for GET /readyz"
        fi
        if [ "$role" = "both" ]; then plan_both_agent; fi
    elif [ "$migrate" -eq 1 ]; then
        plan_add migrate "the agent moves from root to domovoi-agent (see the plan above); the typed word is asked for"
    else
        classify_host
        case "$layout" in
        fresh | migrated)
            plan_account domovoi-agent
            plan_account domovoi-skill
            plan_dir /etc/domovoi 0750 root:domovoi-agent
            plan_dir /etc/domovoi/skills 0755 root:root
            plan_dir /var/lib/domovoi-agent 0700 domovoi-agent:domovoi-agent
            plan_dir /var/lib/domovoi-packages 0755 domovoi-agent:domovoi-agent
            plan_dir /var/lib/domovoi-helper 0700 root:root
            if [ -e /etc/domovoi/helper.toml ]; then
                plan_add keep "/etc/domovoi/helper.toml (exists)"
            else
                plan_add create "/etc/domovoi/helper.toml (mode 0600, owner root:root)"
            fi
            runner_skills_dir="/etc/domovoi/skills"
            runner_packages_dir="/var/lib/domovoi-packages"
            plan_unit domovoi-helper.socket helper_socket_unit
            plan_unit domovoi-helper@.service helper_service_unit
            plan_unit domovoi-helper-selftest.service helper_selftest_unit
            plan_unit domovoi-skill-runner.socket runner_socket_unit
            plan_unit domovoi-skill-runner@.service render_runner_service
            plan_unit domovoi-agent.service agent_unit
            plan_add start "domovoi-helper.socket, then the self-test, then domovoi-skill-runner.socket"
            ;;
        *)
            plan_dir /etc/domovoi 0750 root:root
            plan_dir /etc/domovoi/skills 0750 root:root
            plan_dir /var/lib/domovoi-agent 0700 root:root
            plan_unit domovoi-agent.service legacy_agent_unit
            plan_add note "the agent keeps the root layout on this host ($layout)"
            ;;
        esac
        if [ "$enroll_on" -eq 1 ]; then
            plan_enroll
        elif [ -e "$agent_unit_path" ] && [ -e "$agent_toml" ] && ! journal_exists; then
            plan_add journal "domovoi agent journal init: this agent has no action journal and would not start; the agent is restarted after it"
        fi
    fi
}

# The agent lines of the plan for --role both: the root layout, the unit, and the token dance.
plan_both_agent() {
    plan_dir /etc/domovoi/skills 0750 root:root
    plan_dir /var/lib/domovoi-agent 0700 root:root
    plan_unit domovoi-agent.service legacy_agent_unit
    plan_add note "the agent runs as root on this host: the hub and the agent share /etc/domovoi"
    if [ -e "$agent_unit_path" ] && [ -e /etc/domovoi/agent.toml ]; then
        plan_add keep "the agent here is already enrolled; it is restarted if it is running, not enrolled again"
        return 0
    fi
    plan_add token "stop the hub, make a 15 minute enrollment token for $enroll_name with 'hub token create' as domovoi, start the hub again (the token stays in a private temporary file)"
    plan_add enroll "domovoi agent enroll --hub $enroll_hub --name $enroll_name (the token goes only in the environment of that command)"
    plan_add journal "domovoi agent journal init, unless the action journal in $(agent_state_dir) exists (the agent does not start without it)"
    plan_add start "systemctl enable --now domovoi-agent, then wait up to $agent_timeout s for 'agent status' to show connected"
}

# The enrollment lines of the plan. The token itself is never named.
plan_enroll() {
    plan_add enroll "domovoi agent enroll --hub $enroll_hub --fingerprint $enroll_fp --name $enroll_name (the token goes only in the environment of that command)"
    plan_add journal "domovoi agent journal init, unless the action journal in $(agent_state_dir) exists (the agent does not start without it)"
    plan_add start "systemctl enable --now domovoi-agent, then wait up to $agent_timeout s for 'agent status' to show connected"
}

# Prints the plan.
print_plan() {
    say "Plan for $role $version (dry run; nothing is changed or downloaded):"
    while IFS= read -r pl_line; do
        say "  $pl_line"
    done <"$tmp/plan"
}

# --- Hub settings: hub.env, admin account, start -------------------------------------------
# The value of the last KEY= line of FILE, or nothing.
env_get() { # FILE KEY
    [ -r "$1" ] || return 0
    sed -n "s/^$2=//p" "$1" | tail -n 1
}

# Sets KEY=VALUE in FILE: the first KEY= line is replaced, further ones are dropped, and the
# line is appended when there is none. Every other line is kept as it is.
env_set() { # FILE KEY VALUE
    es_new="$tmp/env.new"
    EV="$3" awk -v key="$2" '
        index($0, key "=") == 1 { if (!done) { print key "=" ENVIRON["EV"]; done = 1 }; next }
        { print }
        END { if (!done) print key "=" ENVIRON["EV"] }' "$1" >"$es_new"
    cat "$es_new" >"$1"
}

# The primary IPv4 address of this machine, or nothing.
detect_ip() {
    di=""
    if command -v ip >/dev/null 2>&1; then
        di="$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p' | head -n 1)"
    fi
    if [ -z "$di" ] && command -v hostname >/dev/null 2>&1; then
        di="$(hostname -I 2>/dev/null | tr ' ' '\n' | grep -E '^[0-9]+(\.[0-9]+){3}$' | grep -v '^127\.' | head -n 1)"
    fi
    printf '%s\n' "$di" | grep -E '^[0-9]+(\.[0-9]+){3}$' || true
}

# The host part of an http, https or wss URL, brackets kept for IPv6.
url_host() {
    uh="${1#*://}"
    uh="${uh%%/*}"
    case "$uh" in
    \[*) printf '%s\n' "${uh%%]*}]" ;;
    *) printf '%s\n' "${uh%%:*}" ;;
    esac
}

# Refuses a URL the hub config would reject, before anything changes. SCHEMES is a grep -E
# alternation such as "https?".
check_url() { # FLAG VALUE SCHEMES
    printf '%s\n' "$2" | grep -Eq "^($3)://(\[[0-9A-Fa-f:]+\]|[A-Za-z0-9._-]+)(:[0-9]{1,5})?/?\$" ||
        fail_with 2 "$1 $2 is not usable: it must look like $4, with no path, spaces or quotes"
}

# Checks the hub settings flags and works out what to write. Sets $set_public_url, $set_bind,
# $set_agent_url; the ones that are empty are left as they are. Exit 2 for a bad value.
resolve_net() {
    set_public_url=""
    set_bind=""
    set_agent_url=""
    rn_lan=0
    if [ -n "$public_url" ]; then
        check_url --public-url "$public_url" "https?" "https://domovoi.lan or http://192.168.1.50:7420"
        set_public_url="${public_url%/}"
    fi
    if [ -n "$agent_url" ]; then
        check_url --agent-url "$agent_url" "wss" "wss://192.168.1.50:7421"
        set_agent_url="${agent_url%/}"
    fi
    case "$bind_opt" in
    "") ;;
    lan)
        rn_lan=1
        set_bind="0.0.0.0:7420"
        ;;
    loopback) set_bind="127.0.0.1:7420" ;;
    *)
        printf '%s\n' "$bind_opt" | grep -Eq '^([0-9]{1,3}(\.[0-9]{1,3}){3}|\[[0-9A-Fa-f:]+\]):[0-9]{1,5}$' ||
            fail_with 2 "--bind $bind_opt is not usable: use lan, loopback, or a numeric HOST:PORT such as 192.168.1.50:7420"
        rn_port="${bind_opt##*:}"
        [ "$rn_port" -ge 1 ] && [ "$rn_port" -le 65535 ] ||
            fail_with 2 "--bind $bind_opt has a port outside 1 to 65535"
        set_bind="$bind_opt"
        ;;
    esac
    rn_have_public="$(env_get "$hub_env" DOMOVOI_WEB__PUBLIC_URL)"
    if [ "$rn_lan" -eq 1 ] && [ -z "$set_public_url" ] && [ -z "$rn_have_public" ]; then
        rn_ip="$(detect_ip)"
        [ -n "$rn_ip" ] ||
            fail_with 2 "--bind lan could not find this machine's IPv4 address; pass --public-url http://ADDRESS:7420"
        set_public_url="http://$rn_ip:7420"
        say "Derived the web address $set_public_url from this machine's address; pass --public-url to choose another."
    fi
    rn_have_agent="$(env_get "$hub_env" DOMOVOI_AGENT_SERVER__PUBLIC_URL)"
    if [ "$rn_lan" -eq 1 ] && [ -z "$set_agent_url" ] && [ -z "$rn_have_agent" ]; then
        rn_pub="${set_public_url:-$rn_have_public}"
        if [ -n "$rn_pub" ]; then
            set_agent_url="wss://$(url_host "$rn_pub"):7421"
            say "Derived the agent address $set_agent_url from the web address; pass --agent-url to choose another."
        fi
    fi
    rn_eff="${set_public_url:-$rn_have_public}"
    case "$rn_eff" in
    http://127.* | http://localhost* | "http://[::1]"*) ;;
    http://*)
        warn "the web address $rn_eff is plain http on a network address: the login and the session travel unencrypted. Put a reverse proxy with TLS in front and use its https address with --public-url."
        ;;
    esac
}

# Checks that FILE is a regular file, not a link, with mode 0600 or 0400 and owned by the
# user running this script, and copies its first line to DEST (0600). FLAG names the option
# in the messages. Exit 2 for anything else; the content is never printed.
read_secret_file() { # FLAG FILE DEST
    [ -f "$2" ] && [ ! -L "$2" ] ||
        fail_with 2 "$1 $2 must be a regular file, not a link or a directory"
    rs_stat="$(stat -c '%a %u' "$2" 2>/dev/null)" ||
        fail_with 2 "cannot read the mode of $2"
    case "$rs_stat" in
    "600 $(id -u)" | "400 $(id -u)") ;;
    *) fail_with 2 "$2 must have mode 0600 or 0400 and be owned by the user running this script (found mode ${rs_stat%% *}, owner uid ${rs_stat#* }); run: chmod 600 $2" ;;
    esac
    sed -n '1{s/\r$//;p;q}' "$2" >"$3" ||
        fail_with 2 "cannot read $2"
    chmod 0600 "$3"
}

# Checks the password source and copies its first line to $tmp/admin.pw (0600, never argv).
prepare_admin_password() {
    if [ -n "$admin_pw_file" ] && [ "$admin_pw_stdin" -eq 1 ]; then
        fail_with 2 "use either --admin-password-file or --admin-password-stdin, not both"
    fi
    if [ -n "$admin_pw_file" ]; then
        read_secret_file --admin-password-file "$admin_pw_file" "$tmp/admin.pw"
    else
        if [ -t 0 ]; then
            fail_with 2 "--admin-password-stdin needs the password piped in, for example: printf '%s\\n' \"\$PW\" | sh install.sh ..."
        fi
        sed -n '1{s/\r$//;p;q}' >"$tmp/admin.pw" || fail_with 2 "cannot read the password from standard input"
    fi
    chmod 0600 "$tmp/admin.pw"
    [ -s "$tmp/admin.pw" ] && [ "$(tr -d '\r\n' <"$tmp/admin.pw" | wc -c)" -gt 0 ] ||
        fail_with 2 "the admin password is empty"
}

# Writes the keys the flags named to hub.env, creating it (0640 root:domovoi) when absent.
# Other lines and keys not named are left as they are.
write_hub_env() {
    [ -n "$set_public_url$set_bind$set_agent_url" ] || return 0
    if [ ! -e "$hub_env" ]; then
        : >"$hub_env"
        manifest_add file "$hub_env"
        chmod 0640 "$hub_env"
        chown root:domovoi "$hub_env"
        say "Created $hub_env"
    fi
    for we_pair in "DOMOVOI_WEB__BIND|$set_bind" "DOMOVOI_WEB__PUBLIC_URL|$set_public_url" \
        "DOMOVOI_AGENT_SERVER__PUBLIC_URL|$set_agent_url"; do
        we_key="${we_pair%%|*}"
        we_val="${we_pair#*|}"
        [ -n "$we_val" ] || continue
        if [ "$(env_get "$hub_env" "$we_key")" = "$we_val" ]; then
            say "$hub_env already has $we_key=$we_val"
        else
            env_set "$hub_env" "$we_key" "$we_val"
            say "Set $we_key=$we_val in $hub_env"
        fi
    done
}

# Runs a command as the domovoi user with the same home the unit gives it.
run_as_domovoi() {
    if command -v runuser >/dev/null 2>&1; then
        runuser -u domovoi -- env HOME=/var/lib/domovoi "$@"
    elif command -v setpriv >/dev/null 2>&1; then
        setpriv --reuid=domovoi --regid=domovoi --init-groups env HOME=/var/lib/domovoi "$@"
    elif command -v su >/dev/null 2>&1; then
        case "$*" in
        *"'"*) fail "cannot run $* as domovoi through su: the path has a quote" ;;
        esac
        su -s /bin/sh domovoi -c "HOME=/var/lib/domovoi exec $*"
    else
        fail "cannot run a command as the domovoi user: none of runuser, setpriv or su is available"
    fi
}

# Creates the admin account with `hub set-password --password-stdin`, as the hub user and
# before the first start. Only on a fresh data directory; an existing admin is never reset.
create_admin() {
    [ -s "$tmp/admin.pw" ] || return 0
    if [ "$upgrade" -eq 1 ] || [ "$hub_fresh_data" -eq 0 ]; then
        say "Skipped the admin account: this hub already has data, so an admin may exist and its password is never reset here."
        return 0
    fi
    if run_as_domovoi "$bin" hub set-password --password-stdin <"$tmp/admin.pw" >"$tmp/setpw.out" 2>&1; then
        admin_created=1
        # The hub's database now exists; a data directory that was there already does not own it.
        if [ "$rollback_armed" -eq 1 ]; then
            find /var/lib/domovoi -mindepth 1 -maxdepth 1 | while IFS= read -r ca_path; do
                manifest_add tree "$ca_path"
            done
        fi
        say "Created the admin account (user name: admin)."
    else
        show_file_err "$tmp/setpw.out"
        fail "could not create the admin account (the hub's message is above; the password needs at least 12 characters)"
    fi
    rm -f "$tmp/admin.pw"
}

# Where the hub answers on this machine: its bind address with a wildcard turned to loopback.
local_web_url() {
    lw="$(env_get "$hub_env" DOMOVOI_WEB__BIND)"
    [ -n "$lw" ] || lw="127.0.0.1:7420"
    case "$lw" in
    0.0.0.0:*) lw="127.0.0.1:${lw#*:}" ;;
    "[::]:"*) lw="[::1]:${lw#\[::\]:}" ;;
    esac
    printf 'http://%s\n' "$lw"
}

# The address to tell the user: the public URL, else the bind address (a wildcard becomes
# this machine's address).
user_web_url() {
    uw="$(env_get "$hub_env" DOMOVOI_WEB__PUBLIC_URL)"
    if [ -n "$uw" ]; then
        printf '%s\n' "$uw"
        return 0
    fi
    uw="$(env_get "$hub_env" DOMOVOI_WEB__BIND)"
    [ -n "$uw" ] || uw="127.0.0.1:7420"
    case "$uw" in
    0.0.0.0:* | "[::]:"*)
        uw_ip="$(detect_ip)"
        printf 'http://%s:%s\n' "${uw_ip:-127.0.0.1}" "${uw##*:}"
        ;;
    *) printf 'http://%s\n' "$uw" ;;
    esac
}

# One probe of GET URL/readyz.
probe_ready() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsS -m 3 -o /dev/null "$1/readyz" >/dev/null 2>&1
    else
        wget -q -T 3 -O /dev/null "$1/readyz" >/dev/null 2>&1
    fi
}

# Hides a setup token in text on stdin.
redact_tokens() {
    sed 's/token=[0-9a-fA-F][0-9a-fA-F]*/token=REDACTED/g'
}

# Waits up to $DOMOVOI_READY_TIMEOUT seconds for GET URL/readyz, with a progress line every
# five seconds. On timeout it shows the last journal lines and stops with exit 40.
wait_ready() {
    wr_url="$1"
    wr_max="${DOMOVOI_READY_TIMEOUT:-60}"
    wr_i=0
    say "Waiting for the hub to answer at $wr_url/readyz (up to $wr_max s)..."
    while ! probe_ready "$wr_url"; do
        if [ "$wr_i" -ge "$wr_max" ]; then
            journalctl -u domovoi-hub -n 30 --no-pager 2>&1 | redact_tokens >"$tmp/journal.out" || true
            show_file_err "$tmp/journal.out"
            fail_with 40 "the hub did not answer $wr_url/readyz within $wr_max s; the last log lines are above. Check: systemctl status domovoi-hub"
        fi
        sleep 1
        wr_i=$((wr_i + 1))
        if [ $((wr_i % 5)) -eq 0 ]; then
            say "  still waiting for the hub ($wr_i of $wr_max s)"
        fi
    done
    say "The hub is ready."
}

# Starts the hub after a fresh install, or restarts it after an upgrade when it was running,
# then waits until it is ready. Sets $started.
start_hub() {
    rollback_disarm
    sh_url="$(local_web_url)"
    if [ "$upgrade" -eq 1 ]; then
        if ! systemctl is-active --quiet domovoi-hub; then
            say "domovoi-hub is not running, so it was left stopped. Start it with: systemctl start domovoi-hub"
            return 0
        fi
        say "Restarting domovoi-hub to run the new binary."
        systemctl restart domovoi-hub || fail "systemctl restart domovoi-hub failed; see: systemctl status domovoi-hub"
    else
        systemctl enable --now domovoi-hub || fail "systemctl enable --now domovoi-hub failed; see: systemctl status domovoi-hub"
        say "Enabled and started domovoi-hub."
    fi
    if [ -z "$(env_get "$hub_env" DOMOVOI_WEB__BIND)" ] && [ -f /etc/domovoi/hub.toml ] &&
        grep -Eq '^[[:space:]]*bind[[:space:]]*=' /etc/domovoi/hub.toml; then
        say "The listen address is set in /etc/domovoi/hub.toml, so the wait for /readyz is skipped."
        started=1
        return 0
    fi
    wait_ready "$sh_url"
    started=1
}

# The newest single-use setup token in the journal, or nothing.
journal_setup_token() {
    journalctl -u domovoi-hub -o cat --no-pager 2>/dev/null |
        sed -n 's/.*\/setup?token=\([0-9a-fA-F][0-9a-fA-F]*\).*/\1/p' | tail -n 1
}

# Sets $setup_url to the setup link, built on the address people open. Retries for a few
# seconds because the hub prints it just after it is ready.
find_setup_link() {
    fs_i=0
    fs_tok=""
    while [ "$fs_i" -lt 8 ]; do
        fs_tok="$(journal_setup_token)"
        [ -z "$fs_tok" ] || break
        fs_i=$((fs_i + 1))
        sleep 1
    done
    if [ -n "$fs_tok" ]; then
        setup_url="$web_url/setup?token=$fs_tok"
    fi
}

# --- Agent enrollment ---------------------------------------------------------------------
# This machine's short host name as Domovoi names hosts: lower case, letters, digits and
# dashes, at most 63 characters. Prints nothing when nothing usable is left.
default_host_name() {
    dh="$(hostname -s 2>/dev/null || hostname 2>/dev/null || uname -n 2>/dev/null)" || dh=""
    printf '%s\n' "$dh" | tr '[:upper:]' '[:lower:]' | sed -e 's/[^a-z0-9]/-/g' -e 's/--*/-/g' -e 's/^-//' |
        cut -c1-63 | sed -e 's/-$//' | head -n 1
}

# Settles $enroll_name: the flag, else this machine's cleaned-up short host name; exit 2 when
# it is not a name Domovoi accepts.
resolve_enroll_name() {
    if [ -z "$enroll_name" ]; then
        enroll_name="$(default_host_name)"
        [ -n "$enroll_name" ] ||
            fail_with 2 "could not make a host name from this machine's name; pass --name"
        say "Using the host name $enroll_name; pass --name to choose another."
    fi
    printf '%s\n' "$enroll_name" | grep -Eq '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$' ||
        fail_with 2 "--name $enroll_name is not usable: use lower-case letters, digits and dashes, at most 63, not starting or ending with a dash"
}

# Checks the enrollment flags and the token, before anything is looked at or changed. Sets
# $enroll_on to 1 when the agent is to be enrolled and started, and leaves the token in
# $tmp/enroll.tok (0600). Exit 2 for a bad or incomplete set.
resolve_enroll() {
    re_env=0
    if [ -n "${DOMOVOI_ENROLL_TOKEN:-}" ]; then re_env=1; fi
    if [ "$role" = "both" ]; then
        [ -z "$enroll_hub$enroll_fp$token_file" ] ||
            fail_with 2 "--role both makes its own enrollment token and the agent dials this hub; --hub, --fingerprint and --token-file go with --role agent. --name and --agent-url are the flags --role both takes"
        [ "$start" -eq 1 ] ||
            fail_with 2 "--no-start cannot be used with --role both: the hub has to run for the agent to enroll. Use --role hub --no-start, then enroll by hand"
        if [ "$service" -eq 0 ]; then
            [ -z "$enroll_name" ] ||
                fail_with 2 "--name needs the service install; it cannot be used with --no-service"
            return 0
        fi
        resolve_enroll_name
        enroll_hub="wss://127.0.0.1:7421"
        if [ -n "$agent_url" ]; then enroll_hub="${agent_url%/}"; fi
        enroll_on=1
        return 0
    fi
    if [ "$role" != "agent" ]; then
        [ -z "$enroll_hub$enroll_fp$enroll_name$token_file" ] ||
            fail_with 2 "--hub, --fingerprint, --name and --token-file go with --role agent (--name also with --role both)"
        return 0
    fi
    re_given=0
    if [ -n "$enroll_hub" ]; then re_given=$((re_given + 1)); fi
    if [ -n "$enroll_fp" ]; then re_given=$((re_given + 1)); fi
    if [ -n "$token_file" ] || [ "$re_env" -eq 1 ]; then re_given=$((re_given + 1)); fi
    if [ "$re_given" -eq 0 ]; then
        [ -z "$enroll_name" ] ||
            fail_with 2 "--name goes with --hub, --fingerprint and a token (--token-file or DOMOVOI_ENROLL_TOKEN)"
        return 0
    fi
    [ "$re_given" -eq 3 ] ||
        fail_with 2 "enrolling needs all of --hub, --fingerprint and a token (--token-file FILE or DOMOVOI_ENROLL_TOKEN); give all three or none"
    if [ -n "$token_file" ] && [ "$re_env" -eq 1 ]; then
        fail_with 2 "use either --token-file or DOMOVOI_ENROLL_TOKEN, not both"
    fi
    [ "$service" -eq 1 ] ||
        fail_with 2 "enrolling needs the service install; it cannot be used with --no-service"
    [ "$start" -eq 1 ] ||
        fail_with 2 "enrolling starts the agent, so it cannot be used with --no-start; enroll by hand with the command from the Add host dialog"
    [ "$migrate" -eq 0 ] ||
        fail_with 2 "--migrate-identity cannot be combined with enrolling"
    check_url --hub "$enroll_hub" "wss" "wss://hub.lan:7421"
    enroll_hub="${enroll_hub%/}"
    printf '%s\n' "$enroll_fp" | grep -Eq '^([0-9A-Fa-f]{2}:){31}[0-9A-Fa-f]{2}$|^[0-9A-Fa-f]{64}$' ||
        fail_with 2 "--fingerprint must be 64 hex digits, with or without colons, as the hub prints it"
    resolve_enroll_name
    if [ -n "$token_file" ]; then
        read_secret_file --token-file "$token_file" "$tmp/enroll.tok"
    else
        (umask 077 && printf '%s\n' "$DOMOVOI_ENROLL_TOKEN" | sed -n '1{s/\r$//;p;q}' >"$tmp/enroll.tok")
        chmod 0600 "$tmp/enroll.tok"
    fi
    # No other command of this script needs to see it.
    unset DOMOVOI_ENROLL_TOKEN
    printf '%s\n' "$(cat "$tmp/enroll.tok")" | grep -Eq '^[A-Za-z0-9_-]{8,512}$' ||
        fail_with 2 "the enrollment token is empty or not in the form the hub prints (letters, digits, - and _)"
    enroll_on=1
}

# Hides the enrollment token in text on stdin: its own value and anything after --token.
scrub_token() {
    st_tok=""
    if [ -s "$tmp/enroll.tok" ]; then st_tok="$(cat "$tmp/enroll.tok")"; fi
    if [ -n "$st_tok" ]; then
        sed -e "s|$st_tok|REDACTED|g" -e 's/--token [^ ]*/--token REDACTED/g'
    else
        sed -e 's/--token [^ ]*/--token REDACTED/g'
    fi
}

# Prints what to run by hand after a failed enrollment, as the next steps of the result.
# PHASE is enroll (no usable token or enrolling failed) or connect.
agent_hint() { # PHASE
    say_always ""
    say_always "To finish the agent by hand:"
    case "$1" in
    connect)
        ns "$bin agent status"
        ns "journalctl -u domovoi-agent -n 50 --no-pager"
        ns "Check that $enroll_hub is reachable from this machine and that the fingerprint is the hub's own; then: systemctl restart domovoi-agent"
        ;;
    journal)
        ns "sudo $bin agent journal init"
        ns "sudo systemctl enable --now domovoi-agent"
        ;;
    *)
        ns "In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here, with sudo (a token works once, so ask for a new one)"
        ns "sudo $bin agent journal init"
        ns "sudo systemctl enable --now domovoi-agent"
        ;;
    esac
}

# The directory of the agent's state: agent.state_dir of agent.toml, else the default.
agent_state_dir() {
    asd_dir=""
    if [ -r "$agent_toml" ]; then
        asd_dir="$(sed -n 's/^[[:space:]]*state_dir[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$agent_toml" | tail -n 1)"
    fi
    printf '%s\n' "${asd_dir:-/var/lib/domovoi-agent}"
}

# Whether the agent's action journal files exist. The agent refuses to start without them.
journal_exists() {
    je_dir="$(agent_state_dir)"
    [ -e "$je_dir/action-journal.json" ] || [ -e "$je_dir/action-journal.bin" ]
}

# Creates the agent's action journal as root with `agent journal init`, which refuses when one
# exists, so it runs only when the journal files are absent. Sets $ej_created to 1 when it ran.
# A failure stops with exit 40 and the command to run by hand. With the argument "step" it
# opens a numbered step first.
ensure_agent_journal() { # [step]
    ej_created=0
    if journal_exists; then return 0; fi
    if [ "${1:-}" = "step" ]; then step journal "Creating the agent's action journal"; fi
    say "Creating the agent's action journal in $(agent_state_dir) (the agent does not start without it)."
    ej_rc=0
    "$bin" agent journal init >"$tmp/journal.out" 2>&1 </dev/null || ej_rc=$?
    if [ "$ej_rc" -ne 0 ]; then
        show_file_err "$tmp/journal.out"
        agent_hint journal
        fail_with 40 "creating the agent's action journal failed (the message is above); run it by hand: sudo $bin agent journal init"
    fi
    show_file "$tmp/journal.out"
    manifest_add file "$(agent_state_dir)/action-journal.json"
    manifest_add file "$(agent_state_dir)/action-journal.bin"
    ej_created=1
}

# For an agent that is already enrolled: makes its journal when an older installer left it
# out, and restarts the agent if it is enabled, so running the installer again heals it.
heal_agent_journal() {
    [ -e "$agent_unit_path" ] && [ -e "$agent_toml" ] || return 0
    ensure_agent_journal
    if [ "$ej_created" -eq 1 ] && systemctl is-enabled --quiet domovoi-agent 2>/dev/null; then
        systemctl restart domovoi-agent || warn "the agent did not restart; check: systemctl status domovoi-agent"
        say "Restarted domovoi-agent now that its journal exists."
    fi
}

# Waits up to $agent_timeout seconds for `agent status` to say the link is connected, with a
# progress line every five seconds. A refused protocol stops it at once; a timeout shows the
# last journal lines (with the token hidden) and stops with exit 40.
wait_agent_link() {
    wl_i=0
    say "Waiting for the agent to connect to $enroll_hub (up to $agent_timeout s)..."
    while :; do
        wl_st="$("$bin" agent status 2>&1 </dev/null)" || wl_st=""
        case "$wl_st" in
        "connected since"*)
            say "The agent is connected: $wl_st"
            return 0
            ;;
        incompatible*)
            agent_hint connect
            fail_with 40 "the hub refused this agent: $(printf '%s\n' "$wl_st" | head -n 1 | scrub_token)"
            ;;
        esac
        if [ "$wl_i" -ge "$agent_timeout" ]; then
            journalctl -u domovoi-agent -n 30 --no-pager 2>&1 | scrub_token >"$tmp/agent-journal.out" || true
            show_file_err "$tmp/agent-journal.out"
            agent_hint connect
            fail_with 40 "the agent did not connect to $enroll_hub within $agent_timeout s; its last log lines are above. Last status: ${wl_st:-none}"
        fi
        sleep 1
        wl_i=$((wl_i + 1))
        if [ $((wl_i % 5)) -eq 0 ]; then
            say "  still waiting for the agent ($wl_i of $agent_timeout s)"
        fi
    done
}

# Enrolls this machine's agent as root with the token passed only through the environment of
# that one command, then enables and starts the unit and waits until it is connected. Needs
# $enroll_hub, $enroll_fp, $enroll_name and the token in $tmp/enroll.tok.
enroll_and_start_agent() {
    es_tok="$(cat "$tmp/enroll.tok")"
    es_rc=0
    es_had_toml=0
    if [ -e "$agent_toml" ]; then es_had_toml=1; fi
    DOMOVOI_ENROLL_TOKEN="$es_tok" "$bin" agent enroll --hub "$enroll_hub" \
        --fingerprint "$enroll_fp" --name "$enroll_name" >"$tmp/enroll.out" 2>&1 </dev/null || es_rc=$?
    scrub_token <"$tmp/enroll.out" >"$tmp/enroll.shown"
    if [ "$es_rc" -ne 0 ]; then
        show_file_err "$tmp/enroll.shown"
        agent_hint enroll
        fail_with 40 "enrolling the agent failed (the message is above); the token may be used up or expired, or the fingerprint or address may be wrong"
    fi
    show_file "$tmp/enroll.shown"
    if [ "$es_had_toml" -eq 0 ]; then manifest_add file "$agent_toml"; fi
    enrolled_host="$enroll_name"
    ensure_agent_journal step
    rollback_disarm
    systemctl enable --now domovoi-agent || {
        agent_hint connect
        fail_with 40 "systemctl enable --now domovoi-agent failed; see: systemctl status domovoi-agent"
    }
    agent_started=1
    say "Enabled and started domovoi-agent."
    wait_agent_link
}

# --- --role both: the agent of the machine the hub runs on -------------------------------
# After the first start of the hub (which makes its TLS certificate): stops the hub, makes a
# 15 minute token with `hub token create` as the hub user (the command needs the database the
# running hub holds), starts the hub again and waits until it is ready. The token and the
# fingerprint go to $enroll_fp and $tmp/enroll.tok; nothing about them is printed or logged.
both_make_token() {
    say "Making a one-time enrollment token for $enroll_name. The hub is stopped for a moment: the command needs its database."
    systemctl stop domovoi-hub || fail "systemctl stop domovoi-hub failed; see: systemctl status domovoi-hub"
    hub_stopped=1
    mt_rc=0
    : >"$tmp/token.out"
    chmod 0600 "$tmp/token.out"
    run_as_domovoi "$bin" hub token create "$enroll_name" --ttl 15m >"$tmp/token.out" 2>&1 </dev/null || mt_rc=$?
    mt_start=0
    systemctl start domovoi-hub || mt_start=1
    hub_stopped=0
    if [ "$mt_start" -eq 0 ]; then
        if [ -z "$(env_get "$hub_env" DOMOVOI_WEB__BIND)" ] && [ -f /etc/domovoi/hub.toml ] &&
            grep -Eq '^[[:space:]]*bind[[:space:]]*=' /etc/domovoi/hub.toml; then
            say "The listen address is set in /etc/domovoi/hub.toml, so the wait for /readyz is skipped."
        else
            wait_ready "$(local_web_url)"
        fi
    fi
    if [ "$mt_rc" -ne 0 ]; then
        scrub_token <"$tmp/token.out" >"$tmp/token.shown"
        show_file_err "$tmp/token.shown"
        agent_hint enroll
        fail_with 40 "could not make an enrollment token (the hub's message is above); the hub is running"
    fi
    [ "$mt_start" -eq 0 ] || {
        agent_hint enroll
        fail_with 40 "could not start the hub again after making the token; see: systemctl status domovoi-hub"
    }
    mt_tok="$(sed -n 's/.* --token \([A-Za-z0-9_-]*\) .*/\1/p' "$tmp/token.out" | head -n 1)"
    mt_fp="$(sed -n 's/^Hub fingerprint: *//p' "$tmp/token.out" | head -n 1 | tr -d ' \r')"
    rm -f "$tmp/token.out" "$tmp/token.shown"
    if [ -z "$mt_tok" ] || ! printf '%s\n' "$mt_tok" | grep -Eq '^[A-Za-z0-9_-]{8,512}$' ||
        ! printf '%s\n' "$mt_fp" | grep -Eq '^([0-9A-Fa-f]{2}:){31}[0-9A-Fa-f]{2}$|^[0-9A-Fa-f]{64}$'; then
        mt_tok=""
        agent_hint enroll
        fail_with 40 "$bin hub token create printed no token or fingerprint this installer can read; the hub is running"
    fi
    (umask 077 && printf '%s\n' "$mt_tok" >"$tmp/enroll.tok")
    chmod 0600 "$tmp/enroll.tok"
    mt_tok=""
    enroll_fp="$mt_fp"
}

# The agent of --role both, after the hub: the root layout of a host that has a hub, the
# unit, then (when the hub runs and this agent is not enrolled yet) the token, the enrollment
# and the start. An agent that is already enrolled is only restarted when it is running.
both_agent_part() {
    layout="hub"
    ba_enrolled=0
    if [ -e "$agent_unit_path" ] && [ -e /etc/domovoi/agent.toml ]; then ba_enrolled=1; fi
    create_legacy_agent_layout
    place_unit domovoi-agent.service legacy_agent_unit 0
    systemctl daemon-reload
    if [ "$ba_enrolled" -eq 1 ]; then
        ensure_agent_journal
        if systemctl is-active --quiet domovoi-agent ||
            { [ "$ej_created" -eq 1 ] && systemctl is-enabled --quiet domovoi-agent 2>/dev/null; }; then
            systemctl restart domovoi-agent || warn "the agent did not restart; check: systemctl status domovoi-agent"
            say "Restarted domovoi-agent to run the new binary."
        fi
        say "The agent on this machine is already enrolled, so it was not enrolled again."
        return 0
    fi
    if [ "$started" -ne 1 ]; then
        say "The hub is not running, so the agent was not enrolled."
        both_wait=1
        return 0
    fi
    step enroll "Enrolling and starting the agent"
    fail_code=40
    both_make_token
    enroll_and_start_agent
}

# --- Final summary ------------------------------------------------------------------------
record_next() {
    next_list="$next_list${next_list:+
}$1"
}
# One next step: printed and kept for the JSON result.
ns() {
    say_always "  $1"
    record_next "$1"
}

# The summary of a hub that is running: the address to open, how to sign in, what is left for
# a person, and the commands to check on it. The setup link is a secret: it is printed here
# and put in the JSON result, and the log file gets a placeholder.
hub_started_summary() {
    web_url="$(user_web_url)"
    if [ "$upgrade" -eq 1 ]; then
        say_always "The hub was restarted on the new version and answers at $web_url"
    else
        say_always "The hub is running. Open: $web_url"
        if [ "$admin_created" -eq 1 ]; then
            say_always "  Sign in as admin with the password you supplied; there is no setup link."
        else
            find_setup_link
            if [ -n "$setup_url" ]; then
                say_always "  Create the admin account with this single-use link (valid 24 hours):"
                log_line "  <setup link: not written to the log>"
                printf '  %s\n' "$setup_url"
            else
                say_always "  The setup link was not found in the journal yet. Get it with:"
                say_always "    journalctl -u domovoi-hub -o cat | grep '/setup?token='"
            fi
        fi
        say_always ""
        say_always "Still to do by hand:"
        ns "Settings: add the Discord bot token and channel (create the bot in the Discord developer portal)"
        ns "Settings: add an LLM provider and its API key"
        if [ -z "$(env_get "$hub_env" DOMOVOI_AGENT_SERVER__PUBLIC_URL)" ]; then
            ns "Settings, Hub: set the Agent server URL (wss://HOST:7421), the address agents dial"
        fi
        if [ "$role" = "both" ]; then
            ns "Hosts, Add host: enroll any other machine you want to watch (install.sh --role agent on it)"
        else
            ns "Hosts, Add host: enroll each machine you want to watch (install.sh --role agent on it)"
        fi
    fi
    if [ "$role" = "both" ] && [ -n "$enrolled_host" ]; then
        say_always ""
        say_always "The agent of this machine is enrolled as host $enrolled_host and connected."
    elif [ "$role" = "both" ]; then
        say_always ""
        say_always "The agent of this machine was set up earlier and is enrolled; it was not enrolled again."
    fi
    say_always ""
    say_always "Check on it:"
    ns "systemctl status domovoi-hub"
    ns "journalctl -u domovoi-hub -f"
    if [ "$role" = "both" ]; then
        ns "systemctl status domovoi-agent"
        ns "$bin agent status"
    fi
}
# The summary of an agent that was enrolled and is connected.
agent_started_summary() {
    say_always "The agent is enrolled as host $enrolled_host and connected to $enroll_hub."
    say_always ""
    say_always "Check on it:"
    ns "systemctl status domovoi-agent"
    ns "journalctl -u domovoi-agent -f"
    ns "$bin agent status"
}
finish_summary() {
    say_always ""
    if [ "$dry_run" -eq 1 ]; then
        say_always "Dry run done: domovoi $version ($role) would be installed at $bin."
    else
        say_always "Done: domovoi $version ($role) is installed at $bin."
    fi
    if [ "$log_keep" -eq 1 ]; then say_always "Log file: $log_file"; fi
}

# Rejects a value the installer cannot use, before anything is looked at or changed.
validate_inputs() {
    case "$version" in
    latest) ;;
    *[!0-9A-Za-z.-]*) fail_with 2 "--version $version is not a release tag; use latest or a tag such as v0.1.0" ;;
    *)
        printf '%s\n' "$version" | grep -Eq '^v?[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)?$' ||
            fail_with 2 "--version $version is not a release tag; use latest or a tag such as v0.1.0"
        ;;
    esac
    case "$prefix" in
    /*) ;;
    *) fail_with 2 "--prefix must be an absolute path, not $prefix" ;;
    esac
    case "$RELEASE_BASE" in
    *@* | *" "*) fail_with 2 "DOMOVOI_RELEASE_BASE must not contain a user name or a space" ;;
    https://*) ;;
    http://127.0.0.1 | http://127.0.0.1[:/]* | http://localhost | http://localhost[:/]* | "http://[::1]" | "http://[::1]"[:/]*) ;;
    http://*)
        [ "${DOMOVOI_ALLOW_HTTP:-}" = "1" ] ||
            fail_with 2 "DOMOVOI_RELEASE_BASE uses plain http, which anyone on the network can change; use https, or set DOMOVOI_ALLOW_HTTP=1 to allow it"
        ;;
    *) fail_with 2 "DOMOVOI_RELEASE_BASE must start with https://" ;;
    esac
    case "$version" in
    latest | v*) ;;
    *) version="v$version" ;;
    esac
}

prefix="/usr/local"
service=1
migrate=0
rollback=0
upgrade=0
installed_version=""
state="fresh"
target=""
name=""
dl=""
min_free_mb="${DOMOVOI_MIN_FREE_MB:-200}"
case "$min_free_mb" in "" | *[!0-9]*) min_free_mb=200 ;; esac
if [ "${DOMOVOI_NONINTERACTIVE:-}" = "1" ]; then yes=1; fi

trap on_exit EXIT
trap 'exit 1' INT TERM

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
    --no-start)
        start=0
        shift
        ;;
    --public-url | --bind | --agent-url | --admin-password-file)
        [ $# -ge 2 ] || fail "$1 needs a value"
        case "$1" in
        --public-url) public_url="$2" ;;
        --bind) bind_opt="$2" ;;
        --agent-url) agent_url="$2" ;;
        *) admin_pw_file="$2" ;;
        esac
        shift 2
        ;;
    --public-url=* | --bind=* | --agent-url=* | --admin-password-file=*)
        case "$1" in
        --public-url=*) public_url="${1#*=}" ;;
        --bind=*) bind_opt="${1#*=}" ;;
        --agent-url=*) agent_url="${1#*=}" ;;
        *) admin_pw_file="${1#*=}" ;;
        esac
        shift
        ;;
    --admin-password-stdin)
        admin_pw_stdin=1
        shift
        ;;
    --hub | --fingerprint | --name | --token-file)
        [ $# -ge 2 ] || fail "$1 needs a value"
        case "$1" in
        --hub) enroll_hub="$2" ;;
        --fingerprint) enroll_fp="$2" ;;
        --name) enroll_name="$2" ;;
        *) token_file="$2" ;;
        esac
        shift 2
        ;;
    --hub=* | --fingerprint=* | --name=* | --token-file=*)
        case "$1" in
        --hub=*) enroll_hub="${1#*=}" ;;
        --fingerprint=*) enroll_fp="${1#*=}" ;;
        --name=*) enroll_name="${1#*=}" ;;
        *) token_file="${1#*=}" ;;
        esac
        shift
        ;;
    --token | --token=*)
        # The value is never read, kept or logged: a token in a flag lands in the shell
        # history and in the process list.
        fail_with 2 "--token is not accepted: a token on the command line stays in the shell history and the process list. Put it in a file with mode 0600 and use --token-file FILE, or set DOMOVOI_ENROLL_TOKEN"
        ;;
    --accept-data-reset)
        accept_reset=1
        shift
        ;;
    --yes)
        yes=1
        shift
        ;;
    --dry-run)
        dry_run=1
        shift
        ;;
    --json)
        json=1
        shift
        ;;
    --quiet)
        quiet=1
        shift
        ;;
    --verbose)
        verbose=1
        shift
        ;;
    --force-downgrade)
        force_downgrade=1
        shift
        ;;
    --keep-on-failure)
        keep_on_failure=1
        shift
        ;;
    --uninstall)
        uninstall=1
        shift
        ;;
    --purge)
        purge=1
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

[ "$quiet" -eq 0 ] || [ "$verbose" -eq 0 ] || fail "--quiet and --verbose cannot be used together"

if [ "$purge" -eq 1 ] && [ "$uninstall" -eq 0 ]; then
    fail_with 2 "--purge goes with --uninstall"
fi
if [ "$uninstall" -eq 1 ]; then
    uc_bad=""
    if [ -n "$role" ]; then uc_bad="$uc_bad --role"; fi
    if [ "$version" != "latest" ]; then uc_bad="$uc_bad --version"; fi
    if [ "$service" -eq 0 ]; then uc_bad="$uc_bad --no-service"; fi
    if [ "$start" -eq 0 ]; then uc_bad="$uc_bad --no-start"; fi
    if [ "$migrate" -eq 1 ]; then uc_bad="$uc_bad --migrate-identity"; fi
    if [ "$rollback" -eq 1 ]; then uc_bad="$uc_bad --rollback-identity"; fi
    if [ "$keep_on_failure" -eq 1 ]; then uc_bad="$uc_bad --keep-on-failure"; fi
    if [ "$accept_reset" -eq 1 ]; then uc_bad="$uc_bad --accept-data-reset"; fi
    if [ "$force_downgrade" -eq 1 ]; then uc_bad="$uc_bad --force-downgrade"; fi
    if [ -n "$public_url$bind_opt$agent_url$admin_pw_file" ] || [ "$admin_pw_stdin" -eq 1 ]; then
        uc_bad="$uc_bad --public-url/--bind/--agent-url/--admin-password-*"
    fi
    if [ -n "$enroll_hub$enroll_fp$enroll_name$token_file" ]; then uc_bad="$uc_bad --hub/--fingerprint/--name/--token-file"; fi
    [ -z "$uc_bad" ] || fail_with 2 "--uninstall cannot be combined with:$uc_bad. It removes an installation; it does not install"
    if [ "$purge" -eq 1 ] && [ "$yes" -eq 0 ] && [ "$dry_run" -eq 0 ]; then
        ( : </dev/tty ) 2>/dev/null ||
            fail_with 2 "--purge deletes data and asks you to type the word purge on the terminal, and there is none here; pass --yes to confirm without one"
    fi
fi

tmp="$(mktemp -d)" || fail_with 10 "cannot create a temporary directory"
setup_log
exec 3>&1
if [ "$json" -eq 1 ]; then
    exec 1>/dev/null 2>>"$log_file"
fi
init_colors
result_pending=1
log_line "arguments: role=$role version=$version prefix=$prefix service=$service dry_run=$dry_run json=$json yes=$yes"

if [ "$rollback" -eq 1 ]; then
    fail_code=40
    [ "$migrate" -eq 0 ] || fail_with 2 "--rollback-identity and --migrate-identity cannot be used together"
    [ "$(id -u)" -eq 0 ] || fail_with 10 "--rollback-identity must run as root (sudo)"
    if [ ! -d /run/systemd/system ] || ! command -v systemctl >/dev/null 2>&1; then
        fail_with 10 "--rollback-identity needs a systemd host"
    fi
    rollback_apply
    exit 0
fi

if [ "$uninstall" -eq 1 ]; then
    run_uninstall
    exit 0
fi

case "$role" in
hub | both) hub_part=1 ;;
agent) ;;
"")
    usage >&2
    fail "--role is required"
    ;;
*) fail "--role takes hub, agent or both, not $role" ;;
esac

if [ "$migrate" -eq 1 ]; then
    [ "$role" = "agent" ] || fail "--migrate-identity goes with --role agent"
    [ "$service" -eq 1 ] || fail "--migrate-identity cannot be combined with --no-service"
fi

validate_inputs

case "${DOMOVOI_READY_TIMEOUT:-60}" in
"" | *[!0-9]* | 0) fail_with 2 "DOMOVOI_READY_TIMEOUT must be a whole number of seconds, not ${DOMOVOI_READY_TIMEOUT:-}" ;;
esac
if [ -n "$public_url$bind_opt$agent_url$admin_pw_file" ] || [ "$admin_pw_stdin" -eq 1 ]; then
    [ "$hub_part" -eq 1 ] ||
        fail_with 2 "--public-url, --bind, --agent-url and the admin password flags go with --role hub or both"
    [ "$service" -eq 1 ] ||
        fail_with 2 "--public-url, --bind, --agent-url and the admin password flags need the service install; they cannot be used with --no-service"
fi
if [ "$hub_part" -eq 1 ] && [ "$service" -eq 1 ]; then
    resolve_net
    if [ -n "$admin_pw_file" ] || [ "$admin_pw_stdin" -eq 1 ]; then prepare_admin_password; fi
fi
[ "$accept_reset" -eq 0 ] || [ "$hub_part" -eq 1 ] ||
    fail_with 2 "--accept-data-reset goes with --role hub or both: an agent has no database to reset"
resolve_enroll
case "${DOMOVOI_AGENT_TIMEOUT:-30}" in
"" | *[!0-9]* | 0) fail_with 2 "DOMOVOI_AGENT_TIMEOUT must be a whole number of seconds, not ${DOMOVOI_AGENT_TIMEOUT:-}" ;;
esac
agent_timeout="${DOMOVOI_AGENT_TIMEOUT:-30}"
if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then have_systemd=1; fi

bin_dir="$prefix/bin"
bin="$bin_dir/domovoi"
RELEASE_BASE="${RELEASE_BASE%/}"

fail_code=10
if [ "$dry_run" -eq 1 ]; then
    step_total=2
else
    step_total=$((4 + service))
    if [ "$hub_part" -eq 1 ] && [ "$service" -eq 1 ] && [ "$start" -eq 1 ] && [ "$have_systemd" -eq 1 ]; then
        step_total=$((step_total + 1))
    fi
    if [ "$enroll_on" -eq 1 ]; then step_total=$((step_total + 2)); fi
fi

step preflight "Checking this machine"
preflight_local
if [ "$pf_failed" -eq 0 ]; then
    define_downloader
    if [ "$migrate" -eq 1 ]; then migrate_preconditions; fi
    resolve_version
    detect_state
    check_schema_reset
fi
print_preflight
[ "$pf_failed" -eq 0 ] || fail "preflight failed: $(preflight_failures)"
if [ "$reset_pending" -eq 1 ] && [ "$dry_run" -eq 0 ] && [ "$service" -eq 1 ] && [ "$start" -eq 1 ] && [ "$have_systemd" -eq 1 ]; then
    step_total=$((step_total - 1))
    if [ "$role" = "both" ]; then step_total=$((step_total - 1)); fi
fi
if [ "$state" = "downgrade" ]; then
    warn "downgrading from $installed_version to $version (--force-downgrade)"
fi

if [ "$dry_run" -eq 1 ]; then
    step plan "Planning the install"
    build_plan
    if [ "$migrate" -eq 1 ]; then migrate_plan; fi
    print_plan
    say ""
    say "Dry run: nothing was changed, and the archive was not downloaded."
    finish_summary
    exit 0
fi

# The migration shows its plan and asks before anything is downloaded or changed.
if [ "$migrate" -eq 1 ]; then
    migrate_plan
    migrate_confirm
fi

step download "Downloading $name.tar.gz"
fail_code=20
fetch "$RELEASE_BASE/download/$version/$name.tar.gz" "$tmp/$name.tar.gz" ||
    fail "could not download $RELEASE_BASE/download/$version/$name.tar.gz"
fetch "$RELEASE_BASE/download/$version/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" ||
    fail "could not download the checksum for $name.tar.gz"

vsay "Fetched $RELEASE_BASE/download/$version/$name.tar.gz into $tmp"

step verify "Verifying the checksum"
(cd "$tmp" && sha256sum -c "$name.tar.gz.sha256" >/dev/null 2>&1) ||
    fail "the SHA-256 of $name.tar.gz does not match its .sha256 file; nothing was installed"
say "Checksum verified. To check where the archive was built, download it and run"
say "  gh attestation verify $name.tar.gz --repo $REPO"

step install "Installing the binary"
fail_code=30
arm_rollback
tar -C "$tmp" -xzf "$tmp/$name.tar.gz"
[ -f "$tmp/$name/domovoi" ] || fail "$name.tar.gz has no domovoi binary"
"$tmp/$name/domovoi" version >/dev/null 2>&1 || fail "the downloaded binary does not run on this machine"

mkdir_rec "$bin_dir"
if [ ! -e "$bin" ] && [ ! -L "$bin" ]; then manifest_add file "$bin"; fi
install -m 0755 "$tmp/$name/domovoi" "$bin"
say "Installed $("$bin" version | head -n 1) to $bin"

if [ -d "$tmp/$name/examples" ]; then
    examples="$prefix/share/domovoi/examples"
    examples_new=0
    if [ ! -e "$examples" ] && [ ! -L "$examples" ]; then examples_new=1; fi
    mkdir_rec "$examples"
    if [ "$examples_new" -eq 1 ]; then manifest_add tree "$examples"; fi
    cp -R "$tmp/$name/examples/." "$examples/"
fi

if [ "$service" -eq 0 ]; then
    say "Skipped the user, directories and systemd unit (--no-service)."
    finish_summary
    exit 0
fi

if [ "$role" = "both" ]; then
    step service "Setting up the hub and the agent services"
else
    step service "Setting up the $role service"
fi
fail_code=40
if [ "$migrate" -eq 1 ]; then
    migrate_apply
    finish_summary
    exit 0
fi

unit_name="domovoi-$role.service"
if [ "$role" = "both" ]; then unit_name="domovoi-hub.service"; fi
if [ -e "/etc/systemd/system/$unit_name" ]; then
    upgrade=1
fi

if [ "$hub_part" -eq 1 ]; then
    if [ -n "$(ls -A /var/lib/domovoi 2>/dev/null)" ]; then hub_fresh_data=0; fi
    create_hub_layout
    write_hub_env
    create_admin
    place_unit "$unit_name" hub_unit 0
    if [ "$have_systemd" -eq 1 ]; then
        systemctl daemon-reload
    fi
    if [ "$reset_pending" -eq 1 ]; then
        say "The hub was not restarted: its database in $hub_data_dir is from before 0.1.0 and the new version cannot open it."
    elif [ "$start" -eq 1 ] && [ "$have_systemd" -eq 1 ]; then
        step start "Starting the hub"
        fail_code=40
        start_hub
    fi
    if [ "$role" = "both" ]; then both_agent_part; fi
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
    if [ "$enroll_on" -eq 1 ]; then
        step enroll "Enrolling and starting the agent"
        fail_code=40
        enroll_and_start_agent
    elif [ "$upgrade" -eq 1 ] && [ "$service" -eq 1 ] && [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
        heal_agent_journal
    fi
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

say_always ""
if [ "$hub_part" -eq 1 ] && [ "$started" -eq 1 ] && [ "$both_wait" -eq 0 ]; then
    hub_started_summary
    finish_summary
    exit 0
fi
if [ "$role" = "agent" ] && [ "$enroll_on" -eq 1 ]; then
    agent_started_summary
    finish_summary
    exit 0
fi
say_always "Next steps:"
if [ "$reset_pending" -eq 1 ]; then
    say_always "The old hub database is still in $hub_data_dir, and this version cannot open it. To finish:"
    ns "systemctl stop domovoi-hub"
    ns "mv $hub_data_dir $hub_data_dir.before-0.1.0"
    ns "install -d -m 0750 -o domovoi -g domovoi $hub_data_dir"
    ns "systemctl enable --now domovoi-hub"
    ns "journalctl -u domovoi-hub | grep setup     # open this link to create the admin account"
    if [ "$role" = "both" ]; then
        ns "Run this installer again with --role both: it enrolls the agent of this machine"
    fi
elif [ "$role" = "both" ] && [ "$upgrade" -eq 1 ]; then
    ns "systemctl restart domovoi-hub    # runs the new binary; config and data are kept"
    if [ "$both_wait" -eq 1 ]; then
        ns "Run this installer again with --role both once the hub is running: it enrolls the agent of this machine"
    else
        ns "systemctl restart domovoi-agent"
    fi
elif [ "$upgrade" -eq 1 ]; then
    ns "systemctl restart domovoi-$role    # runs the new binary; config and data are kept"
elif [ "$hub_part" -eq 1 ]; then
    ns "systemctl enable --now domovoi-hub"
    ns "journalctl -u domovoi-hub | grep setup     # open this link to create the admin account"
    ns "sudo -u domovoi $bin selftest --offline    # optional: checks the pipeline end to end"
    say_always ""
    say_always "No config file is needed: everything is set in the web UI."
    if [ "$admin_created" -eq 1 ]; then
        say_always "The admin account (user name: admin) exists already, so there is no setup link."
    fi
elif [ "$layout" = "fresh" ] || [ "$layout" = "migrated" ]; then
    say_always "  1. In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here,"
    say_always "     with sudo: the files it writes are handed to the domovoi-agent account automatically."
    say_always "  2. sudo $bin agent journal init"
    say_always "  3. sudo systemctl enable --now domovoi-agent"
    record_next "In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here, with sudo: the files it writes are handed to the domovoi-agent account automatically."
    record_next "sudo $bin agent journal init"
    record_next "sudo systemctl enable --now domovoi-agent"
    say_always ""
    say_always "The helper and skill runner sockets are running. After a runner failure, clear it with:"
    say_always "  systemctl reset-failed 'domovoi-skill-runner@*'"
else
    say_always "  1. In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here."
    say_always "  2. sudo $bin agent journal init"
    say_always "  3. sudo systemctl enable --now domovoi-agent"
    record_next "In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here."
    record_next "sudo $bin agent journal init"
    record_next "sudo systemctl enable --now domovoi-agent"
fi
finish_summary
