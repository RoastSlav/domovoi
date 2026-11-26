#!/bin/sh
# Tests the three-identities layout with real accounts, real directories and real systemd
# units: the installer, the root helper behind its socket and the self-test. Run it as root
# on a throwaway machine with systemd as PID 1 (a virtual machine, not a container: the helper unit needs a real systemd and mount namespaces). It
# creates and deletes accounts, owners, modes and units.
#
#   sudo env DOMOVOI_PRIVILEGED_TESTS=1 DOMOVOI_BIN=$PWD/target/debug/domovoi \
#     sh deploy/test/identities.sh
#
# It builds nothing. DOMOVOI_BIN is the binary to install; the repository's install.sh is
# the installer under test, served a fake release like deploy/test/installer.sh does. It
# creates only the names the installer creates (domovoi-agent, domovoi-skill, their groups
# and paths) after refusing to run when any of them exists, plus domovoi-probe-ci-*
# accounts for the negative cases. A trap deletes only what it recorded, and the script
# fails when a probe object or one of the real names is left over.
#
# It needs: python3, curl, tar, sha256sum, useradd, userdel, groupadd, groupdel, runuser,
# systemctl and journalctl.
set -eu

if [ "${DOMOVOI_PRIVILEGED_TESTS:-}" != "1" ]; then
    echo "identities.sh creates accounts and changes owners and modes. Run it as root on a" >&2
    echo "disposable machine with systemd as PID 1, with DOMOVOI_PRIVILEGED_TESTS=1." >&2
    exit 2
fi
if [ "$(id -u)" -ne 0 ]; then
    echo "run this as root, on a machine you can throw away" >&2
    exit 2
fi
bin_arg="${DOMOVOI_BIN:-}"
if [ -z "$bin_arg" ] || [ ! -x "$bin_arg" ]; then
    echo "set DOMOVOI_BIN to the domovoi binary to install" >&2
    exit 2
fi
bin_src="$(cd "$(dirname "$bin_arg")" && pwd)/$(basename "$bin_arg")"
for tool in python3 curl tar sha256sum useradd userdel groupadd groupdel gpasswd runuser systemctl journalctl; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is needed" >&2
        exit 2
    }
done
if [ ! -d /run/systemd/system ] || [ "$(ps -p 1 -o comm= | tr -d ' ')" != "systemd" ]; then
    echo "systemd is not PID 1 here; this test needs a machine that boots with systemd" >&2
    exit 2
fi

root="$(cd "$(dirname "$0")/../.." && pwd)"
installer="$root/install.sh"

# The names the installer creates. None of them may exist: the test deletes them again.
real_users="domovoi domovoi-agent domovoi-skill"
real_paths="/etc/domovoi /var/lib/domovoi /var/lib/domovoi-agent /var/lib/domovoi-packages
/var/lib/domovoi-helper /usr/local/bin/domovoi /usr/local/share/domovoi /run/domovoi-helper.sock
/etc/systemd/system/domovoi-hub.service /etc/systemd/system/domovoi-agent.service
/etc/systemd/system/domovoi-agent.service.d /etc/systemd/system/domovoi-helper.socket
/etc/systemd/system/domovoi-helper.socket.d /etc/systemd/system/domovoi-helper@.service
/etc/systemd/system/domovoi-helper@.service.d /run/domovoi-skill-runner.sock
/etc/systemd/system/domovoi-skill-runner.socket /etc/systemd/system/domovoi-skill-runner.socket.d
/etc/systemd/system/domovoi-skill-runner@.service /etc/systemd/system/domovoi-skill-runner@.service.d"
clash=0
for name in $real_users; do
    if id "$name" >/dev/null 2>&1 || getent group "$name" >/dev/null 2>&1; then
        echo "the account or group $name exists; run this on a clean machine" >&2
        clash=1
    fi
done
for path in $real_paths; do
    if [ -e "$path" ] || [ -L "$path" ]; then
        echo "$path exists; run this on a clean machine" >&2
        clash=1
    fi
done
if getent passwd | cut -d: -f1 | grep -q '^domovoi-probe-ci-'; then
    echo "a domovoi-probe-ci-* account exists; run this on a clean machine" >&2
    clash=1
fi
[ "$clash" -eq 0 ] || exit 2

work="$(mktemp -d)"
chmod 0755 "$work"
server=""
: >"$work/users"
: >"$work/groups"

# The lines of a record file as one space-separated word list.
listed() { tr '\n' ' ' <"$1"; }

# Deletes only what the lists above and the record files name.
wipe_host() {
    set +e
    systemctl stop domovoi-agent.service domovoi-helper.socket domovoi-skill-runner.socket >/dev/null 2>&1
    systemctl stop 'domovoi-helper@*.service' 'domovoi-skill-runner@*.service' >/dev/null 2>&1
    systemctl disable domovoi-helper.socket domovoi-skill-runner.socket domovoi-agent.service >/dev/null 2>&1
    for path in $real_paths; do
        case "$path" in
        /etc/systemd/system/* | /run/domovoi-helper.sock | /run/domovoi-skill-runner.sock) rm -rf "$path" ;;
        esac
    done
    systemctl daemon-reload >/dev/null 2>&1
    systemctl reset-failed 'domovoi-helper*' 'domovoi-skill-runner*' domovoi-agent.service >/dev/null 2>&1
    rm -rf /etc/domovoi /var/lib/domovoi-agent /var/lib/domovoi-packages /var/lib/domovoi-helper \
        /usr/local/bin/domovoi /usr/local/share/domovoi
    wh_users="$real_users $(listed "$work/users")"
    wh_groups="$wh_users $(listed "$work/groups")"
    for name in $wh_users; do
        if id "$name" >/dev/null 2>&1; then
            userdel "$name" >/dev/null 2>&1
        fi
    done
    for name in $wh_groups; do
        if getent group "$name" >/dev/null 2>&1; then
            groupdel "$name" >/dev/null 2>&1
        fi
    done
    set -e
}
cleanup() {
    [ -z "$server" ] || kill "$server" 2>/dev/null || true
    wipe_host
    rm -rf "$work"
}
trap cleanup EXIT
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
# Rows run in a subshell with errexit on: the first failing command fails the row.
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

facts() { stat -c '%a %U:%G' "$1"; }
mode_of() { stat -c '%a' "$1"; }
owner_of() { stat -c '%U:%G' "$1"; }
groups_of() { id -Gn "$1" | tr ' ' '\n' | sort | tr '\n' ' ' | sed 's/ $//'; }
expect_eq() { [ "$2" = "$3" ] || die "$1: expected '$3', found '$2'"; }
show_out() { sed 's/^/    | /' "$work/out" >&2; }
record_user() { printf '%s\n' "$1" >>"$work/users"; }
record_group() { printf '%s\n' "$1" >>"$work/groups"; }
nologin="$(command -v nologin || echo /usr/sbin/nologin)"

# Creates a probe account that is in no group but its own: the first argument is its name and
# any further arguments go to useradd.
probe_user() {
    pu_name="$1"
    shift
    record_user "$pu_name"
    useradd --system --user-group --no-create-home --shell "$nologin" "$@" "$pu_name"
}

# What the helper socket answers to one request from an account, as compact JSON, or
# "connect-error <errno>".
cat >"$work/client.py" <<'PY'
import json, socket, struct, sys, time, uuid
path = "/run/domovoi-helper.sock"
op = sys.argv[1]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
try:
    s.connect(path)
except OSError as e:
    print("connect-error", e.errno)
    sys.exit(3)
if op == "hold":
    time.sleep(float(sys.argv[2]))
    sys.exit(0)
request = {"op": op}
if len(sys.argv) > 2:
    request.update(json.loads(sys.argv[2]))
body = json.dumps({"v": 1, "id": str(uuid.uuid4()), "request": request}).encode()
try:
    s.sendall(struct.pack(">I", len(body)) + body)
except OSError:
    pass
try:
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
except OSError as e:
    print("read-error", e.errno)
    sys.exit(4)
print(json.dumps(json.loads(buf)["response"], sort_keys=True, separators=(",", ":")))
PY
# The optional third argument is a JSON object of the request's other fields.
helper_call() { runuser -u "$1" -- python3 "$work/client.py" "$2" ${3:+"$3"} 2>&1; }

case "$(uname -m)" in
x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
*)
    echo "unsupported machine $(uname -m)" >&2
    exit 2
    ;;
esac

# A release laid out like GitHub's, holding the binary under test.
tag="v9.9.0"
name="domovoi-$tag-$target"
mkdir -p "$work/srv/download/$tag" "$work/build/$name/examples"
cp "$bin_src" "$work/build/$name/domovoi"
chmod 0755 "$work/build/$name/domovoi"
echo "# example" >"$work/build/$name/examples/hub.toml"
tar -C "$work/build" -czf "$work/srv/download/$tag/$name.tar.gz" "$name"
(cd "$work/srv/download/$tag" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
port=18734
cat >"$work/server.py" <<'PY'
import http.server, os, sys
class H(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass
os.chdir(sys.argv[1])
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[2])), H).serve_forever()
PY
python3 "$work/server.py" "$work/srv" "$port" &
server=$!
i=0
until curl -fsS -o /dev/null 2>/dev/null "http://127.0.0.1:$port/download/$tag/$name.tar.gz"; do
    i=$((i + 1))
    [ "$i" -lt 50 ] || {
        echo "the test server did not start" >&2
        exit 1
    }
    sleep 0.1
done
export DOMOVOI_RELEASE_BASE="http://127.0.0.1:$port"

run_install() {
    sh "$installer" "$@" >"$work/out" 2>&1
}
installed_bin=/usr/local/bin/domovoi

#
# Part 1: an existing account is verified and never modified.
#
row_begin installer_never_modifies_an_existing_account
(
    set -e
    groupadd domovoi-probe-ci-extra
    record_group domovoi-probe-ci-extra
    record_user domovoi-skill
    useradd --system --user-group --no-create-home --shell "$nologin" -G domovoi-probe-ci-extra domovoi-skill
    before="$(getent passwd domovoi-skill) $(groups_of domovoi-skill)"
    if run_install --role agent --version "$tag"; then
        show_out
        die "the install should have failed"
    fi
    grep -q 'domovoi-skill' "$work/out" || {
        show_out
        die "the failure does not name domovoi-skill"
    }
    expect_eq "domovoi-skill is unchanged" "$(getent passwd domovoi-skill) $(groups_of domovoi-skill)" "$before"
    if id domovoi-agent >/dev/null 2>&1; then die "domovoi-agent was created before the check"; fi
    [ ! -e /var/lib/domovoi-agent ] || die "a directory was created before the check"
    userdel domovoi-skill
    groupdel domovoi-probe-ci-extra
    rm -rf /etc/domovoi /var/lib/domovoi-agent /usr/local/bin/domovoi /usr/local/share/domovoi
)
row_end $?
wipe_host

#
# Part 2: a fresh install, the helper in a real unit, the self-test.
#
row_begin fresh_install_creates_accounts_directories_and_units_with_the_table_modes
(
    set -e
    if run_install --role agent --version "$tag"; then :; else
        show_out
        die "the install failed"
    fi
    cp "$work/out" "$work/fresh.out"
    expected_agent_groups="domovoi-agent"
    if getent group systemd-journal >/dev/null 2>&1; then
        expected_agent_groups="domovoi-agent systemd-journal"
    fi
    expect_eq "domovoi-agent groups" "$(groups_of domovoi-agent)" "$expected_agent_groups"
    expect_eq "domovoi-skill groups" "$(groups_of domovoi-skill)" "domovoi-skill"
    case "$(getent passwd domovoi-agent | cut -d: -f7)" in */nologin | */false) ;; *) die "domovoi-agent has a login shell" ;; esac
    case "$(getent passwd domovoi-skill | cut -d: -f7)" in */nologin | */false) ;; *) die "domovoi-skill has a login shell" ;; esac
    expect_eq "/etc/domovoi" "$(facts /etc/domovoi)" "750 root:domovoi-agent"
    expect_eq "/etc/domovoi/skills" "$(facts /etc/domovoi/skills)" "755 root:root"
    expect_eq "/etc/domovoi/helper.toml" "$(facts /etc/domovoi/helper.toml)" "600 root:root"
    expect_eq "/var/lib/domovoi-agent" "$(facts /var/lib/domovoi-agent)" "700 domovoi-agent:domovoi-agent"
    expect_eq "/var/lib/domovoi-packages" "$(facts /var/lib/domovoi-packages)" "755 domovoi-agent:domovoi-agent"
    expect_eq "/var/lib/domovoi-helper" "$(facts /var/lib/domovoi-helper)" "700 root:root"
    expect_eq "the helper lock" "$(facts /var/lib/domovoi-helper/state.lock)" "600 root:root"
    expect_eq "the platform record" "$(facts /var/lib/domovoi-helper/platform.json)" "600 root:root"
    expect_eq "the helper socket node" "$(facts /run/domovoi-helper.sock)" "660 root:domovoi-agent"
    grep -q 'allow_write_groups_on_untested = false' /etc/domovoi/helper.toml || die "helper.toml is not the default"
    [ ! -e /etc/domovoi/agent.toml ] || die "the installer wrote agent.toml"
    cmp -s /etc/systemd/system/domovoi-helper.socket "$root/deploy/systemd/domovoi-helper.socket" || die "the helper socket unit differs"
    cmp -s "/etc/systemd/system/domovoi-helper@.service" "$root/deploy/systemd/domovoi-helper@.service" || die "the helper service unit differs"
    cmp -s /etc/systemd/system/domovoi-agent.service "$root/deploy/systemd/domovoi-agent.service" || die "the agent unit differs"
    systemctl is-active --quiet domovoi-helper.socket || die "the helper socket is not active"
    systemctl is-enabled --quiet domovoi-helper.socket || die "the helper socket is not enabled"
    if systemctl is-active --quiet domovoi-agent.service; then die "the installer started the agent"; fi
    grep -q 'Started domovoi-helper.socket' "$work/out" || die "the output does not say the socket started"
)
row_end $?

row_begin installer_creates_mask_and_bind_paths_before_enabling_sockets
(
    set -e
    created_line="$(grep -n 'Created /var/lib/domovoi-helper$' "$work/fresh.out" | head -n 1 | cut -d: -f1)"
    started_line="$(grep -n 'Started domovoi-helper.socket' "$work/fresh.out" | head -n 1 | cut -d: -f1)"
    [ -n "$created_line" ] || die "the output never says the helper directory was created"
    [ -n "$started_line" ] || die "the output never says the helper socket started"
    [ "$created_line" -lt "$started_line" ] || die "the helper directory was created after the socket started"
    born="$(stat -c %W /var/lib/domovoi-helper)"
    case "$born" in *[!0-9]* | "") born=0 ;; esac
    if [ "$born" -gt 0 ]; then
        entered="$(systemctl show -p ActiveEnterTimestamp --value domovoi-helper.socket)"
        [ -n "$entered" ] || die "the helper socket has no start time"
        [ "$born" -le "$(date -d "$entered" +%s)" ] || die "the helper directory is younger than the socket"
    fi
)
row_end $?

row_begin selftest_installer_passes_on_a_clean_host_and_reports_runner_checks_honestly
(
    set -e
    if "$installed_bin" agent selftest --installer >"$work/selftest.out" 2>&1; then :; else
        sed 's/^/    | /' "$work/selftest.out" >&2
        die "the self-test failed on a clean host"
    fi
    for check in ST-01 ST-02 ST-12 ST-15; do
        grep -q "^$check pass" "$work/selftest.out" || {
            sed 's/^/    | /' "$work/selftest.out" >&2
            die "$check did not pass"
        }
    done
    for check in ST-05 ST-06 ST-13; do
        grep -Eq "^$check (nd|pass)" "$work/selftest.out" || die "$check is neither nd nor pass"
    done
    grep -q '^boundary: ' "$work/selftest.out" || die "the report has no boundary line"
)
row_end $?

row_begin installer_reports_wrong_modes_on_a_migrated_host_and_changes_nothing
(
    set -e
    trap 'chmod 0700 /var/lib/domovoi-agent' EXIT
    before="$(for p in /etc/domovoi /etc/domovoi/skills /etc/domovoi/helper.toml /var/lib/domovoi-packages /var/lib/domovoi-helper; do facts "$p"; done)"
    chmod 0755 /var/lib/domovoi-agent
    if run_install --role agent --version "$tag"; then
        show_out
        die "the install should have stopped"
    fi
    grep -q '/var/lib/domovoi-agent' "$work/out" || die "the offending path is not reported"
    grep -q 'found mode 755' "$work/out" || die "the found mode is not reported"
    grep -q 'expected mode 700' "$work/out" || die "the expected mode is not reported"
    grep -q 'helper repair' "$work/out" || die "the repair command is not mentioned"
    expect_eq "the offending directory is untouched" "$(mode_of /var/lib/domovoi-agent)" "755"
    after="$(for p in /etc/domovoi /etc/domovoi/skills /etc/domovoi/helper.toml /var/lib/domovoi-packages /var/lib/domovoi-helper; do facts "$p"; done)"
    expect_eq "every other path is untouched" "$after" "$before"
)
row_end $?

row_begin helper_adopts_fd0_in_a_real_unit
(
    set -e
    answer="$(helper_call domovoi-agent health || true)"
    case "$answer" in *'"result":"health"'*) ;; *) die "no health answer: $answer" ;; esac
    if journalctl --no-pager -u 'domovoi-helper@*' 2>/dev/null | grep -q 'adopt_'; then die "the helper reported an adoption error"; fi
    if systemctl list-units --failed --plain --no-legend 'domovoi-helper@*' | grep -q .; then die "a helper instance failed"; fi
)
row_end $?

row_begin helper_start_check_runs_in_a_real_unit
(
    set -e
    trap 'chmod 0700 /var/lib/domovoi-helper' EXIT
    chmod 0755 /var/lib/domovoi-helper
    answer="$(helper_call domovoi-agent health || true)"
    case "$answer" in
    *'"result":"start_refused"'*'"token":"helper_state_permissions"'*) ;;
    *) die "the start check did not refuse: $answer" ;;
    esac
    expect_eq "the helper directory is still 0755" "$(mode_of /var/lib/domovoi-helper)" "755"
)
row_end $?

row_begin helper_restart_and_sigkill_keep_state_permissions
(
    set -e
    before="$(facts /var/lib/domovoi-helper) $(facts /var/lib/domovoi-helper/state.lock)"
    systemctl restart domovoi-helper.socket
    answer="$(helper_call domovoi-agent health || true)"
    case "$answer" in *'"result":"health"'*) ;; *) die "no health answer after a restart: $answer" ;; esac
    runuser -u domovoi-agent -- python3 "$work/client.py" hold 6 &
    holder=$!
    sleep 2
    instance="$(systemctl list-units --all --plain --no-legend 'domovoi-helper@*.service' | awk '{print $1}' | head -n 1)"
    [ -n "$instance" ] || die "no helper instance is running for the held connection"
    pid="$(systemctl show -p MainPID --value "$instance")"
    [ "${pid:-0}" -gt 0 ] || die "the instance has no main process"
    kill -KILL "$pid"
    wait "$holder" || true
    systemctl reset-failed 'domovoi-helper@*.service' 2>/dev/null || true
    expect_eq "modes after a restart and a SIGKILL" "$(facts /var/lib/domovoi-helper) $(facts /var/lib/domovoi-helper/state.lock)" "$before"
    answer="$(helper_call domovoi-agent health || true)"
    case "$answer" in *'"result":"health"'*) ;; *) die "no health answer after the SIGKILL: $answer" ;; esac
)
row_end $?

row_begin helper_unit_starts_under_the_shipped_directives
(
    set -e
    answer="$(helper_call domovoi-agent health || true)"
    case "$answer" in *'"result":"health"'*) ;; *) die "no health answer: $answer" ;; esac
    if journalctl --no-pager -u 'domovoi-helper@*' 2>/dev/null | grep -Eq '226/NAMESPACE|status=226'; then die "an instance failed to set up its namespace"; fi
    if systemctl list-units --failed --plain --no-legend 'domovoi-helper@*' | grep -q .; then die "a helper instance is left failed"; fi
    systemctl stop domovoi-helper.socket
    [ ! -e /run/domovoi-helper.sock ] || die "the socket node survived a stop (RemoveOnStop)"
    systemctl start domovoi-helper.socket
    expect_eq "the socket node after a restart" "$(facts /run/domovoi-helper.sock)" "660 root:domovoi-agent"
)
row_end $?

# The read operations (ADR 0036): the widened unit, SMART, the kernel log, listening sockets and
# Docker through the real socket. These rows change a sysctl value and start a listener under a
# probe account; both are put back by the row that changed them.
row_begin helper_unit_starts_under_the_widened_directives
(
    set -e
    answer="$(helper_call domovoi-agent health || true)"
    case "$answer" in *'"result":"health"'*) ;; *) die "no health answer: $answer" ;; esac
    # A connection that sends nothing keeps one instance alive, so its capabilities can be read.
    runuser -u domovoi-agent -- python3 "$work/client.py" hold 4 >/dev/null 2>&1 &
    hold_pid=$!
    sleep 1
    instance="$(systemctl list-units --plain --no-legend 'domovoi-helper@*.service' | awk 'NR==1 {print $1}')"
    [ -n "$instance" ] || die "no helper instance is running while a connection is open"
    main_pid="$(systemctl show -p MainPID --value "$instance")"
    cap_eff="$(awk '/^CapEff:/ {print $2}' "/proc/$main_pid/status")"
    want="$(python3 -c 'print("%016x" % sum(1 << n for n in (2, 12, 17, 19, 21, 34)))')"
    wait "$hold_pid" 2>/dev/null || true
    expect_eq "CapEff of a helper instance" "$cap_eff" "$want"
    if journalctl --no-pager -u 'domovoi-helper@*' 2>/dev/null | grep -Eq '226/NAMESPACE|status=226'; then die "an instance failed to set up its namespace"; fi
    if systemctl list-units --failed --plain --no-legend 'domovoi-helper@*' | grep -q .; then die "a helper instance is left failed"; fi
)
row_end $?

row_begin helper_smart_report_runs_in_the_shipped_unit
(
    set -e
    answer="$(helper_call domovoi-agent smart_scan || true)"
    case "$answer" in
    *'"result":"smart_scan"'*) ;;
    *'"token":"tool_missing"'*) printf '    note: smartctl is not installed here\n' >&2 ;;
    *) die "smart_scan was neither a scan nor tool_missing: $answer" ;;
    esac
    if systemctl list-units --failed --plain --no-legend 'domovoi-helper@*' | grep -q .; then die "a helper instance failed"; fi
    device="$(printf '%s' "$answer" | python3 -c 'import json,sys
try:
    d = json.load(sys.stdin).get("devices", [])
except ValueError:
    d = []
print(d[0] if d else "")')"
    [ -n "$device" ] || skip_row "no SMART device is visible here; run it on a host with a disk that answers smartctl"
    report="$(helper_call domovoi-agent smart_report "{\"device\":\"$device\"}" || true)"
    case "$report" in *'"result":"smart_report"'*) ;; *) die "smart_report for $device: $report" ;; esac
    printf '%s' "$report" | python3 -c 'import json,sys; json.loads(json.load(sys.stdin)["json"])' || die "the report is not smartctl JSON"
    # Record whether this device also answers without CAP_SYS_ADMIN (decision OD-1).
    dropin=/etc/systemd/system/domovoi-helper@.service.d
    trap 'rm -rf "$dropin"; systemctl daemon-reload' EXIT
    mkdir -p "$dropin"
    printf '[Service]\nCapabilityBoundingSet=\nCapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_RAWIO CAP_SYSLOG CAP_SYS_PTRACE CAP_DAC_READ_SEARCH\n' >"$dropin/zz-nosysadmin.conf"
    systemctl daemon-reload
    without="$(helper_call domovoi-agent smart_report "{\"device\":\"$device\"}" || true)"
    case "$without" in
    *'"result":"smart_report"'*) printf '    measured: %s answers smartctl without CAP_SYS_ADMIN\n' "$device" >&2 ;;
    *) printf '    measured: %s needs CAP_SYS_ADMIN (%s)\n' "$device" "$without" >&2 ;;
    esac
)
row_end $?

row_begin helper_kernel_log_reads_with_dmesg_restrict_on
(
    set -e
    command -v dmesg >/dev/null 2>&1 || skip_row "dmesg is not installed"
    old="$(sysctl -n kernel.dmesg_restrict 2>/dev/null)" || skip_row "kernel.dmesg_restrict is not readable here"
    trap 'sysctl -qw kernel.dmesg_restrict="$old"' EXIT
    sysctl -qw kernel.dmesg_restrict=1
    if runuser -u domovoi-agent -- dmesg >/dev/null 2>&1; then die "the agent account can read the ring buffer itself"; fi
    answer="$(helper_call domovoi-agent kernel_log '{"since_minutes":1440,"lines":50}' || true)"
    case "$answer" in *'"result":"kernel_log"'*) ;; *) die "no kernel log answer: $answer" ;; esac
)
row_end $?

row_begin helper_net_listeners_show_the_owning_process_of_another_user
(
    set -e
    probe_user domovoi-probe-ci-listen
    listener=""
    trap '[ -z "$listener" ] || kill "$listener" 2>/dev/null || true' EXIT
    runuser -u domovoi-probe-ci-listen -- python3 -c 'import socket,time
s = socket.socket()
s.bind(("127.0.0.1", 18791))
s.listen(1)
time.sleep(30)' >/dev/null 2>&1 &
    listener=$!
    sleep 1
    answer="$(helper_call domovoi-agent net_listeners || true)"
    case "$answer" in *'"result":"net_listeners"'*) ;; *) die "no listener answer: $answer" ;; esac
    printf '%s' "$answer" | python3 -c 'import json,sys
rows = json.load(sys.stdin)["listeners"]
sys.exit(0 if any(r["port"] == 18791 and r.get("process", "").startswith("python") for r in rows) else 1)' || die "the helper did not name the process listening on 18791: $answer"
)
row_end $?

row_begin helper_docker_read_works_through_the_real_socket_as_domovoi_agent
(
    set -e
    sock=/var/run/docker.sock
    [ -S "$sock" ] || skip_row "no Docker socket on this host; run it where Docker is installed"
    if runuser -u domovoi-agent -- python3 -c 'import socket,sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])' "$sock" 2>/dev/null; then die "the agent account can open the Docker socket (ST-16)"; fi
    answer="$(helper_call domovoi-agent docker_list || true)"
    case "$answer" in *'"result":"docker_containers"'*) ;; *) die "no container list: $answer" ;; esac
)
row_end $?

row_begin agent_with_boundary_in_effect_reads_docker_and_smart_through_the_helper
(
    set -e
    "$installed_bin" agent selftest --installer >"$work/selftest.out" 2>&1 || true
    grep -q '^ST-01 pass' "$work/selftest.out" || die "ST-01 did not pass"
    grep -q '^boundary: ' "$work/selftest.out" || die "the report has no boundary line"
    answer="$(helper_call domovoi-agent smart_scan || true)"
    case "$answer" in *'"result":"smart_scan"'* | *'"token":"tool_missing"'*) ;; *) die "no typed smart answer: $answer" ;; esac
    answer="$(helper_call domovoi-agent docker_probe || true)"
    case "$answer" in *'"result":"docker_engine"'* | *'"token":"engine_absent"'*) ;; *) die "no typed docker answer: $answer" ;; esac
)
row_end $?

row_begin selftest_still_passes_on_a_clean_host_with_the_widened_unit
(
    set -e
    "$installed_bin" agent selftest --installer >"$work/selftest.out" 2>&1 || true
    for check in ST-01 ST-02 ST-07 ST-12 ST-15; do
        grep -q "^$check pass" "$work/selftest.out" || {
            sed 's/^/    | /' "$work/selftest.out" >&2
            die "$check did not pass with the widened unit"
        }
    done
    for check in ST-10 ST-11 ST-16; do
        grep -Eq "^$check (nd|pass)" "$work/selftest.out" || die "$check is neither nd nor pass"
    done
)
row_end $?

row_begin real_user_skill_cannot_connect_to_helper_socket
(
    set -e
    trap 'userdel domovoi-probe-ci-user 2>/dev/null || true; groupdel domovoi-probe-ci-user 2>/dev/null || true' EXIT
    probe_user domovoi-probe-ci-user
    accepted="$(systemctl show -p NAccepted --value domovoi-helper.socket)"
    answer="$(helper_call domovoi-probe-ci-user health || true)"
    case "$answer" in "connect-error 13"*) ;; *) die "the probe account was not refused with EACCES: $answer" ;; esac
    expect_eq "connections accepted" "$(systemctl show -p NAccepted --value domovoi-helper.socket)" "$accepted"
)
row_end $?

row_begin agent_cannot_read_helper_state_directory_or_proc
(
    set -e
    trap 'rm -f /var/lib/domovoi-helper/attest.key' EXIT
    (umask 077 && printf 'fixture key\n' >/var/lib/domovoi-helper/attest.key)
    chown root:root /var/lib/domovoi-helper/attest.key
    chmod 0600 /var/lib/domovoi-helper/attest.key
    for target in /var/lib/domovoi-helper/attest.key /var/lib/domovoi-helper /var/lib/domovoi-helper/state.lock /proc/1/environ; do
        if [ -d "$target" ]; then read_cmd="ls"; else read_cmd="cat"; fi
        message="$(runuser -u domovoi-agent -- "$read_cmd" "$target" 2>&1 >/dev/null || true)"
        case "$message" in
        *"Permission denied"*) ;;
        *) die "the agent was not denied $target: $message" ;;
        esac
    done
)
row_end $?

row_begin skill_given_agent_group_is_stopped_by_helper_uid_check
(
    set -e
    trap 'userdel domovoi-probe-ci-agentgrp 2>/dev/null || true; groupdel domovoi-probe-ci-agentgrp 2>/dev/null || true' EXIT
    record_user domovoi-probe-ci-agentgrp
    useradd --system --gid domovoi-agent --no-create-home --shell "$nologin" domovoi-probe-ci-agentgrp
    answer="$(helper_call domovoi-probe-ci-agentgrp health || true)"
    case "$answer" in *'"token":"peer_not_agent"'*) ;; *) die "the group member was not refused by the uid check: $answer" ;; esac
)
row_end $?

row_begin docker_group_member_fails_selftest
(
    set -e
    getent group docker >/dev/null 2>&1 || skip_row "this machine has no docker group; install Docker to run this row"
    trap 'gpasswd -d domovoi-agent docker >/dev/null 2>&1; true' EXIT
    gpasswd -a domovoi-agent docker >/dev/null
    if "$installed_bin" agent selftest --installer >"$work/selftest.out" 2>&1; then
        die "the self-test passed with domovoi-agent in the docker group"
    fi
    grep -q '^ST-04 fail docker_group_member' "$work/selftest.out" || {
        sed 's/^/    | /' "$work/selftest.out" >&2
        die "ST-04 did not fail with docker_group_member"
    }
)
row_end $?

row_begin selftest_fails_on_world_connectable_socket
(
    set -e
    trap 'systemctl restart domovoi-helper.socket' EXIT
    chmod 0666 /run/domovoi-helper.sock
    if "$installed_bin" agent selftest --installer >"$work/selftest.out" 2>&1; then
        die "the self-test passed with a world-connectable socket"
    fi
    grep -q '^ST-12 fail socket_mode_wrong' "$work/selftest.out" || {
        sed 's/^/    | /' "$work/selftest.out" >&2
        die "ST-12 did not fail with socket_mode_wrong"
    }
)
row_end $?

row_begin selftest_detects_a_dropin_on_the_helper_unit
(
    set -e
    trap 'rm -rf "/etc/systemd/system/domovoi-helper@.service.d"; systemctl daemon-reload' EXIT
    mkdir -p "/etc/systemd/system/domovoi-helper@.service.d"
    printf '[Service]\nEnvironment=DOMOVOI_TEST=1\n' >"/etc/systemd/system/domovoi-helper@.service.d/zz-test.conf"
    systemctl daemon-reload
    if "$installed_bin" agent selftest --installer >"$work/selftest.out" 2>&1; then
        die "the self-test passed with a drop-in on the helper unit"
    fi
    grep -q '^ST-07 fail unit_modified' "$work/selftest.out" || {
        sed 's/^/    | /' "$work/selftest.out" >&2
        die "ST-07 did not fail with unit_modified"
    }
)
row_end $?

row_begin privileged_rust_tests
(
    set -e
    [ -f "$root/crates/domovoi-helper/tests/privileged.rs" ] || skip_row "crates/domovoi-helper/tests/privileged.rs does not exist"
    command -v cargo >/dev/null 2>&1 || skip_row "cargo is not on root's PATH; run with sudo env PATH=\$PATH"
    cd "$root"
    DOMOVOI_BIN="$bin_src" cargo test -p domovoi-helper --test privileged -- --ignored
)
row_end $?

wipe_host

#
# Part 3: options of the removed identity migration are rejected.
#
row_begin migrate_identity_is_an_unknown_option
(
    set -e
    if run_install --role agent --version "$tag" --migrate-identity; then die "--migrate-identity was accepted"; fi
    grep -q 'unknown option: --migrate-identity' "$work/out" || die "the option was not reported as unknown"
    if run_install --rollback-identity; then die "--rollback-identity was accepted"; fi
    grep -q 'unknown option: --rollback-identity' "$work/out" || die "the option was not reported as unknown"
)
row_end $?

#
# Clean up and check that nothing is left.
#
wipe_host
leftovers=""
all_users="$real_users $(listed "$work/users")"
all_groups="$all_users $(listed "$work/groups")"
for name in $all_users; do
    if id "$name" >/dev/null 2>&1; then leftovers="$leftovers account:$name"; fi
done
for name in $all_groups; do
    if getent group "$name" >/dev/null 2>&1; then leftovers="$leftovers group:$name"; fi
done
probe_accounts="$(getent passwd | cut -d: -f1 | grep '^domovoi-probe-ci-' || true)"
for probe in $probe_accounts; do
    leftovers="$leftovers account:$probe"
done
probe_groups="$(getent group | cut -d: -f1 | grep '^domovoi-probe-ci-' || true)"
for probe in $probe_groups; do
    leftovers="$leftovers group:$probe"
done
for path in $real_paths; do
    if [ -e "$path" ] || [ -L "$path" ]; then leftovers="$leftovers path:$path"; fi
done
if [ -n "$leftovers" ]; then
    printf 'FAIL nothing_is_left_behind (still there:%s)\n' "$leftovers"
    failed=$((failed + 1))
else
    pass nothing_is_left_behind
    passed=$((passed + 1))
fi

echo "$passed passed, $failed failed, $skipped skipped"
if [ "$failed" -gt 0 ]; then
    exit 1
fi
echo "all identity checks passed"
