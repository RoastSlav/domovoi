#!/bin/sh
# Tests install.sh against a fake release served from a local web server. Run it as root
# in a throwaway container or VM: it creates the domovoi user, /etc/domovoi,
# /var/lib/domovoi* and the systemd units. Stubs stand in for systemd, and a stub binary
# records its calls, so it also covers the hub start, enrolling an agent, --role both (the
# order of start, stop, token, start, enroll, enable) and the pre-0.1.0 database rule.
#
#   docker run --rm -v "$PWD:/src" -w /src debian:12 \
#     sh -c 'apt-get update -qq && apt-get install -y -qq curl python3 >/dev/null && deploy/test/installer.sh'
set -eu

root="$(cd "$(dirname "$0")/../.." && pwd)"
installer="$root/install.sh"
work="$(mktemp -d)"
chmod 0755 "$work"
server=""
cleanup() {
    [ -z "$server" ] || kill "$server" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 1' INT TERM

failures=0
pass() { printf 'ok   %s\n' "$*"; }
fail() {
    printf 'FAIL %s\n' "$*"
    failures=$((failures + 1))
}
check() {
    desc="$1"
    shift
    if "$@"; then pass "$desc"; else fail "$desc"; fi
}
mode_of() { stat -c '%a' "$1"; }
owner_of() { stat -c '%U:%G' "$1"; }

[ "$(id -u)" -eq 0 ] || {
    echo "run this as root, in a container or VM you can throw away" >&2
    exit 2
}

for path in /usr/local/bin/domovoi /etc/domovoi /var/lib/domovoi /var/lib/domovoi-agent \
    /var/lib/domovoi-packages /var/lib/domovoi-helper; do
    [ ! -e "$path" ] || {
        echo "$path exists; run this on a clean machine" >&2
        exit 2
    }
done
for account in domovoi-agent domovoi-skill; do
    ! id "$account" >/dev/null 2>&1 || {
        echo "the $account account exists; run this on a clean machine" >&2
        exit 2
    }
done

# The units embedded in install.sh are the ones in deploy/systemd/.
for pair in hub:domovoi-hub.service agent:domovoi-agent.service \
    helper-socket:domovoi-helper.socket helper-service:domovoi-helper@.service \
    runner-socket:domovoi-skill-runner.socket runner-service:domovoi-skill-runner@.service; do
    unit="${pair%%:*}"
    unit_file="${pair#*:}"
    check "embedded $unit unit matches deploy/systemd" \
        sh -c "sh '$installer' --print-unit $unit | cmp -s - '$root/deploy/systemd/$unit_file'"
done
check "the agent unit runs as domovoi-agent" \
    sh -c "sh '$installer' --print-unit agent | grep -qx 'User=domovoi-agent'"
check "the root agent unit keeps running as root" \
    sh -c "! sh '$installer' --print-unit agent-root | grep -Eq '^(User|Group)='"
check "an unknown unit name is refused" \
    sh -c "! sh '$installer' --print-unit runner >/dev/null 2>&1"

# The runner steps need systemd, so they are tested through the functions of install.sh, taken
# out of the script as written, with stubs for the binary and for systemctl.
fns="$work/install-functions.sh"
grep '^\(say\|say_always\|log_line\|show_file\|show_file_err\|fail\)() {' "$installer" >"$fns" # one-line functions
for fn in err fail_with failed_checks runner_service_unit render_runner_service runner_prefix_ok \
    runner_dirs place_unit restart_replaced_socket start_sockets selftest_hint manifest_add mkdir_rec \
    rollback_disarm check_helper_toml warn; do
    sed -n "/^$fn() {/,/^}/p" "$installer" >>"$fns"
done
check "the runner functions were extracted" test "$(grep -c '^[a-z_]*() {' "$fns")" -eq 22

rendered="$work/rendered.service"
render_with() {
    sh -c ". '$fns'; runner_skills_dir='$1'; runner_packages_dir='$2'; render_runner_service" >"$rendered"
}
render_with /etc/domovoi/skills /var/lib/domovoi-packages
check "the default directories render to the shipped runner unit" \
    cmp -s "$rendered" "$root/deploy/systemd/domovoi-skill-runner@.service"
render_with /srv/skills /srv/packages
check "a custom skills_dir and packages_dir reach the ExecStart arguments" \
    grep -qx 'ExecStart=/usr/local/bin/domovoi skill-runner serve --skills-dir /srv/skills --packages-dir /srv/packages' "$rendered"
check "a custom skills_dir and packages_dir reach the bind" \
    grep -qx 'BindReadOnlyPaths=/srv/skills /srv/packages' "$rendered"
check "no default directory is left in a custom rendering" \
    sh -c "! grep -E '/etc/domovoi/skills|domovoi-packages' '$rendered'"
check "the mask still names its three parents" \
    grep -qx 'TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro' "$rendered"

for bad in /home/me /root /root/x /run/prefix /tmp/p /dev/shm/p; do
    check "a prefix of $bad is refused for the runner" \
        sh -c ". '$fns'; prefix='$bad'; ! (runner_prefix_ok) >/dev/null 2>&1"
done
for good in /usr/local /opt/domovoi /usr; do
    check "a prefix of $good is allowed for the runner" \
        sh -c ". '$fns'; prefix='$good'; runner_prefix_ok"
done

# Stubs: a binary that answers `agent dirs` and `agent selftest`, and a systemctl that logs.
stub="$work/stub"
mkdir -p "$stub"
cat >"$stub/domovoi" <<'STUB'
#!/bin/sh
case "$*" in
"agent dirs")
    [ "${STUB_DIRS:-ok}" = ok ] || { echo "agent.toml: a path is not usable" >&2; exit 1; }
    echo "skills_dir=/srv/skills"
    echo "packages_dir=/srv/packages"
    ;;
"agent selftest --installer --record")
    [ "${STUB_FAIL:-none}" != first ] || { echo "ST-05 fail runner_groups_extra"; exit 1; }
    echo "ST-01 pass"
    ;;
"check config --kind helper "*)
    [ "${STUB_HELPER_TOML:-ok}" = ok ] || { echo "error: groups[0]: unknown helper group"; exit 1; }
    echo "helper.toml: ok (0 warning(s))"
    ;;
"agent selftest --installer")
    [ "${STUB_FAIL:-none}" != second ] || { echo "ST-13 fail runner_probe"; exit 1; }
    echo "ST-13 pass"
    ;;
esac
STUB
chmod 0755 "$stub/domovoi"
sockets_run() {
    : >"$work/systemctl.log"
    sh -c ". '$fns'; systemctl() { echo \"\$*\" >>'$work/systemctl.log'; }; bin='$stub/domovoi'; tmp='$work'; rollback_armed=0; $1" \
        >"$work/sockets.out" 2>&1
}
check "agent dirs sets the two runner directories and adds them to the created-set" \
    sh -c ". '$fns'; bin='$stub/domovoi'; CREATED_PATHS='/etc/domovoi'; runner_dirs; test \"\$runner_skills_dir \$runner_packages_dir\" = '/srv/skills /srv/packages' && test \"\$CREATED_PATHS\" = '/etc/domovoi /srv/skills /srv/packages'"
if STUB_DIRS=refuse sh -c ". '$fns'; bin='$stub/domovoi'; CREATED_PATHS=x; runner_dirs" >"$work/dirs.out" 2>&1; then
    fail "a refused agent dirs stops the runner steps"
else
    pass "a refused agent dirs stops the runner steps"
fi
check "a refused agent dirs says nothing was changed" grep -q "nothing was changed" "$work/dirs.out"

sockets_run "check_helper_toml '$work/helper.toml'"
check "a valid helper.toml is said nothing about" \
    sh -c "! grep -q 'did not pass' '$work/sockets.out'"
STUB_HELPER_TOML=bad
export STUB_HELPER_TOML
sockets_run "check_helper_toml '$work/helper.toml'" || fail "a bad helper.toml stops the install"
check "a bad helper.toml is shown with the command that checks it" \
    sh -c "grep -q 'unknown helper group' '$work/sockets.out' && grep -q 'check config $work/helper.toml' '$work/sockets.out'"
STUB_HELPER_TOML=ok

sockets_run start_sockets
check "both sockets start when the self-test passes" \
    sh -c "grep -qx 'enable --now domovoi-helper.socket' '$work/systemctl.log' && grep -qx 'enable --now domovoi-skill-runner.socket' '$work/systemctl.log' && ! grep -q disable '$work/systemctl.log'"
STUB_FAIL=first
export STUB_FAIL
if sockets_run start_sockets; then
    fail "a failed self-test stops the install"
else
    pass "a failed self-test stops the install"
fi
STUB_FAIL=none
check "a failed self-test never starts the runner socket" \
    sh -c "! grep -q 'enable --now domovoi-skill-runner.socket' '$work/systemctl.log'"
check "a failed self-test is named by its check" grep -q "ST-05" "$work/sockets.out"
STUB_FAIL=second
if sockets_run start_sockets; then
    fail "a failed check with the runner socket up stops the install"
else
    pass "a failed check with the runner socket up stops the install"
fi
STUB_FAIL=none
check "a failed check with the runner socket up stops the socket again" \
    grep -qx 'disable --now domovoi-skill-runner.socket' "$work/systemctl.log"
check "that failure names the check" grep -q "ST-13" "$work/sockets.out"
check "that failure says the agent was not enrolled and what to run" \
    grep -q "The agent was not enrolled or started" "$work/sockets.out"

# A runner unit that differs from the release is replaced; the old text is saved and the change is printed.
mkdir -p /etc/systemd/system
printf '[Service]\nProcSubset=pid\n' >/etc/systemd/system/domovoi-skill-runner@.service
sh -c ". '$fns'; runner_skills_dir=/etc/domovoi/skills; runner_packages_dir=/var/lib/domovoi-packages; bin=/usr/local/bin/domovoi; prefix='$work/prefix'; tmp='$work'; rollback_armed=0; keep_units=0; place_unit domovoi-skill-runner@.service render_runner_service" \
    >"$work/place.out" 2>&1 || true
check "a stale runner unit is replaced" \
    cmp -s /etc/systemd/system/domovoi-skill-runner@.service "$root/deploy/systemd/domovoi-skill-runner@.service"
check "the replacement is reported" grep -q "Replaced /etc/systemd/system/domovoi-skill-runner@.service" "$work/place.out"
check "the report names the changed directive" grep -q "ProcSubset=pid" "$work/place.out"
check "the old text is saved" sh -c "grep -l 'ProcSubset=pid' '$work'/prefix/share/domovoi/unit-backups/domovoi-skill-runner@.service.* >/dev/null"
# --keep-units keeps it and says so.
printf '[Service]\nProcSubset=pid\n' >/etc/systemd/system/domovoi-skill-runner@.service
sh -c ". '$fns'; runner_skills_dir=/etc/domovoi/skills; runner_packages_dir=/var/lib/domovoi-packages; bin=/usr/local/bin/domovoi; prefix='$work/prefix'; tmp='$work'; rollback_armed=0; keep_units=1; place_unit domovoi-skill-runner@.service render_runner_service" \
    >"$work/place.out" 2>&1 || true
check "--keep-units keeps the unit" grep -q "ProcSubset=pid" /etc/systemd/system/domovoi-skill-runner@.service
check "--keep-units says so" grep -q "Kept /etc/systemd/system/domovoi-skill-runner@.service (--keep-units)" "$work/place.out"
check "--keep-units saves the new text beside" \
    cmp -s "$work/prefix/share/domovoi/domovoi-skill-runner@.service" "$root/deploy/systemd/domovoi-skill-runner@.service"
rm -f /etc/systemd/system/domovoi-skill-runner@.service

# A replaced socket unit is restarted, since enable --now keeps a listening socket as it is.
: >"$work/systemctl.log"
printf '[Socket]\n' >/etc/systemd/system/domovoi-skill-runner.socket
sh -c ". '$fns'; systemctl() { echo \"\$*\" >>'$work/systemctl.log'; }; runner_skills_dir=/etc/domovoi/skills; runner_packages_dir=/var/lib/domovoi-packages; bin=/usr/local/bin/domovoi; prefix='$work/prefix'; tmp='$work'; rollback_armed=0; keep_units=0; runner_socket_unit() { sh '$installer' --print-unit runner-socket; }; place_unit domovoi-skill-runner.socket runner_socket_unit; restart_replaced_socket domovoi-skill-runner.socket; restart_replaced_socket domovoi-helper.socket" \
    >"$work/place.out" 2>&1 || true
check "a replaced socket unit is restarted" \
    grep -qx 'restart domovoi-skill-runner.socket' "$work/systemctl.log"
check "an unchanged socket is not restarted" \
    sh -c "! grep -q 'restart domovoi-helper.socket' '$work/systemctl.log'"
rm -f /etc/systemd/system/domovoi-skill-runner.socket

# An upgrade replaces a starter skill that nobody edited and keeps an edited one.
skills_fns="$work/skill-functions.sh"
grep '^\(say\|say_always\|log_line\)() {' "$installer" >"$skills_fns"
for fn in add_starter_skills normalise_tree manifest_add; do
    sed -n "/^$fn() {/,/^}/p" "$installer" >>"$skills_fns"
done
mkdir -p "$work/new-skills/zz_same" "$work/new-skills/zz_edited" "$work/up/old-skills/zz_same" \
    "$work/up/old-skills/zz_edited" /etc/domovoi/skills/zz_same /etc/domovoi/skills/zz_edited
for name in zz_same zz_edited; do
    echo new >"$work/new-skills/$name/run"
    echo old >"$work/up/old-skills/$name/run"
done
echo old >/etc/domovoi/skills/zz_same/run
echo mine >/etc/domovoi/skills/zz_edited/run
sh -c ". '$skills_fns'; tmp='$work/up'; rollback_armed=0; add_starter_skills '$work/new-skills' normalise" \
    >"$work/skills.out" 2>&1 || true
check "an unedited starter skill is updated" grep -qx new /etc/domovoi/skills/zz_same/run
check "an edited starter skill is kept" grep -qx mine /etc/domovoi/skills/zz_edited/run
check "the kept skill is reported" grep -q "Kept /etc/domovoi/skills/zz_edited" "$work/skills.out"
rm -rf /etc/domovoi

case "$(uname -m)" in
x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
*)
    echo "unsupported machine $(uname -m)" >&2
    exit 2
    ;;
esac

# A release laid out like GitHub's: download/<tag>/<asset>, and latest -> tag/<tag>.
release() {
    tag="$1"
    skills="$2"
    name="domovoi-$tag-$target"
    dir="$work/srv/download/$tag"
    mkdir -p "$dir" "$work/build/$name/examples"
    printf '#!/bin/sh\necho "domovoi %s (test build)"\n' "$tag" >"$work/build/$name/domovoi"
    chmod 0755 "$work/build/$name/domovoi"
    echo "# example" >"$work/build/$name/examples/hub.toml"
    for skill in $skills; do
        mkdir -p "$work/build/$name/examples/skills/$skill"
        printf 'name = "%s"\n' "$skill" >"$work/build/$name/examples/skills/$skill/skill.toml"
    done
    tar -C "$work/build" -czf "$dir/$name.tar.gz" "$name"
    (cd "$dir" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
}
release v9.9.0 "alpha beta"
release v9.9.1 "alpha beta gamma"

port=18733
cat >"$work/server.py" <<'PY'
import http.server, os, sys
class H(http.server.SimpleHTTPRequestHandler):
    def do_HEAD(self):
        if self.path.rstrip("/") == "/latest":
            self.send_response(302)
            self.send_header("Location", "/tag/" + os.environ["LATEST"])
            self.end_headers()
            return
        if self.path.startswith("/tag/"):
            self.send_response(200)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        super().do_HEAD()
    def do_GET(self):
        if self.path.rstrip("/") == "/latest" or self.path.startswith("/tag/"):
            return self.do_HEAD()
        super().do_GET()
    def log_message(self, *args):
        pass
os.chdir(sys.argv[1])
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[2])), H).serve_forever()
PY
LATEST=v9.9.1 python3 "$work/server.py" "$work/srv" "$port" &
server=$!
base="http://127.0.0.1:$port"
i=0
until curl -fsS -o /dev/null 2>/dev/null "$base/download/v9.9.1/domovoi-v9.9.1-$target.tar.gz"; do
    i=$((i + 1))
    [ "$i" -lt 50 ] || {
        echo "the test server did not start" >&2
        exit 1
    }
    sleep 0.1
done
export DOMOVOI_RELEASE_BASE="$base"

run_install() {
    sh "$installer" "$@" >"$work/out" 2>&1
}


# The usage text, the input checks and --dry-run need no state, so they run before the first
# install: a dry run must leave a clean machine clean.
exit_of() { sh "$installer" "$@" >"$work/out" 2>&1 && echo 0 || echo $?; }
check "usage lists the exit codes" sh -c "sh '$installer' --help | grep -q 'Exit codes:' && sh '$installer' --help | grep -Eq '^  50  '"
check "a bad --version exits 2" test "$(exit_of --role hub --version 'v1;x')" = 2
check "a relative --prefix exits 2" test "$(exit_of --role hub --prefix rel/dir)" = 2
check "a plain-http base outside loopback exits 2" \
    test "$(DOMOVOI_RELEASE_BASE=http://example.invalid/r exit_of --role hub --version v9.9.0 --dry-run)" = 2
check "DOMOVOI_ALLOW_HTTP=1 allows a plain-http base" \
    test "$(DOMOVOI_ALLOW_HTTP=1 DOMOVOI_RELEASE_BASE=http://example.invalid/r exit_of --role hub --version v9.9.0 --dry-run)" = 0
check "hub dry run exits 0" test "$(exit_of --role hub --version v9.9.0 --dry-run)" = 0
check "hub dry run prints the plan" grep -q "create   system user domovoi" "$work/out"
check "agent dry run exits 0" test "$(exit_of --role agent --dry-run)" = 0
check "dry run created no binary" test ! -e /usr/local/bin/domovoi
check "dry run created no user" sh -c "! id domovoi >/dev/null 2>&1"
check "dry run created no directory" sh -c "test ! -e /etc/domovoi && test ! -e /var/lib/domovoi && test ! -e /var/lib/domovoi-agent"
check "dry run installed no unit" sh -c "test ! -e /etc/systemd/system/domovoi-hub.service && test ! -e /etc/systemd/system/domovoi-agent.service"
check_json() {
    python3 -I -c '
import json, sys
objs = [json.loads(l) for l in open(sys.argv[1]).read().splitlines()]
assert objs and [o["event"] for o in objs].count("result") == 1 and objs[-1]["event"] == "result"
assert all(o["event"] == "step" for o in objs[:-1])
assert objs[-1]["ok"] is True and objs[-1]["exit_code"] == 0
' "$1"
}
sh "$installer" --role hub --version v9.9.0 --dry-run --json >"$work/json.out" 2>/dev/null
check "json dry run is only JSON lines ending in one result" check_json "$work/json.out"

# A hub, at an explicit version.
check "hub install succeeds" run_install --role hub --version v9.9.0
check "binary installed" test -x /usr/local/bin/domovoi
check "binary mode 0755" test "$(mode_of /usr/local/bin/domovoi)" = 755
check "binary is v9.9.0" sh -c '/usr/local/bin/domovoi version | grep -q v9.9.0'
check "examples installed" test -f /usr/local/share/domovoi/examples/hub.toml
check "domovoi user exists" sh -c "id domovoi >/dev/null"
check "/var/lib/domovoi is 0750 domovoi" \
    test "$(mode_of /var/lib/domovoi) $(owner_of /var/lib/domovoi)" = "750 domovoi:domovoi"
check "/etc/domovoi is 0750 root:domovoi" \
    test "$(mode_of /etc/domovoi) $(owner_of /etc/domovoi)" = "750 root:domovoi"
check "no hub.toml written" test ! -e /etc/domovoi/hub.toml
check "hub unit installed" cmp -s /etc/systemd/system/domovoi-hub.service "$root/deploy/systemd/domovoi-hub.service"
check "hub install prints no privilege boundary notice" test -z "$(grep 'privilege boundary' "$work/out")"
check "hub next steps printed" grep -q "systemctl enable --now domovoi-hub" "$work/out"
check "setup hint printed" grep -q "journalctl -u domovoi-hub | grep setup" "$work/out"
check "attestation hint printed" grep -q "gh attestation verify" "$work/out"

# An agent on the same machine, at the latest version.
check "agent install succeeds" run_install --role agent
check "latest resolved to v9.9.1" sh -c '/usr/local/bin/domovoi version | grep -q v9.9.1'
check "/var/lib/domovoi-agent is 0700" test "$(mode_of /var/lib/domovoi-agent)" = 700
check "/etc/domovoi kept its group" test "$(owner_of /etc/domovoi)" = "root:domovoi"
check "empty skills directory created" test -d /etc/domovoi/skills
check "starter skills copied on first install" test -f /etc/domovoi/skills/alpha/skill.toml
check "starter skill is not writable by others" sh -c "! find /etc/domovoi/skills/alpha -perm -022 | grep -q ."
check "no agent.toml written" test ! -e /etc/domovoi/agent.toml
check "agent unit installed (the root unit: there is no systemd here)" \
    sh -c "sh '$installer' --print-unit agent-root | cmp -s - /etc/systemd/system/domovoi-agent.service"
check "agent next steps printed" grep -q "Add host" "$work/out"
check "agent next steps list the journal init after the enroll and before the start" \
    sh -c "a=\$(grep -n 'Add host' '$work/out' | head -n 1 | cut -d: -f1); b=\$(grep -n 'agent journal init' '$work/out' | head -n 1 | cut -d: -f1); c=\$(grep -n 'sudo systemctl enable --now domovoi-agent' '$work/out' | head -n 1 | cut -d: -f1); test -n \"\$a\" && test -n \"\$b\" && test -n \"\$c\" && test \"\$a\" -lt \"\$b\" && test \"\$b\" -lt \"\$c\""
check "privilege boundary notice printed without systemd" \
    grep -q "privilege boundary not in effect: no systemd on this host" "$work/out"
check "no domovoi-agent account created" sh -c "! id domovoi-agent >/dev/null 2>&1"
check "no domovoi-skill account created" sh -c "! id domovoi-skill >/dev/null 2>&1"
check "no packages directory created" test ! -e /var/lib/domovoi-packages
check "no helper directory created" test ! -e /var/lib/domovoi-helper
check "no helper.toml written" test ! -e /etc/domovoi/helper.toml
check "no helper socket unit installed" test ! -e /etc/systemd/system/domovoi-helper.socket
check "no helper service unit installed" test ! -e /etc/systemd/system/domovoi-helper@.service
check "skills directory keeps today's 0750 root:root" \
    test "$(mode_of /etc/domovoi/skills) $(owner_of /etc/domovoi/skills)" = "750 root:root"
check "state directory keeps today's 0700 root:root" \
    test "$(mode_of /var/lib/domovoi-agent) $(owner_of /var/lib/domovoi-agent)" = "700 root:root"
check "skill added by the latest release copied" test -f /etc/domovoi/skills/gamma/skill.toml
echo "# edited" >>/etc/domovoi/skills/alpha/skill.toml
rm -rf /etc/domovoi/skills/gamma
check "agent upgrade succeeds" run_install --role agent
check "edited starter skill kept" grep -q "# edited" /etc/domovoi/skills/alpha/skill.toml
check "agent upgrade still creates no accounts" sh -c "! id domovoi-agent >/dev/null 2>&1 && ! id domovoi-skill >/dev/null 2>&1"
check "agent upgrade prints the notice again" grep -q "privilege boundary not in effect" "$work/out"
check "missing starter skill copied again on upgrade" test -f /etc/domovoi/skills/gamma/skill.toml

# Running again changes the binary; a unit that differs is replaced and the old one is saved.
echo "# edited" >>/etc/systemd/system/domovoi-hub.service
echo '[hub]' >/etc/domovoi/hub.toml
chmod 0700 /var/lib/domovoi
edited="$(cat /etc/systemd/system/domovoi-hub.service)"
check "second hub install succeeds" run_install --role hub
check "edited unit replaced" test "$(cat /etc/systemd/system/domovoi-hub.service)" != "$edited"
check "replaced unit reported" grep -q "Replaced /etc/systemd/system/domovoi-hub.service" "$work/out"
check "old unit saved" sh -c "ls /usr/local/share/domovoi/unit-backups/domovoi-hub.service.* >/dev/null"
check "hub.toml kept" test "$(cat /etc/domovoi/hub.toml)" = "[hub]"
check "upgrade suggests restart" grep -q "systemctl restart domovoi-hub" "$work/out"
check "upgrade does not suggest enable --now" test -z "$(grep "enable --now" "$work/out")"
check "changed mode kept" test "$(mode_of /var/lib/domovoi)" = 700
check "unchanged unit reported up to date" sh -c "sh '$installer' --role agent 2>&1 | grep -q 'domovoi-agent.service is up to date'"


# --json on a real run, over an install that already exists.
sh "$installer" --role hub --json >"$work/json.out" 2>/dev/null
check "json install is only JSON lines ending in one result" check_json "$work/json.out"
check "json result names the binary" sh -c "grep -q '\"binary\":\"/usr/local/bin/domovoi\"' '$work/json.out'"

# The options of the removed identity migration are unknown.
if run_install --role agent --migrate-identity; then
    fail "--migrate-identity is rejected"
else
    pass "--migrate-identity is rejected"
fi
check "--migrate-identity is reported as an unknown option" grep -q "unknown option: --migrate-identity" "$work/out"
if run_install --rollback-identity; then
    fail "--rollback-identity is rejected"
else
    pass "--rollback-identity is rejected"
fi
check "--print-unit agent-legacy is refused" sh -c "! sh '$installer' --print-unit agent-legacy >/dev/null 2>&1"

# An older version than the one installed is refused without --force-downgrade.
before="$(sha256sum /usr/local/bin/domovoi)"
if run_install --role hub --version v9.9.0; then
    fail "a downgrade is refused without --force-downgrade"
else
    pass "a downgrade is refused without --force-downgrade"
fi
check "the downgrade refusal names the flag" grep -q "force-downgrade" "$work/out"
check "binary untouched after a refused downgrade" test "$(sha256sum /usr/local/bin/domovoi)" = "$before"

# A bad checksum stops the install before anything is replaced.
echo "0000000000000000000000000000000000000000000000000000000000000000  domovoi-v9.9.0-$target.tar.gz" \
    >"$work/srv/download/v9.9.0/domovoi-v9.9.0-$target.tar.gz.sha256"
if run_install --role hub --version v9.9.0 --force-downgrade; then
    fail "bad checksum is refused"
else
    pass "bad checksum is refused"
fi
check "bad checksum explained" grep -q "does not match" "$work/out"
check "binary untouched after a bad checksum" test "$(sha256sum /usr/local/bin/domovoi)" = "$before"

# The hub with systemd present: start, readiness, hub.env and the admin account. A fake
# /run/systemd/system and stubs for systemctl, journalctl and the readiness probe stand in
# for systemd; the downloads go through the real curl. The binary of this release is a stub
# that records the password it receives on stdin.
made_systemd_dir=0
if [ ! -d /run/systemd/system ]; then
    mkdir -p /run/systemd/system
    made_systemd_dir=1
fi
stubs="$work/fakebin"
out_dir="$work/stubout"
mkdir -p "$stubs" "$out_dir"
chmod 0777 "$out_dir"
real_curl="$(command -v curl)"
cat >"$stubs/systemctl" <<STUB
#!/bin/sh
echo "\$*" >>"$work/systemctl.log"
case "\$*" in
"is-active --quiet domovoi-hub") [ -e "$work/active" ] ;;
"enable --now domovoi-hub") touch "$work/active" ;;
*) exit 0 ;;
esac
STUB
cat >"$stubs/journalctl" <<STUB
#!/bin/sh
cat "$work/journal.txt" 2>/dev/null || true
STUB
cat >"$stubs/curl" <<STUB
#!/bin/sh
case "\$*" in
*/readyz*)
    echo "\$*" >>"$work/readyz.log"
    [ -e "$work/ready" ]
    exit \$?
    ;;
esac
exec "$real_curl" "\$@"
STUB
cat >"$stubs/ip" <<'STUB'
#!/bin/sh
echo "1.1.1.1 via 10.0.0.1 dev eth0 src 10.0.0.5 uid 0"
STUB
chmod 0755 "$stubs/systemctl" "$stubs/journalctl" "$stubs/curl" "$stubs/ip"

release_recording() { # TAG: a release whose binary records `hub set-password`
    tag="$1"
    name="domovoi-$tag-$target"
    dir="$work/srv/download/$tag"
    mkdir -p "$dir" "$work/build/$name/examples"
    cat >"$work/build/$name/domovoi" <<STUB
#!/bin/sh
case "\$*" in
"version") echo "domovoi $tag (test build)" ;;
"hub set-password --password-stdin")
    IFS= read -r pw
    if [ "\${#pw}" -lt 12 ]; then echo "Error: the password must be at least 12 characters" >&2; exit 1; fi
    printf '%s\n' "\$pw" >"$out_dir/pw.seen"
    id -un >"$out_dir/ran-as"
    : >/var/lib/domovoi/hub.db
    echo "Password set for admin."
    ;;
esac
STUB
    chmod 0755 "$work/build/$name/domovoi"
    tar -C "$work/build" -czf "$dir/$name.tar.gz" "$name"
    (cd "$dir" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
}
release_recording v9.9.2

hub_reset() { # back to a hub that was never installed, with nothing running
    rm -f /etc/systemd/system/domovoi-hub.service /etc/domovoi/hub.env "$work/active" \
        "$out_dir/pw.seen" "$out_dir/ran-as" "$work/systemctl.log" "$work/readyz.log" \
        "$work/journal.txt" "$work/install.log"
    find /var/lib/domovoi -mindepth 1 -delete
    touch "$work/ready"
}
hub_install() { # ARGS...: a hub install of v9.9.2 with the stubs on the path
    PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --role hub --version v9.9.2 "$@" \
        >"$work/out" 2>"$work/err"
}
hub_rc() { rc=0; hub_install "$@" || rc=$?; echo "$rc"; }
json_field() { # FILE EXPRESSION on r
    python3 -I -c '
import json, sys
r = [json.loads(l) for l in open(sys.argv[1]).read().splitlines()][-1]
print(eval(sys.argv[2]))
' "$1" "$2"
}
adminpw="$work/admin.pw"
printf '%s\n' "a long enough pass phrase" >"$adminpw"
chmod 0600 "$adminpw"

hub_reset
check "a fresh hub with a network and an admin installs" test "$(hub_rc --bind lan --admin-password-file "$adminpw")" = 0
check "systemd: the hub was enabled and started" grep -qx 'enable --now domovoi-hub' "$work/systemctl.log"
check "systemd: readiness was polled on loopback" grep -q 'http://127.0.0.1:7420/readyz' "$work/readyz.log"
check "systemd: hub.env is 0640 root:domovoi" test "$(mode_of /etc/domovoi/hub.env) $(owner_of /etc/domovoi/hub.env)" = "640 root:domovoi"
check "systemd: hub.env has the bind" grep -qx 'DOMOVOI_WEB__BIND=0.0.0.0:7420' /etc/domovoi/hub.env
check "systemd: hub.env has the derived public URL" grep -qx 'DOMOVOI_WEB__PUBLIC_URL=http://10.0.0.5:7420' /etc/domovoi/hub.env
check "systemd: hub.env has the derived agent URL" grep -qx 'DOMOVOI_AGENT_SERVER__PUBLIC_URL=wss://10.0.0.5:7421' /etc/domovoi/hub.env
check "systemd: the admin password reached set-password" test "$(cat "$out_dir/pw.seen")" = "a long enough pass phrase"
check "systemd: set-password ran as the hub user" test "$(cat "$out_dir/ran-as")" = domovoi
check "systemd: the summary gives the address to open" grep -q "Open: http://10.0.0.5:7420" "$work/out"
check "systemd: no setup link with an admin" sh -c "! grep -q '/setup?token' '$work/out'"
check "systemd: the summary names the status and log commands" sh -c "grep -q 'systemctl status domovoi-hub' '$work/out' && grep -q 'journalctl -u domovoi-hub -f' '$work/out'"
check "systemd: the password is not in the output" sh -c "! grep -rq 'pass phrase' '$work/out' '$work/err' '$work/install.log'"
check "systemd: the plain http warning was printed" grep -q unencrypted "$work/err"

# The same install with --json.
hub_reset
PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --role hub --version v9.9.2 \
    --public-url https://domovoi.example --bind loopback --admin-password-file "$adminpw" --json >"$work/json.out" 2>/dev/null
check "systemd: the json result is valid" check_json "$work/json.out"
check "systemd: the json result has the web URL" test "$(json_field "$work/json.out" 'r["urls"]["web"]')" = "https://domovoi.example"
check "systemd: the json result says the admin was created" test "$(json_field "$work/json.out" 'r["admin_created"]')" = "True"
check "systemd: the json result says the hub started" test "$(json_field "$work/json.out" 'r["started"]')" = "True"
check "systemd: the json result has no setup URL with an admin" test "$(json_field "$work/json.out" '"setup" in r["urls"]')" = "False"

# No admin: the setup link is read from the journal and put on the public address. It is a
# secret: shown on the terminal and in the json result, never in the log file.
hub_reset
printf 'No account exists yet. Finish setup in the web UI within 24 hours:\n  http://127.0.0.1:7420/setup?token=feedface0123\n' >"$work/journal.txt"
check "systemd: a hub without an admin installs" test "$(hub_rc --public-url https://domovoi.example --bind loopback)" = 0
check "systemd: the setup link is printed on the public address" grep -qx '  https://domovoi.example/setup?token=feedface0123' "$work/out"
check "systemd: the setup token is not in the log file" sh -c "! grep -q feedface0123 '$work/install.log'"
check "systemd: the agent URL is listed as a step left to do" grep -q "Agent server URL" "$work/out"
hub_reset
printf '  http://127.0.0.1:7420/setup?token=feedface0123\n' >"$work/journal.txt"
PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --role hub --version v9.9.2 \
    --public-url https://domovoi.example --json >"$work/json.out" 2>/dev/null
check "systemd: the json result carries the setup URL" test "$(json_field "$work/json.out" 'r["urls"]["setup"]')" = "https://domovoi.example/setup?token=feedface0123"
check "systemd: the json result says no admin was created" test "$(json_field "$work/json.out" 'r["admin_created"]')" = "False"
check "systemd: the json run kept the token out of the log file" sh -c "! grep -q feedface0123 '$work/install.log'"

# --no-start installs the unit and starts nothing.
hub_reset
check "systemd: --no-start installs" test "$(hub_rc --no-start --public-url https://domovoi.example)" = 0
check "systemd: --no-start starts nothing" sh -c "! grep -qE 'enable --now|restart' '$work/systemctl.log'"
check "systemd: --no-start still prints the manual steps" grep -q "systemctl enable --now domovoi-hub" "$work/out"
check "systemd: --no-start probed nothing" test ! -e "$work/readyz.log"
check "systemd: --no-start without an admin points at the setup link" grep -q "journalctl -u domovoi-hub | grep setup" "$work/out"
hub_reset
check "systemd: --no-start with an admin installs" test "$(hub_rc --no-start --public-url https://domovoi.example --admin-password-file "$adminpw")" = 0
check "systemd: --no-start with an admin asks to open the address, not for a setup link" sh -c "grep -q 'Open https://domovoi.example and sign in as admin' '$work/out' && ! grep -q 'grep setup' '$work/out'"
check "systemd: --no-start still wrote hub.env" grep -qx 'DOMOVOI_WEB__PUBLIC_URL=https://domovoi.example' /etc/domovoi/hub.env

# A hub that never becomes ready: exit 40 with the journal; the token is hidden.
hub_reset
rm -f "$work/ready"
printf 'hub: cannot bind\n  http://127.0.0.1:7420/setup?token=feedface0123\n' >"$work/journal.txt"
DOMOVOI_READY_TIMEOUT=2
export DOMOVOI_READY_TIMEOUT
check "systemd: a hub that never answers exits 40" test "$(hub_rc)" = 40
unset DOMOVOI_READY_TIMEOUT
check "systemd: the timeout shows the journal" grep -q "hub: cannot bind" "$work/out" "$work/err"
check "systemd: the timeout hides the token" sh -c "! grep -q feedface0123 '$work/out' '$work/err' '$work/install.log'"

# An upgrade restarts a running hub, leaves a stopped one stopped, and never touches an admin.
hub_reset
hub_install --public-url https://domovoi.example --admin-password-file "$adminpw" || true
: >"$work/systemctl.log"
rm -f "$out_dir/pw.seen"
touch "$work/active"
check "systemd: an upgrade of a running hub succeeds" test "$(hub_rc --admin-password-file "$adminpw")" = 0
check "systemd: a running hub is restarted" grep -qx 'restart domovoi-hub' "$work/systemctl.log"
check "systemd: an upgrade never resets the admin" test ! -e "$out_dir/pw.seen"
check "systemd: the skipped admin is explained" grep -q "never reset" "$work/out"
check "systemd: hub.env is unchanged by an upgrade without flags" grep -qx 'DOMOVOI_WEB__PUBLIC_URL=https://domovoi.example' /etc/domovoi/hub.env
: >"$work/systemctl.log"
rm -f "$work/active"
check "systemd: an upgrade of a stopped hub succeeds" test "$(hub_rc)" = 0
check "systemd: a stopped hub is left stopped" sh -c "! grep -qE 'enable|restart|start' '$work/systemctl.log'"
check "systemd: that is said" grep -q "left stopped" "$work/out"

# A password the hub rejects stops the install with its message.
hub_reset
printf '%s\n' "tiny-pw" >"$adminpw"
check "systemd: a rejected password exits 40" test "$(hub_rc --admin-password-file "$adminpw")" = 40
check "systemd: the hub message is shown" grep -q "at least 12 characters" "$work/out" "$work/err"
check "systemd: the hub was not started after that" sh -c "! grep -qs 'enable --now' '$work/systemctl.log'"
rm -f "$adminpw"

# The dry run lists the start, hub.env and the admin.
hub_reset
printf '%s\n' "a long enough pass phrase" >"$adminpw"
chmod 0600 "$adminpw"
check "systemd: the dry run succeeds" test "$(hub_rc --dry-run --bind lan --admin-password-file "$adminpw")" = 0
check "systemd: the dry run lists the start" grep -q "enable --now domovoi-hub" "$work/out"
check "systemd: the dry run lists hub.env" grep -q "DOMOVOI_WEB__PUBLIC_URL=http://10.0.0.5:7420" "$work/out"
check "systemd: the dry run lists the admin" grep -q "admin account" "$work/out"
check "systemd: the dry run wrote no hub.env" test ! -e /etc/domovoi/hub.env
check "systemd: the dry run started nothing" test ! -e "$work/systemctl.log"
rm -f "$adminpw"
# --- Agent enrollment, --role both and the cross-boundary rule -----------------------------
# A release whose binary records every call in $out_dir/calls.log (together with the systemctl
# stub, so the order of the two is visible) and answers `agent enroll`, `agent status` and
# `hub token create` the way the real one does.
fp="$(printf 'ab%.0s' $(seq 32))"
fpc="$(printf 'AB:%.0s' $(seq 31))AB"
tok="tok_SECRET0123456789abcd"
cat >"$stubs/hostname" <<STUB
#!/bin/sh
case "\$1" in
-s) echo "Test_Host" ;;
-I) exit 1 ;;
*) echo "Test_Host.example" ;;
esac
STUB
chmod 0755 "$stubs/hostname"
cat >"$stubs/systemctl" <<STUB
#!/bin/sh
echo "\$*" >>"$work/systemctl.log"
echo "systemctl \$*" >>"$out_dir/calls.log"
if [ "\$*" = "daemon-reload" ] && [ -e "$work/daemon-fails" ]; then echo "Failed to connect to bus" >&2; exit 1; fi
case "\$*" in
"is-active --quiet domovoi-hub") [ -e "$work/active" ] ;;
"enable --now domovoi-hub" | "start domovoi-hub") touch "$work/active" ;;
"stop domovoi-hub") rm -f "$work/active" ;;
"is-active --quiet domovoi-agent") [ -e "$work/agent-active" ] ;;
"enable --now domovoi-agent") touch "$work/agent-active" ;;
*) exit 0 ;;
esac
STUB
release_enrolling() { # TAG
    tag="$1"
    name="domovoi-$tag-$target"
    dir="$work/srv/download/$tag"
    mkdir -p "$dir" "$work/build/$name/examples"
    cat >"$work/build/$name/domovoi" <<STUB
#!/bin/sh
echo "domovoi \$*" >>"$out_dir/calls.log"
case "\$*" in
"version") echo "domovoi $tag (test build)" ;;
"agent enroll "*)
    printf '%s' "\${DOMOVOI_ENROLL_TOKEN:-unset}" >"$out_dir/enroll.env"
    id -un >"$out_dir/enroll-ran-as"
    if [ -e "$out_dir/enroll-fails" ]; then
        echo "error: the token \${DOMOVOI_ENROLL_TOKEN} was refused (--token \${DOMOVOI_ENROLL_TOKEN})" >&2
        exit 1
    fi
    : >/etc/domovoi/agent.toml
    echo "Enrolled as host 1 (agent 2)."
    ;;
"agent journal init")
    id -un >"$out_dir/journal-ran-as"
    if [ -e "$out_dir/journal-fails" ]; then
        echo "error: the action journal could not be created" >&2
        exit 1
    fi
    if [ -e /var/lib/domovoi-agent/action-journal.json ] || [ -e /var/lib/domovoi-agent/action-journal.bin ]; then
        echo "error: an action journal exists" >&2
        exit 1
    fi
    : >/var/lib/domovoi-agent/action-journal.json
    : >/var/lib/domovoi-agent/action-journal.bin
    echo "The action journal at /var/lib/domovoi-agent now uses epoch 1."
    ;;
"agent status")
    if [ -e "$out_dir/connect" ]; then
        echo "connected since 2030-01-01T00:00:00Z."
    else
        echo "reconnecting: connection refused (updated now)"
    fi
    ;;
"hub token create "*)
    id -un >"$out_dir/token-ran-as"
    if [ -e "$work/active" ]; then echo "Error: the hub is running and holds the lock" >&2; exit 1; fi
    if [ -e "$out_dir/token-fails" ]; then echo "Error: cannot open the database" >&2; exit 1; fi
    echo "Enrollment token for \$4, valid until 2030-01-01T00:15:00Z."
    echo "Hub fingerprint: $fpc"
    echo "Run this on \$4:"
    echo
    echo "  domovoi agent enroll --hub wss://127.0.0.1:7421/ --token $tok --fingerprint $fpc --name \$4"
    ;;
esac
STUB
    chmod 0755 "$work/build/$name/domovoi"
    tar -C "$work/build" -czf "$dir/$name.tar.gz" "$name"
    (cd "$dir" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
}
release_enrolling v9.9.3
calls_reset() {
    : >"$out_dir/calls.log"
    chmod 0666 "$out_dir/calls.log"
}
enroll_reset() { # a host with a hub user and nothing running or enrolled
    hub_reset
    rm -f /etc/domovoi/agent.toml "$work/agent-active" "$out_dir/connect" "$out_dir/enroll-fails" \
        /var/lib/domovoi-agent/action-journal.json /var/lib/domovoi-agent/action-journal.bin "$out_dir/journal-fails" "$out_dir/journal-ran-as" \
        "$out_dir/token-fails" "$out_dir/enroll.env" "$out_dir/token-ran-as" "$out_dir/enroll-ran-as"
    calls_reset
    printf '%s\n' "$journal_link" >"$work/journal.txt"
}
agent_install() { # ARGS...: the enrolling release with the stubs on the path
    PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --version v9.9.3 "$@" \
        >"$work/out" 2>"$work/err"
}
agent_rc() { rc=0; agent_install "$@" || rc=$?; echo "$rc"; }
first_call() { grep -n -- "$1" "$out_dir/calls.log" | head -n 1 | cut -d: -f1; }
in_order() { # PATTERN...: the first line of each is after the first line of the one before
    io_prev=0
    for io_pat in "$@"; do
        io_n="$(first_call "$io_pat")"
        { [ -n "$io_n" ] && [ "$io_n" -gt "$io_prev" ]; } || return 1
        io_prev="$io_n"
    done
}
no_token() { # FILES...: the token is in none of them
    ! grep -q "$tok" "$@" 2>/dev/null
}
no_token_run() { # everything a run writes: its output, the install log and the recorded calls
    no_token "$work/out" "$work/err" "$work/install.log" "$out_dir/calls.log" "$work/systemctl.log"
}
journal_link='  http://127.0.0.1:7420/setup?token=feedface0123'
tokf="$work/my-token"
(umask 077 && printf '%s\n' "$tok" >"$tokf")
adminpw="$work/admin.pw"
(umask 077 && printf '%s\n' "a long enough pass phrase" >"$adminpw")
agent_journal() { printf 'agent: dialing wss://hub.lan:7421\nagent: enroll --token %s\n' "$tok" >"$work/journal.txt"; }

# An agent with a hub user on the machine keeps the root layout, and is enrolled and started.
enroll_reset
touch "$out_dir/connect"
check "enroll: an agent with the trio installs" test "$(agent_rc --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf" --name nas)" = 0
check "enroll: enroll got the hub, fingerprint and name" grep -qx "domovoi agent enroll --hub wss://hub.lan:7421 --fingerprint $fp --name nas" "$out_dir/calls.log"
check "enroll: the token reached enroll through its environment" test "$(cat "$out_dir/enroll.env")" = "$tok"
check "enroll: enroll ran as root" test "$(cat "$out_dir/enroll-ran-as")" = root
check "enroll: the token is in no argument, output or log" no_token_run
check "enroll: the order is enroll, journal init, enable, status" in_order 'domovoi agent enroll' 'domovoi agent journal init' 'systemctl enable --now domovoi-agent' 'domovoi agent status'
check "enroll: the journal was created as root" test "$(cat "$out_dir/journal-ran-as")" = root
check "enroll: the journal files exist" sh -c "test -e /var/lib/domovoi-agent/action-journal.json && test -e /var/lib/domovoi-agent/action-journal.bin"
check "enroll: the summary says the agent is connected" grep -q "The agent is enrolled as host nas and connected to wss://hub.lan:7421" "$work/out"
check "enroll: the agent next steps of before are not printed" sh -c "! grep -q 'Add host' '$work/out'"
check "enroll: the boundary notice still names the hub" grep -q "privilege boundary not in effect" "$work/out"
check "enroll: no temporary token file is left behind" sh -c "! find /tmp -name enroll.tok 2>/dev/null | grep -q ."

# The token from the environment, the default name, and --json.
enroll_reset
touch "$out_dir/connect"
DOMOVOI_ENROLL_TOKEN="$tok"
export DOMOVOI_ENROLL_TOKEN
PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --version v9.9.3 --role agent \
    --hub wss://hub.lan:7421/ --fingerprint "$fpc" --json >"$work/json.out" 2>"$work/err" || true
unset DOMOVOI_ENROLL_TOKEN
check "enroll: the json result is valid" check_json "$work/json.out"
check "enroll: the json result names the enrolled host" test "$(json_field "$work/json.out" 'r["enrolled_host"]')" = test-host
check "enroll: the json result says the agent started" test "$(json_field "$work/json.out" 'r["agent_started"]')" = True
check "enroll: the default name is the cleaned short host name" grep -q -- "--name test-host" "$out_dir/calls.log"
check "enroll: the environment token reached enroll" test "$(cat "$out_dir/enroll.env")" = "$tok"
check "enroll: the token is not in the json, the error output or the log" no_token "$work/json.out" "$work/err" "$work/install.log" "$out_dir/calls.log"
check "enroll: the colon fingerprint is passed as given" grep -q -- "--fingerprint $fpc" "$out_dir/calls.log"
check "enroll: a trailing slash on the hub address is dropped" grep -q -- "--hub wss://hub.lan:7421 " "$out_dir/calls.log"

# An agent that never connects: exit 40, the journal with the token hidden.
enroll_reset
agent_journal
DOMOVOI_AGENT_TIMEOUT=2
export DOMOVOI_AGENT_TIMEOUT
check "enroll: an agent that never connects exits 40" test "$(agent_rc --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf" --name nas)" = 40
unset DOMOVOI_AGENT_TIMEOUT
check "enroll: the timeout shows the agent's journal" grep -q "agent: dialing wss://hub.lan:7421" "$work/out" "$work/err"
check "enroll: the timeout hides the token" no_token_run
check "enroll: the timeout says what to check by hand" grep -q "agent status" "$work/out"
check "enroll: the unit was started before the wait" grep -qx 'enable --now domovoi-agent' "$work/systemctl.log"
# A refused enroll: exit 40 and the unit is not started.
enroll_reset
touch "$out_dir/enroll-fails"
check "enroll: a refused enroll exits 40" test "$(agent_rc --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf" --name nas)" = 40
check "enroll: a refused enroll does not start the agent" sh -c "! grep -q 'enable --now domovoi-agent' '$work/systemctl.log'"
check "enroll: a refused enroll shows the message with the token hidden" grep -q "was refused (--token REDACTED" "$work/out"
check "enroll: a refused enroll keeps the token out of everything" no_token_run
check "enroll: a refused enroll says what to do by hand" grep -q "Add host" "$work/out"
# An existing journal is kept: init would refuse, so it is not run.
enroll_reset
touch "$out_dir/connect"
: >/var/lib/domovoi-agent/action-journal.json
check "enroll: an agent with a journal installs" test "$(agent_rc --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf" --name nas)" = 0
check "enroll: an existing journal is not initialised again" sh -c "! grep -q 'agent journal init' '$out_dir/calls.log'"
# A journal that cannot be created: exit 40 and the unit is not started.
enroll_reset
touch "$out_dir/journal-fails"
check "enroll: a failed journal init exits 40" test "$(agent_rc --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf" --name nas)" = 40
check "enroll: a failed journal init does not start the agent" sh -c "! grep -q 'enable --now domovoi-agent' '$work/systemctl.log'"
check "enroll: a failed journal init names the command to run" grep -q "sudo /usr/local/bin/domovoi agent journal init" "$work/out" "$work/err"
# The dry run names the steps and runs none of them.
enroll_reset
check "enroll: the dry run succeeds" test "$(agent_rc --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf" --name nas --dry-run)" = 0
check "enroll: the dry run lists the enroll command" grep -q "domovoi agent enroll --hub wss://hub.lan:7421 --fingerprint $fp --name nas" "$work/out"
check "enroll: the dry run lists the journal" grep -q "domovoi agent journal init" "$work/out"
check "enroll: the dry run lists the start and the wait" grep -q "enable --now domovoi-agent, then wait up to 30 s" "$work/out"
check "enroll: the dry run ran nothing" sh -c "! grep -qE 'agent enroll|journal init|token create|systemctl' '$out_dir/calls.log'"
check "enroll: the dry run does not show the token" no_token "$work/out" "$work/err"

# --role both: hub, stop, token, hub, enroll, enable.
enroll_reset
touch "$out_dir/connect"
check "both: a machine with a hub and an agent installs" test "$(agent_rc --role both --public-url https://domovoi.example --bind loopback --name nas --admin-password-file "$adminpw")" = 0
check "both: the order is start, stop, token, start, enroll, enable, status" in_order \
    'systemctl enable --now domovoi-hub' 'systemctl stop domovoi-hub' 'domovoi hub token create nas --ttl 15m' \
    'systemctl start domovoi-hub' 'domovoi agent enroll' 'domovoi agent journal init' 'systemctl enable --now domovoi-agent' 'domovoi agent status'
check "both: the token was made as the hub user" test "$(cat "$out_dir/token-ran-as")" = domovoi
check "both: enroll ran as root" test "$(cat "$out_dir/enroll-ran-as")" = root
check "both: enroll got the token only through its environment" test "$(cat "$out_dir/enroll.env")" = "$tok"
check "both: enroll dialed the local agent server with the hub's fingerprint" grep -qx "domovoi agent enroll --hub wss://127.0.0.1:7421 --fingerprint $fpc --name nas" "$out_dir/calls.log"
check "both: the token is in no argument, output or log" no_token_run
check "both: the hub is running afterwards" test -e "$work/active"
check "both: the agent is running afterwards" test -e "$work/agent-active"
check "both: readiness was probed after each start" test "$(grep -c readyz "$work/readyz.log")" -ge 2
check "both: the agent keeps the root layout and says so" grep -q "privilege boundary not in effect" "$work/out"
check "both: the summary names the host and the connection" grep -q "enrolled as host nas and connected" "$work/out"
check "both: the summary leaves only the hand steps" sh -c "grep -q 'Discord bot token' '$work/out' && grep -q 'LLM provider' '$work/out' && grep -q 'enroll any other machine' '$work/out'"
check "both: the summary lists the agent checks" grep -q "systemctl status domovoi-agent" "$work/out"
check "both: no temporary token file is left behind" sh -c "! find /tmp -name enroll.tok -o -name token.out 2>/dev/null | grep -q ."
# The json result and the setup link without an admin.
enroll_reset
touch "$out_dir/connect"
PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --version v9.9.3 --role both \
    --public-url https://domovoi.example --bind loopback --json >"$work/json.out" 2>"$work/err" || true
check "both: the json result is valid" check_json "$work/json.out"
check "both: the json result has the role and the layout" test "$(json_field "$work/json.out" '(r["role"], r["layout"])')" = "('both', 'hub')"
check "both: the json result names the enrolled host" test "$(json_field "$work/json.out" 'r["enrolled_host"]')" = test-host
check "both: the json result says both started" test "$(json_field "$work/json.out" '(r["started"], r["agent_started"])')" = "(True, True)"
check "both: the json result carries the setup link" test "$(json_field "$work/json.out" 'r["urls"]["setup"]')" = "https://domovoi.example/setup?token=feedface0123"
check "both: the json result says no admin was created" test "$(json_field "$work/json.out" 'r["admin_created"]')" = "False"
check "both: the json run kept the token and the setup link out of the log" no_token "$work/json.out" "$work/err" "$work/install.log" "$out_dir/calls.log"
check "both: the setup link is not in the log file" sh -c "! grep -q feedface0123 '$work/install.log'"
# A re-run is an upgrade: nothing is enrolled again.
calls_reset
check "both: a second run succeeds" test "$(agent_rc --role both)" = 0
check "both: a second run makes no token" sh -c "! grep -q 'token create' '$out_dir/calls.log'"
check "both: a second run enrolls nothing" sh -c "! grep -q 'agent enroll' '$out_dir/calls.log'"
check "both: a second run restarts the hub and the agent" sh -c "grep -qx 'systemctl restart domovoi-hub' '$out_dir/calls.log' && grep -qx 'systemctl restart domovoi-agent' '$out_dir/calls.log'"
check "both: a second run says the agent was not enrolled again" grep -q "already enrolled" "$work/out"
check "both: a second run leaves the journal alone" sh -c "! grep -q 'journal init' '$out_dir/calls.log'"
# An agent set up by an older installer has no journal: a re-run creates it before restarting.
rm -f /var/lib/domovoi-agent/action-journal.json /var/lib/domovoi-agent/action-journal.bin
calls_reset
check "both: a run that finds no journal succeeds" test "$(agent_rc --role both)" = 0
check "both: the journal is created before the agent restarts" in_order 'domovoi agent journal init' 'systemctl restart domovoi-agent'
check "both: the healing run enrolls nothing" sh -c "! grep -q 'agent enroll' '$out_dir/calls.log'"
# Failures after the hub started leave it running and say what to run.
enroll_reset
touch "$out_dir/token-fails"
check "both: a failed token exits 40" test "$(agent_rc --role both --bind loopback --name nas)" = 40
check "both: a failed token leaves the hub running" test -e "$work/active"
check "both: a failed token started the hub again" in_order 'systemctl stop domovoi-hub' 'systemctl start domovoi-hub'
check "both: a failed token shows the hub's message" grep -q "cannot open the database" "$work/out" "$work/err"
check "both: a failed token says what to run by hand" sh -c "grep -q 'Add host' '$work/out' && grep -q 'systemctl enable --now domovoi-agent' '$work/out'"
check "both: a failed token enrolled nothing" sh -c "! grep -q 'agent enroll' '$out_dir/calls.log'"
enroll_reset
touch "$out_dir/enroll-fails"
check "both: a refused enroll exits 40" test "$(agent_rc --role both --bind loopback --name nas)" = 40
check "both: a refused enroll leaves the hub running" test -e "$work/active"
check "both: a refused enroll keeps the token out of everything" no_token_run
check "both: a refused enroll says what to run by hand" grep -q "Add host" "$work/out"
enroll_reset
DOMOVOI_AGENT_TIMEOUT=2
export DOMOVOI_AGENT_TIMEOUT
check "both: an agent that never connects exits 40" test "$(agent_rc --role both --bind loopback --name nas)" = 40
unset DOMOVOI_AGENT_TIMEOUT
check "both: an agent that never connects leaves the hub running" test -e "$work/active"
check "both: the timeout keeps the token out of everything" no_token_run
# The dry run of both lists every step.
enroll_reset
check "both: the dry run succeeds" test "$(agent_rc --role both --bind loopback --name nas --dry-run)" = 0
check "both: the dry run lists the token and the enrollment" sh -c "grep -q 'stop the hub, make a 15 minute enrollment token for nas' '$work/out' && grep -q 'domovoi agent enroll --hub wss://127.0.0.1:7421 --name nas' '$work/out'"
check "both: the dry run ran nothing" sh -c "! grep -qE 'agent enroll|token create|systemctl' '$out_dir/calls.log'"
check "both: --no-start is refused" test "$(agent_rc --role both --no-start)" = 2
check "help says --role both refuses --no-start" sh -c "sh '$installer' --help | tr '\\n' ' ' | grep -Eq 'role both +refuses it'"

# A hub database from before 0.1.0 is not upgraded.
release v0.0.9 ""
release v0.1.0 ""
old_install() { # ARGS...
    PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" "$@" >"$work/out" 2>"$work/err"
}
old_rc() { rc=0; old_install "$@" || rc=$?; echo "$rc"; }
enroll_reset
check "schema: an old hub installs" test "$(old_rc --role hub --version v0.0.9 --force-downgrade)" = 0
touch /var/lib/domovoi/hub.db
: >"$work/systemctl.log"
check "schema: 0.1.0 over a 0.0.9 database exits 10" test "$(old_rc --role hub --version v0.1.0)" = 10
check "schema: the refusal names the flag" grep -q -- "--accept-data-reset" "$work/err"
check "schema: the refusal says the schema is replaced" grep -q "replaces the whole database schema" "$work/err"
check "schema: the refused run left the old binary" sh -c "/usr/local/bin/domovoi version | grep -q v0.0.9"
check "schema: the refused run restarted nothing" sh -c "! grep -q restart '$work/systemctl.log'"
check "schema: an agent is not refused" test "$(old_rc --role agent --version v0.1.0 --dry-run)" = 0
check "schema: a dry run of the hub is refused too" test "$(old_rc --role hub --version v0.1.0 --dry-run)" = 10
check "schema: the flag lets the install go on" test "$(old_rc --role hub --version v0.1.0 --accept-data-reset)" = 0
check "schema: the new binary is installed" sh -c "/usr/local/bin/domovoi version | grep -q v0.1.0"
check "schema: the old data directory is untouched" test -e /var/lib/domovoi/hub.db
check "schema: the hub is not restarted" sh -c "! grep -qE 'restart|enable --now' '$work/systemctl.log'"
check "schema: the warning says the data is left untouched" grep -q "left untouched" "$work/out" "$work/err"
check "schema: the steps to move the directory are printed" sh -c "grep -q 'mv /var/lib/domovoi /var/lib/domovoi.before-0.1.0' '$work/out' && grep -q 'systemctl enable --now domovoi-hub' '$work/out'"
check "schema: an old hub goes back in for the next case" test "$(old_rc --role hub --version v0.0.9 --force-downgrade)" = 0
check "schema: both with the flag installs and enrolls nothing" test "$(old_rc --role both --version v0.1.0 --accept-data-reset)" = 0
check "schema: both with the flag made no token" sh -c "! grep -q 'token create' '$out_dir/calls.log'"
check "schema: both with the flag says to run again" grep -q "Run this installer again with --role both" "$work/out"
check "schema: a flag for an agent is refused" test "$(old_rc --role agent --accept-data-reset)" = 2
rm -f /var/lib/domovoi/hub.db

# --- Uninstall and rollback with real accounts, directories and units ----------------------
# The binary of this release answers what the installer asks of it (agent dirs, the self-test,
# the journal, the helper's firewall cleanup) and records every call, so a real install of the
# agent behind its privilege boundary and of the hub runs against the stub systemctl.
release_full() { # TAG
    tag="$1"
    name="domovoi-$tag-$target"
    dir="$work/srv/download/$tag"
    mkdir -p "$dir" "$work/build/$name/examples"
    cat >"$work/build/$name/domovoi" <<STUB
#!/bin/sh
echo "domovoi \$*" >>"$out_dir/calls.log"
case "\$*" in
"version") echo "domovoi $tag (test build)" ;;
"agent dirs")
    echo "skills_dir=/srv/dt-skills"
    echo "packages_dir=/srv/dt-packages"
    ;;
"agent selftest --installer --record" | "agent selftest --installer") echo "ST-01 pass" ;;
"helper nft-recreate") echo "no firewall table" ;;
"helper init")
    (umask 077 && : >/var/lib/domovoi-helper/state.lock)
    ;;
"agent journal init")
    : >/var/lib/domovoi-agent/action-journal.json
    : >/var/lib/domovoi-agent/action-journal.bin
    ;;
"hub set-password --password-stdin")
    IFS= read -r pw
    : >/var/lib/domovoi/hub.db
    echo "Password set for admin."
    ;;
esac
STUB
    chmod 0755 "$work/build/$name/domovoi"
    tar -C "$work/build" -czf "$dir/$name.tar.gz" "$name"
    (cd "$dir" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
}
release_full v9.9.5

# Back to a machine that never had Domovoi: every file, directory, unit and account.
wipe_machine() {
    rm -f /etc/systemd/system/domovoi-* /usr/local/bin/domovoi "$work/active" "$work/agent-active"
    rm -rf /etc/systemd/system/domovoi-*.d /usr/local/share/domovoi /etc/domovoi /var/lib/domovoi \
        /var/lib/domovoi-agent /var/lib/domovoi-packages /var/lib/domovoi-helper /srv/dt-skills /srv/dt-packages
    rm -f /var/log/domovoi-install.log "$work/install.log"
    for wm_account in domovoi domovoi-agent domovoi-skill; do
        ! id "$wm_account" >/dev/null 2>&1 || userdel "$wm_account" >/dev/null 2>&1 || true
        ! getent group "$wm_account" >/dev/null 2>&1 || groupdel "$wm_account" >/dev/null 2>&1 || true
    done
    : >"$work/systemctl.log"
    calls_reset
    touch "$work/ready"
}
un_install() { # ARGS...: the installer with the stubs on the path, the default log file
    PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" "$@" >"$work/out" 2>"$work/err"
}
un_rc() { rc=0; un_install "$@" || rc=$?; echo "$rc"; }
full_install() { # the agent behind its boundary first, then a hub
    wipe_machine
    printf '%s\n' "$journal_link" >"$work/journal.txt"
    test "$(PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --version v9.9.5 --role agent >"$work/full.out" 2>&1 && echo 0 || echo 1)" = 0 &&
        test "$(PATH="$stubs:$PATH" DOMOVOI_INSTALL_LOG="$work/install.log" sh "$installer" --version v9.9.5 --role hub --bind loopback --no-start >>"$work/full.out" 2>&1 && echo 0 || echo 1)" = 0
}
state_of() { # a listing of everything an uninstall may touch
    find /etc/domovoi /var/lib/domovoi /var/lib/domovoi-agent /var/lib/domovoi-packages /var/lib/domovoi-helper \
        /usr/local/share/domovoi /srv/dt-skills /srv/dt-packages /etc/systemd/system -name '*domovoi*' -o -path '/srv/dt-*' 2>/dev/null | sort
    ls -l /usr/local/bin/domovoi 2>/dev/null
    getent passwd domovoi domovoi-agent domovoi-skill | sort
}

check "uninstall: the full install (agent behind the boundary, then the hub) succeeds" full_install
if [ ! -e /etc/domovoi/installer-created ]; then cat "$work/full.out"; fi
check "uninstall: the install made the three accounts" sh -c "id domovoi >/dev/null && id domovoi-agent >/dev/null && id domovoi-skill >/dev/null"
check "uninstall: the install made the units" sh -c "test -e /etc/systemd/system/domovoi-hub.service && test -e /etc/systemd/system/domovoi-agent.service && test -e /etc/systemd/system/domovoi-helper.socket && test -e /etc/systemd/system/domovoi-skill-runner.socket"
check "uninstall: the installer record lists the accounts" sh -c "grep -qx 'user domovoi' /etc/domovoi/installer-created && grep -qx 'user domovoi-agent' /etc/domovoi/installer-created && grep -qx 'user domovoi-skill' /etc/domovoi/installer-created"
check "uninstall: the agent's own directories were made outside the default tree" test -d /srv/dt-skills
mkdir -p /etc/systemd/system/domovoi-hub.service.d
echo "[Service]" >/etc/systemd/system/domovoi-hub.service.d/override.conf
echo "keep me" >/etc/domovoi.sentinel
echo "keep me" >/var/lib/domovoi.sentinel
echo "[Unit]" >/etc/systemd/system/my-own.service
mkdir -p /etc/systemd/system/my-own.service.d
unrelated_ok() {
    test "$(cat /etc/domovoi.sentinel)" = "keep me" && test "$(cat /var/lib/domovoi.sentinel)" = "keep me" &&
        test -e /etc/systemd/system/my-own.service && test -d /etc/systemd/system/my-own.service.d
}

# The dry run changes nothing and lists the plan.
state_of >"$work/state.before"
calls_reset
: >"$work/systemctl.log"
check "uninstall: the dry run exits 0" test "$(un_rc --uninstall --dry-run)" = 0
state_of >"$work/state.after"
check "uninstall: the dry run left every path and account as it was" cmp -s "$work/state.before" "$work/state.after"
check "uninstall: the dry run stopped nothing" sh -c "! grep -qE 'disable|stop|daemon-reload' '$work/systemctl.log'"
check "uninstall: the dry run ran no helper cleanup" sh -c "! grep -q 'nft-recreate' '$out_dir/calls.log'"
check "uninstall: the dry run lists the stops" grep -q "stop .*domovoi-hub.service" "$work/out"
check "uninstall: the dry run lists the binary and the examples" sh -c "grep -q 'remove .*/usr/local/bin/domovoi' '$work/out' && grep -q 'remove .*/usr/local/share/domovoi' '$work/out'"
check "uninstall: the dry run lists what it keeps" sh -c "grep -q 'keep .*/etc/domovoi' '$work/out' && grep -q 'keep .*/var/lib/domovoi-agent' '$work/out'"
check "uninstall: the dry run says nothing was changed" grep -q "Dry run: nothing was changed" "$work/out"
check "uninstall: the purge dry run lists the deletions" sh -c "sh '$installer' --uninstall --purge --dry-run 2>&1 | grep -q 'delete .*/var/lib/domovoi-agent'"
check "uninstall: the purge dry run changed nothing" sh -c "test -e /etc/domovoi/installer-created && id domovoi-agent >/dev/null 2>&1"

# --uninstall --yes: the program goes, the data stays.
: >"$work/systemctl.log"
check "uninstall: --uninstall --yes exits 0" test "$(un_rc --uninstall --yes)" = 0
for unit in domovoi-agent.service domovoi-hub.service domovoi-helper.socket domovoi-skill-runner.socket; do
    check "uninstall: $unit was stopped and disabled" grep -qx "disable --now $unit" "$work/systemctl.log"
    check "uninstall: $unit is gone" test ! -e "/etc/systemd/system/$unit"
done
check "uninstall: the instance units were stopped" grep -q "^stop domovoi-skill-runner@\*.service" "$work/systemctl.log"
check "uninstall: systemd reloaded" grep -qx "daemon-reload" "$work/systemctl.log"
check "uninstall: the helper's firewall table was cleaned up" grep -qx "domovoi helper nft-recreate" "$out_dir/calls.log"
check "uninstall: the unit templates are gone" sh -c "test ! -e '/etc/systemd/system/domovoi-skill-runner@.service' && test ! -e '/etc/systemd/system/domovoi-helper@.service'"
check "uninstall: the binary is gone" test ! -e /usr/local/bin/domovoi
check "uninstall: the examples directory is gone" test ! -e /usr/local/share/domovoi
check "uninstall: the configuration is kept" test -f /etc/domovoi/installer-created
check "uninstall: the data directories are kept" sh -c "test -d /var/lib/domovoi && test -d /var/lib/domovoi-agent && test -d /var/lib/domovoi-packages && test -d /var/lib/domovoi-helper"
check "uninstall: the skills directory is kept" test -d /etc/domovoi/skills
check "uninstall: the directories outside the tree are kept" sh -c "test -d /srv/dt-skills && test -d /srv/dt-packages"
check "uninstall: the accounts are kept" sh -c "id domovoi >/dev/null && id domovoi-agent >/dev/null && id domovoi-skill >/dev/null"
check "uninstall: the drop-in is kept without --purge" test -f /etc/systemd/system/domovoi-hub.service.d/override.conf
check "uninstall: unrelated files are untouched" unrelated_ok
check "uninstall: the summary names what was kept" sh -c "grep -q 'kept on purpose' '$work/out' && grep -q '/etc/domovoi' '$work/out' && grep -q '/var/lib/domovoi-agent' '$work/out' && grep -q 'account domovoi-agent' '$work/out' && grep -q 'account domovoi-skill' '$work/out'"
check "uninstall: the summary says --purge deletes what was kept" grep -q -- "--purge deletes" "$work/out"
check "uninstall: the text summary prints the purge command" grep -q "sudo sh install.sh --uninstall --purge" "$work/out"
check "uninstall: a second run finds nothing to remove and exits 10" test "$(un_rc --uninstall --yes)" = 10
check "uninstall: that run says the data and accounts are still there" grep -q "nothing to uninstall.*still there" "$work/err"

# --purge: the data, the configuration, the drop-ins, the log and the accounts go too.
# The program is installed again over the kept data and accounts, and the default log is used.
printf '%s\n' "$journal_link" >"$work/journal.txt"
check "uninstall: a hub installs again over the kept data" test "$(un_rc --version v9.9.5 --role hub --bind loopback --no-start)" = 0
echo "x" >/var/log/domovoi-install.log
PATH="$stubs:$PATH" env -u DOMOVOI_INSTALL_LOG sh "$installer" --uninstall --purge --yes --json >"$work/json.out" 2>"$work/err" || true
check "uninstall: the json purge result is valid" check_json "$work/json.out"
check "uninstall: the json result says uninstall and purge" test "$(json_field "$work/json.out" '(r["mode"], r["purge"], r["rolled_back"])')" = "('uninstall', True, False)"
check "uninstall: the json result lists the removed paths" test "$(json_field "$work/json.out" '("/usr/local/bin/domovoi" in r["removed"], "/var/lib/domovoi-agent" in r["removed"], "/etc/domovoi" in r["removed"])')" = "(True, True, True)"
check "uninstall: the json result lists the removed accounts" test "$(json_field "$work/json.out" '("account domovoi-skill" in r["removed"])')" = "True"
check "uninstall: the json result names the binary path" test "$(json_field "$work/json.out" 'r["paths"]["binary"]')" = "/usr/local/bin/domovoi"
check "uninstall: the json purge ran only JSON on standard output" sh -c "! grep -v '^{' '$work/json.out'"
check "uninstall: purge removed /etc/domovoi" test ! -e /etc/domovoi
for path in /var/lib/domovoi /var/lib/domovoi-agent /var/lib/domovoi-packages /var/lib/domovoi-helper /srv/dt-skills /srv/dt-packages \
    /etc/systemd/system/domovoi-hub.service.d /var/log/domovoi-install.log /usr/local/bin/domovoi; do
    check "uninstall: purge removed $path" test ! -e "$path"
done
check "uninstall: purge removed the accounts and their groups the installer made" \
    sh -c "! id domovoi >/dev/null 2>&1 && ! id domovoi-agent >/dev/null 2>&1 && ! id domovoi-skill >/dev/null 2>&1 && ! getent group domovoi >/dev/null && ! getent group domovoi-agent >/dev/null && ! getent group domovoi-skill >/dev/null"
check "uninstall: purge left the sibling of /etc/domovoi alone" unrelated_ok
check "uninstall: purge left no unit of Domovoi" sh -c "! ls /etc/systemd/system | grep -q '^domovoi'"
check "uninstall: a clean machine exits 10 and says nothing is installed" test "$(un_rc --uninstall --yes)" = 10
check "uninstall: that message says nothing was found" grep -q "nothing to uninstall: no Domovoi installation was found" "$work/err"
check "uninstall: a clean machine json result is a failure with exit 10" sh -c "PATH='$stubs:'\"\$PATH\" DOMOVOI_INSTALL_LOG='$work/install.log' sh '$installer' --uninstall --yes --json 2>/dev/null | tail -n 1 | grep -q '\"ok\":false,\"exit_code\":10'"
rm -f /etc/domovoi.sentinel /var/lib/domovoi.sentinel /etc/systemd/system/my-own.service
rmdir /etc/systemd/system/my-own.service.d

# Arguments that need root to see their effect.
check "uninstall: --purge without --uninstall exits 2" test "$(un_rc --role hub --purge)" = 2
check "uninstall: --purge without --uninstall says what it goes with" grep -q -- "--purge goes with --uninstall" "$work/err"
if ! sh -c ': </dev/tty' 2>/dev/null; then
    wipe_machine
    check "uninstall: a machine for the purge confirmation installs" test "$(un_rc --version v9.9.5 --role hub --bind loopback --no-start)" = 0
    check "uninstall: --purge without --yes and without a terminal exits 2" test "$(un_rc --uninstall --purge)" = 2
    check "uninstall: that refusal says to pass --yes" grep -q "pass --yes" "$work/err"
    check "uninstall: the refused purge deleted nothing" sh -c "test -d /etc/domovoi && test -x /usr/local/bin/domovoi && id domovoi >/dev/null"
fi

# A custom prefix: that prefix's binary and examples go, and the default prefix is not looked at.
wipe_machine
check "uninstall: a hub installs under a custom prefix" test "$(un_rc --version v9.9.5 --role hub --bind loopback --no-start --prefix /opt/dt-alt)" = 0
check "uninstall: the custom binary is there" test -x /opt/dt-alt/bin/domovoi
check "uninstall: the custom prefix dry run names its binary" sh -c "sh '$installer' --uninstall --dry-run --prefix /opt/dt-alt 2>&1 | grep -q '/opt/dt-alt/bin/domovoi'"
check "uninstall: the custom prefix uninstall exits 0" test "$(un_rc --uninstall --yes --prefix /opt/dt-alt)" = 0
check "uninstall: the custom binary is gone" test ! -e /opt/dt-alt/bin/domovoi
check "uninstall: the custom examples are gone" test ! -e /opt/dt-alt/share/domovoi
check "uninstall: the hub unit is gone" test ! -e /etc/systemd/system/domovoi-hub.service
check "uninstall: the data stays" test -d /var/lib/domovoi
rm -rf /opt/dt-alt

# A fresh install that fails before the first service starts is rolled back, accounts included.
wipe_machine
printf '%s\n' "tiny-pw" >"$adminpw"
chmod 0600 "$adminpw"
check "rollback: a hub with a rejected admin password exits 40" \
    test "$(un_rc --version v9.9.2 --role hub --bind loopback --admin-password-file "$adminpw")" = 40
check "rollback: it says it rolled back" grep -q "^Rolled back:" "$work/out"
check "rollback: the binary is gone" test ! -e /usr/local/bin/domovoi
check "rollback: the examples are gone" test ! -e /usr/local/share/domovoi
check "rollback: the user and its group are gone" sh -c "! id domovoi >/dev/null 2>&1 && ! getent group domovoi >/dev/null"
check "rollback: the directories are gone" sh -c "test ! -e /etc/domovoi && test ! -e /var/lib/domovoi"
check "rollback: the unit is gone" test ! -e /etc/systemd/system/domovoi-hub.service
check "rollback: nothing was started" sh -c "! grep -qE 'enable --now|start' '$work/systemctl.log'"
check "rollback: the list names the user, the group and the directories" sh -c "grep -qx '  user domovoi' '$work/out' && grep -qx '  group domovoi' '$work/out' && grep -qx '  /var/lib/domovoi' '$work/out'"
check "rollback: a clean machine is left, so an uninstall finds nothing" test "$(un_rc --uninstall --yes)" = 10
# --keep-on-failure leaves it all for debugging.
wipe_machine
touch "$work/daemon-fails"
check "rollback: a hub whose units cannot be loaded exits 40" test "$(un_rc --version v9.9.2 --role hub --bind loopback)" = 40
check "rollback: the unit that was placed is removed again" test ! -e /etc/systemd/system/domovoi-hub.service
check "rollback: that run removed the user and the directories too" \
    sh -c "grep -q '^Rolled back:' '$work/out' && ! id domovoi >/dev/null 2>&1 && test ! -e /etc/domovoi && test ! -e /var/lib/domovoi && test ! -e /usr/local/bin/domovoi"
check "rollback: the list names the unit" grep -qx "  /etc/systemd/system/domovoi-hub.service" "$work/out"
wipe_machine
check "rollback: --keep-on-failure still exits 40" \
    test "$(un_rc --version v9.9.2 --role hub --bind loopback --keep-on-failure)" = 40
check "rollback: --keep-on-failure keeps the user, the directories and the unit" \
    sh -c "id domovoi >/dev/null && test -d /etc/domovoi && test -d /var/lib/domovoi && test -e /etc/systemd/system/domovoi-hub.service && test -x /usr/local/bin/domovoi"
check "rollback: --keep-on-failure says it did not undo" grep -q "Left in place (--keep-on-failure)" "$work/out"
rm -f "$work/daemon-fails"
# A hub that never answers after its first start is not undone: the run is past the point of undoing.
wipe_machine
rm -f "$work/ready"
DOMOVOI_READY_TIMEOUT=2
export DOMOVOI_READY_TIMEOUT
check "rollback: a hub that never becomes ready exits 40" test "$(un_rc --version v9.9.2 --role hub --bind loopback)" = 40
unset DOMOVOI_READY_TIMEOUT
check "rollback: a failure after the first start is not rolled back" sh -c "! grep -q 'Rolled back' '$work/out' && id domovoi >/dev/null && test -x /usr/local/bin/domovoi"
rm -f "$adminpw"
wipe_machine
mkdir -p /var/lib/domovoi

hub_reset
rm -f /etc/systemd/system/domovoi-hub.service
if [ "$made_systemd_dir" -eq 1 ]; then rmdir /run/systemd/system /run/systemd 2>/dev/null || true; fi

# Without root a user prefix works with --no-service, and is refused without it.
if id nobody >/dev/null 2>&1 && command -v su >/dev/null 2>&1; then
    mkdir -p "$work/user" && chown nobody "$work/user"
    check "unprivileged --no-service install succeeds" \
        su -s /bin/sh nobody -c "DOMOVOI_RELEASE_BASE=$base sh '$installer' --role agent --no-service --prefix '$work/user' >/dev/null 2>&1"
    check "unprivileged binary installed" test -x "$work/user/bin/domovoi"
    if su -s /bin/sh nobody -c "DOMOVOI_RELEASE_BASE=$base sh '$installer' --role agent --prefix '$work/user' >/dev/null 2>&1"; then
        fail "unprivileged install with a service is refused"
    else
        pass "unprivileged install with a service is refused"
    fi
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "all installer checks passed"
