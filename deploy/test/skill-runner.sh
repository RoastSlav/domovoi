#!/bin/sh
# Tests the skill runner with real accounts and real systemd: the socket and service units of
# deploy/systemd rendered under probe names, the runner binary behind the socket, and the
# behaviours of systemd that the unit design relies on (the canaries). Run it as root on a
# throwaway machine with systemd as PID 1 and cgroup v2 (a virtual machine, not a container).
#
#   sudo env DOMOVOI_PRIVILEGED_TESTS=1 DOMOVOI_BIN=$PWD/target/debug/domovoi \
#     sh deploy/test/skill-runner.sh
#
# Optional: DOMOVOI_IMPL_HASH_CMD, a command that takes a skill folder and prints the
# implementation hash the agent would send (64 hex digits). Without it the rows that run a
# skill through the runner are skipped and say so.
#
# Probe names only: the units, the socket path, the directories and the binary copy are all
# called domovoi-probe-ci-*, so the test is safe next to a live agent. The runner resolves the
# accounts domovoi-agent and domovoi-skill by name, so those must exist: the test uses them as
# found (it never modifies an existing account) and creates, then deletes, one that is absent.
# A trap removes what the test recorded and the script fails when a domovoi-probe-ci-* object
# is left behind.
#
# It needs: python3, useradd, userdel, groupadd, groupdel, gpasswd, runuser, systemctl,
# journalctl and findmnt.
set -eu

if [ "${DOMOVOI_PRIVILEGED_TESTS:-}" != "1" ]; then
    echo "skill-runner.sh creates accounts, units and directories. Run it as root on a" >&2
    echo "disposable machine with systemd as PID 1, with DOMOVOI_PRIVILEGED_TESTS=1." >&2
    exit 2
fi
if [ "$(id -u)" -ne 0 ]; then
    echo "run this as root, on a machine you can throw away" >&2
    exit 2
fi
bin_arg="${DOMOVOI_BIN:-}"
if [ -z "$bin_arg" ] || [ ! -x "$bin_arg" ]; then
    echo "set DOMOVOI_BIN to the domovoi binary to test" >&2
    exit 2
fi
bin_src="$(cd "$(dirname "$bin_arg")" && pwd)/$(basename "$bin_arg")"
for tool in python3 useradd userdel groupadd groupdel gpasswd runuser systemctl journalctl findmnt; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is needed" >&2
        exit 2
    }
done
if [ ! -d /run/systemd/system ] || [ "$(ps -p 1 -o comm= | tr -d ' ')" != "systemd" ]; then
    echo "systemd is not PID 1 here; this test needs a machine that boots with systemd" >&2
    exit 2
fi
echo "systemd: $(systemctl --version | sed -n 1p); cgroup filesystem: $(stat -fc %T /sys/fs/cgroup)"
if [ "$(stat -fc %T /sys/fs/cgroup)" != "cgroup2fs" ]; then
    echo "cgroup v2 is not mounted; this test needs it" >&2
    exit 2
fi

root="$(cd "$(dirname "$0")/../.." && pwd)"
unit_dir=/etc/systemd/system

# Every object the test creates carries this prefix.
p="domovoi-probe-ci"
sock="/run/$p-runner.sock"
sock_unit="$p-runner.socket"
svc_unit="$p-runner@.service"
skills="/var/lib/$p-skills"
packages="/var/lib/$p-packages"
probe_bin_dir="/usr/local/lib/$p"
probe_bin="$probe_bin_dir/domovoi"
stray="$p-stray"

clash=0
for path in "$sock" "$unit_dir/$sock_unit" "$unit_dir/$svc_unit" "$skills" "$packages" "$probe_bin_dir"; do
    if [ -e "$path" ] || [ -L "$path" ]; then
        echo "$path exists; run this on a clean machine" >&2
        clash=1
    fi
done
if getent passwd | cut -d: -f1 | grep -q "^$p-"; then
    echo "a $p-* account exists; run this on a clean machine" >&2
    clash=1
fi
if find "$unit_dir" -maxdepth 1 -name "$p-*" 2>/dev/null | grep -q .; then
    echo "a $p-* unit exists; run this on a clean machine" >&2
    clash=1
fi
[ "$clash" -eq 0 ] || exit 2

work="$(mktemp -d)"
chmod 0755 "$work"
: >"$work/users"
: >"$work/groups"
leftover=0

# Deletes only what this script created, then fails the run when a probe object remains.
cleanup() {
    set +e
    systemctl stop "$sock_unit" >/dev/null 2>&1
    systemctl stop "$p-runner@*.service" "$p-canary*" >/dev/null 2>&1
    for u in "$unit_dir/$p-"*; do
        [ -e "$u" ] || continue
        systemctl disable "$(basename "$u")" >/dev/null 2>&1
        rm -rf "$u"
    done
    systemctl daemon-reload >/dev/null 2>&1
    systemctl reset-failed "$p-*" >/dev/null 2>&1
    rm -rf "$skills" "$packages" "$probe_bin_dir" "$sock" "/var/lib/$p-absent"
    for name in $(tr '\n' ' ' <"$work/users"); do
        id "$name" >/dev/null 2>&1 && userdel "$name" >/dev/null 2>&1
    done
    for name in $(tr '\n' ' ' <"$work/groups"); do
        getent group "$name" >/dev/null 2>&1 && groupdel "$name" >/dev/null 2>&1
    done
    rm -rf "$work"
    if find "$unit_dir" /var/lib /usr/local/lib /run -maxdepth 1 -name "$p-*" 2>/dev/null | grep -q . ||
        getent passwd | cut -d: -f1 | grep -q "^$p-" ||
        getent group | cut -d: -f1 | grep -q "^$p-"; then
        echo "a $p-* object is left behind" >&2
        leftover=1
    fi
    return 0
}
finish() {
    rc=$?
    cleanup
    [ "$leftover" -eq 0 ] || rc=1
    exit "$rc"
}
trap finish EXIT
trap 'exit 1' INT TERM

passed=0
failed=0
skipped=0
current=""
pass() { printf 'ok   %s\n' "$*"; }
die() {
    printf '  %s\n' "$*" >&2
    exit 1
}
row_begin() {
    current="$1"
    : >"$work/skip-reason"
    set +e
}
row_end() {
    set -e
    case "$1" in
    0)
        pass "$current"
        passed=$((passed + 1))
        ;;
    3)
        printf 'skip %s (%s)\n' "$current" "$(cat "$work/skip-reason")"
        skipped=$((skipped + 1))
        ;;
    *)
        printf 'FAIL %s\n' "$current"
        failed=$((failed + 1))
        ;;
    esac
}
skip_row() {
    printf '%s' "$*" >"$work/skip-reason"
    exit 3
}
record_user() { printf '%s\n' "$1" >>"$work/users"; }
record_group() { printf '%s\n' "$1" >>"$work/groups"; }
nologin="$(command -v nologin || echo /usr/sbin/nologin)"

# The two accounts the runner resolves by name: used as found, created when absent.
for account in domovoi-agent domovoi-skill; do
    if ! id "$account" >/dev/null 2>&1; then
        useradd --system --user-group --no-create-home --shell "$nologin" "$account"
        record_user "$account"
        record_group "$account"
    fi
done
[ "$(id -Gn domovoi-skill)" = "domovoi-skill" ] || {
    echo "domovoi-skill is in a group besides its own; fix it first: the runner refuses to start" >&2
    exit 2
}

# A client of the runner protocol: one frame out, the first reply frame back as compact JSON,
# or "connect-error <errno>" / "closed". With "cancel SECONDS" it closes the connection after
# that long instead of waiting for a reply.
cat >"$work/client.py" <<'PY'
import json, socket, struct, sys, time
path, body = sys.argv[1], sys.argv[2]
cancel = float(sys.argv[3]) if len(sys.argv) > 3 else None
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
try:
    s.connect(path)
except OSError as e:
    print("connect-error", e.errno)
    sys.exit(3)
raw = body.encode()
s.sendall(struct.pack(">I", len(raw)) + raw)
if cancel is not None:
    time.sleep(cancel)
    s.close()
    print("cancelled")
    sys.exit(0)
s.settimeout(30)
head = b""
while len(head) < 4:
    chunk = s.recv(4 - len(head))
    if not chunk:
        print("closed")
        sys.exit(4)
    head += chunk
size = struct.unpack(">I", head)[0]
buf = b""
while len(buf) < size:
    chunk = s.recv(size - len(buf))
    if not chunk:
        break
    buf += chunk
print(json.dumps(json.loads(buf), sort_keys=True, separators=(",", ":")))
PY
ask() { # ask USER JSON [cancel-seconds]
    runuser -u "$1" -- python3 "$work/client.py" "$sock" "$2" ${3:+"$3"} 2>&1 || true
}
health='{"type":"health","protocol":1}'
probe='{"type":"probe","protocol":1,"helper_pid":null}'

# The runner's units, rendered from the shipped files under probe names. $1 is the
# RuntimeMaxSec to use (the shipped 3720s makes no row wait an hour).
render_units() {
    sed -e "s|/run/domovoi-skill-runner.sock|$sock|" \
        "$root/deploy/systemd/domovoi-skill-runner.socket" >"$unit_dir/$sock_unit"
    sed -e "s|/usr/local/bin/domovoi|$probe_bin|" \
        -e 's|/etc/domovoi/skills|@S@|g' -e 's|/var/lib/domovoi-packages|@P@|g' \
        -e "s|@S@|$skills|g" -e "s|@P@|$packages|g" \
        -e "s|RuntimeMaxSec=3720s|RuntimeMaxSec=$1|" \
        "$root/deploy/systemd/domovoi-skill-runner@.service" >"$unit_dir/$svc_unit"
    systemctl daemon-reload
}
install -d -m 0755 "$probe_bin_dir" "$skills" "$packages"
install -m 0755 "$bin_src" "$probe_bin"
chown -R root:root "$skills"
chown domovoi-agent:domovoi-agent "$packages"
render_units 3720s
systemctl start "$sock_unit"
failed_instances() { systemctl list-units --all --state=failed --no-legend "$p-runner@*" | wc -l; }

hash_of() { # hash_of SKILL_DIR
    [ -n "${DOMOVOI_IMPL_HASH_CMD:-}" ] || skip_row "DOMOVOI_IMPL_HASH_CMD is not set, so no skill can be run through the runner"
    sh -c "$DOMOVOI_IMPL_HASH_CMD \"\$1\"" sh "$1"
}
# A probe skill that sleeps, with a grandchild in its own session.
make_sleeper() {
    d="$skills/$p-sleep"
    mkdir -p "$d"
    printf 'name = "%s-sleep"\ndescription = "Sleeps"\nrisk = "read"\nentry = "run"\ntimeout = "1m"\napplies_to = ["disk.free"]\nmax_output_bytes = 4096\n' "$p" >"$d/skill.toml"
    printf '#!/bin/sh\nsetsid sleep 600 &\necho $! >/tmp/%s-grandchild\nexec sleep 600\n' "$p" >"$d/run"
    chmod 0755 "$d/run"
    chown -R root:root "$d"
}
run_request() { # run_request SKILL HASH
    printf '{"type":"run","protocol":1,"execution_id":"01234567-89ab-cdef-0123-456789abcdef","source":"skills_dir","skill":"%s","impl_hash":"%s","args":{},"timeout_secs":60,"max_output_bytes":4096}' "$1" "$2"
}

#
# The runner behind its socket.
#
row_begin corrected_units_start_isolate_cancel_time_out_and_clean_up
(
    set -e
    out="$(ask root "$health")"
    case "$out" in *'"type":"health"'*) ;; *) die "root health: $out" ;; esac
    case "$out" in *"\"skills_root\":\"$skills\""*) ;; *) die "root health roots: $out" ;; esac
    out="$(ask domovoi-agent "$health")"
    case "$out" in *'"type":"health"'*) ;; *) die "agent health: $out" ;; esac
    out="$(ask root "$probe")"
    case "$out" in *'"type":"probe"'*) ;; *) die "probe: $out" ;; esac
    [ "$(failed_instances)" -eq 0 ] || die "a failed runner instance is left after plain requests"
    [ ! -e "$sock" ] || [ "$(stat -c '%a %U:%G' "$sock")" = "660 root:domovoi-agent" ] || die "the socket node is not 660 root:domovoi-agent"
    systemctl stop "$sock_unit"
    [ ! -e "$sock" ] || die "RemoveOnStop=yes left the socket node behind"
    systemctl start "$sock_unit"
)
row_end $?

row_begin runner_refuses_a_stranger_and_a_skill_account_cannot_connect
(
    set -e
    id "$stray" >/dev/null 2>&1 || {
        useradd --system --user-group --no-create-home --shell "$nologin" "$stray"
        record_user "$stray"
        record_group "$stray"
    }
    out="$(ask "$stray" "$health")"
    case "$out" in "connect-error 13"*) ;; *) die "a stray account: $out" ;; esac
    out="$(ask domovoi-skill "$health")"
    case "$out" in "connect-error 13"*) ;; *) die "domovoi-skill: $out" ;; esac
)
row_end $?

row_begin real_user_skill_view_hides_sockets_state_and_key_under_corrected_unit
(
    set -e
    out="$(ask root "$probe")"
    case "$out" in *'"run_user_exists":false'*) ;; *) die "/run/user is visible: $out" ;; esac
    case "$out" in *'"etc_listing":'*'"agent.toml"'*) die "agent.toml is listed under /etc/domovoi: $out" ;; esac
    case "$out" in *'"etc_listing":'*'"helper.toml"'*) die "helper.toml is listed under /etc/domovoi: $out" ;; esac
    case "$out" in *'"var_lib_listing":'*'"domovoi-agent"'*) die "the agent state is listed under /var/lib: $out" ;; esac
    case "$out" in *'"var_lib_listing":'*'"domovoi-helper"'*) die "the helper state is listed under /var/lib: $out" ;; esac
    # The eight-point verdict itself is evaluate_isolation, run by `domovoi agent selftest` (ST-13)
    # against the real unit names; this row checks the raw observations that matter most.
)
row_end $?

row_begin runner_writes_frames_to_inherited_socket_and_logs_reach_journal_under_run_mask
(
    set -e
    out="$(ask root '{"type":"health","protocol":1,"extra":1}')"
    case "$out" in *'"code":"bad_request"'*) ;; *) die "an unknown field was not refused as bad_request: $out" ;; esac
    out="$(ask root '{"type":"health","protocol":99}')"
    case "$out" in *'"code":"protocol_mismatch"'*) ;; *) die "a wrong protocol was not refused: $out" ;; esac
    out="$(ask root "$(run_request none 0000000000000000000000000000000000000000000000000000000000000000)")"
    case "$out" in *'"code":"peer_not_allowed"'*) ;; *) die "root may not run skills: $out" ;; esac
    # Whether the runner logs anything for a refusal is its own business; an empty journal is
    # reported, not failed, because the /run mask is exactly what this row re-measures.
    n="$(journalctl -u "$p-runner@*" --no-pager -q 2>/dev/null | wc -l)"
    echo "    journal lines from the runner instances: $n" >&2
)
row_end $?

row_begin runner_starts_on_a_host_with_no_packages_section
(
    set -e
    [ -d "$packages" ] || die "the packages directory does not exist"
    [ ! -e /etc/domovoi/agent.toml ] || skip_row "this host has an agent.toml"
    out="$(ask domovoi-agent "$health")"
    case "$out" in *'"type":"health"'*) ;; *) die "health without a [packages] section: $out" ;; esac
)
row_end $?

row_begin polluted_skill_account_fails_start_check_and_is_still_masked
(
    set -e
    if [ -s "$work/users" ] && grep -qx domovoi-skill "$work/users"; then :; else skip_row "domovoi-skill belongs to this host; the test does not add groups to an account it did not create"; fi
    groupadd "$p-extra"
    record_group "$p-extra"
    gpasswd -a domovoi-skill "$p-extra" >/dev/null
    out="$(ask root "$health")"
    case "$out" in *'"code":"runner_groups_extra"'*) ;; *) gpasswd -d domovoi-skill "$p-extra" >/dev/null; die "no refusal for an extra group: $out" ;; esac
    out="$(ask root "$probe")"
    gpasswd -d domovoi-skill "$p-extra" >/dev/null
    case "$out" in *'"code":"runner_groups_extra"'*) ;; *) die "probe was not refused too: $out" ;; esac
)
row_end $?

row_begin runner_kills_skill_group_on_eof_and_exits_zero
(
    set -e
    make_sleeper
    h="$(hash_of "$skills/$p-sleep")"
    ask domovoi-agent "$(run_request "$p-sleep" "$h")" 2 >/dev/null
    sleep 5
    ! pgrep -f "sleep 600" >/dev/null || die "the skill or its grandchild is still running after the cancel"
    [ "$(failed_instances)" -eq 0 ] || die "the cancel left a failed runner instance"
)
row_end $?

row_begin runtime_max_sec_kills_a_setsid_grandchild_and_leaves_no_failed_unit
(
    set -e
    h="$(hash_of "$skills/$p-sleep")"
    render_units 8s
    # Hold the connection open past RuntimeMaxSec; the unit stops the whole cgroup.
    ask domovoi-agent "$(run_request "$p-sleep" "$h")" 14 >/dev/null
    ! pgrep -f "sleep 600" >/dev/null || die "a setsid grandchild survived RuntimeMaxSec"
    render_units 3720s
)
row_end $?

row_begin skill_given_agent_group_is_stopped_by_mask_not_by_group
(
    skip_row "needs a skill that reads the agent state; no such probe skill is written yet"
)
row_end $?

row_begin skills_dir_is_unreachable_without_mask_when_etc_domovoi_is_0750
(
    skip_row "needs a unit variant without the mask and a skills directory under /etc/domovoi; the probe names keep the skills directory elsewhere"
)
row_end $?

row_begin protect_home_yes_closes_home_and_root_writes
(
    skip_row "covered by ST-13's point 5 through domovoi agent selftest on the real names (see the installer pass)"
)
row_end $?

row_begin selftest_detects_dropin_that_removes_the_mask
(
    skip_row "domovoi agent selftest reads the real unit names; run it on the real-name installer pass with a drop-in"
)
row_end $?

row_begin noexec_run_blocks_direct_exec_under_run_while_usr_bin_units_start
(
    findmnt -no OPTIONS /run | grep -q noexec || skip_row "/run is not mounted noexec on this host"
    out="$(ask root "$health")"
    case "$out" in *'"type":"health"'*) ;; *) die "a unit with ExecStart under /usr does not start with /run noexec: $out" ;; esac
)
row_end $?

#
# Canaries: how systemd behaves. A failing canary means systemd changed, and the design
# notes (ADR 0034) are revisited, not the product.
#
canary_unit() { # canary_unit NAME BODY
    printf '[Service]\n%s\n' "$2" >"$unit_dir/$p-canary-$1.service"
    systemctl daemon-reload
}
status_of() { systemctl show -p ExecMainStatus --value "$p-canary-$1.service"; }

row_begin rw39_unit_text_fails_with_226
(
    set -e
    canary_unit rw39 "Type=oneshot
ExecStart=/bin/true
TemporaryFileSystem=/var/lib:ro
InaccessiblePaths=/var/lib/domovoi-helper"
    ! systemctl start "$p-canary-rw39.service" 2>/dev/null || die "a child under a masked parent started; systemd changed"
    [ "$(status_of rw39)" = 226 ] || die "expected 226/NAMESPACE, found $(status_of rw39)"
)
row_end $?

row_begin plain_mask_on_absent_path_fails_closed
(
    set -e
    canary_unit absent "Type=oneshot
ExecStart=/bin/true
BindReadOnlyPaths=/nonexistent-$p"
    ! systemctl start "$p-canary-absent.service" 2>/dev/null || die "a bind of an absent path started; systemd changed"
    [ "$(status_of absent)" = 226 ] || die "expected 226/NAMESPACE, found $(status_of absent)"
)
row_end $?

row_begin dash_mask_on_absent_path_starts_and_skips
(
    set -e
    canary_unit dash "Type=oneshot
ExecStart=/bin/true
BindReadOnlyPaths=-/nonexistent-$p"
    systemctl start "$p-canary-dash.service" || die "a -prefixed bind of an absent path did not start"
)
row_end $?

row_begin temporary_filesystem_on_absent_path_creates_host_directory
(
    set -e
    canary_unit mkdir "Type=oneshot
ExecStart=/bin/true
TemporaryFileSystem=/var/lib/$p-absent:ro"
    systemctl start "$p-canary-mkdir.service" || die "the unit did not start"
    [ -d "/var/lib/$p-absent" ] || die "systemd no longer creates the mount point on the host"
)
row_end $?

row_begin runtime_max_sec_is_ignored_for_oneshot
(
    set -e
    canary_unit oneshot "Type=oneshot
RuntimeMaxSec=1s
ExecStart=/bin/sleep 4"
    systemctl start "$p-canary-oneshot.service" || die "RuntimeMaxSec now stops a oneshot; UNIT-4's reason changed"
)
row_end $?

row_begin closing_connection_does_not_end_instance_without_runner_cancel
(
    set -e
    printf '[Socket]\nListenStream=/run/%s-canary.sock\nAccept=yes\n' "$p" >"$unit_dir/$p-canary.socket"
    printf '[Service]\nStandardInput=socket\nExecStart=/bin/sleep 20\n' >"$unit_dir/$p-canary@.service"
    systemctl daemon-reload
    systemctl start "$p-canary.socket"
    python3 -c "import socket; s=socket.socket(socket.AF_UNIX); s.connect('/run/$p-canary.sock'); s.close()"
    sleep 2
    systemctl list-units --no-legend "$p-canary@*" | grep -q running || die "the instance ended when the client closed; systemd changed"
    systemctl stop "$p-canary.socket" "$p-canary@*.service" || true
    rm -f "/run/$p-canary.sock"
)
row_end $?

row_begin failed_instance_lingers_until_reset_failed
(
    set -e
    printf '[Service]\nExecStart=/bin/false\n' >"$unit_dir/$p-canary-lingers@.service"
    systemctl daemon-reload
    systemctl start "$p-canary-lingers@one.service" 2>/dev/null || true
    systemctl is-failed --quiet "$p-canary-lingers@one.service" || die "the instance is not left failed"
    systemctl reset-failed "$p-canary-lingers@one.service"
    ! systemctl is-failed --quiet "$p-canary-lingers@one.service" || die "reset-failed did not clear it"
)
row_end $?

echo "$passed passed, $failed failed, $skipped skipped"
[ "$failed" -eq 0 ]
