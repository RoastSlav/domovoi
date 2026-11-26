#!/bin/sh
# Tests install.sh against a fake release served from a local web server. Run it as root
# in a throwaway container or VM: it creates the domovoi user, /etc/domovoi,
# /var/lib/domovoi* and the systemd units.
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
check "the legacy agent unit keeps running as root" \
    sh -c "! sh '$installer' --print-unit agent-legacy | grep -Eq '^(User|Group)='"
check "an unknown unit name is refused" \
    sh -c "! sh '$installer' --print-unit runner >/dev/null 2>&1"

# The runner steps need systemd, so they are tested through the functions of install.sh, taken
# out of the script as written, with stubs for the binary and for systemctl.
fns="$work/install-functions.sh"
grep '^\(say\|log_line\|show_file\|show_file_err\|fail\)() {' "$installer" >"$fns" # one-line functions
for fn in err fail_with failed_checks runner_service_unit render_runner_service runner_prefix_ok \
    runner_dirs place_unit start_sockets; do
    sed -n "/^$fn() {/,/^}/p" "$installer" >>"$fns"
done
check "the runner functions were extracted" test "$(grep -c '^[a-z_]*() {' "$fns")" -eq 14

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
"agent selftest --installer")
    [ "${STUB_FAIL:-none}" != second ] || { echo "ST-13 fail runner_probe"; exit 1; }
    echo "ST-13 pass"
    ;;
esac
STUB
chmod 0755 "$stub/domovoi"
sockets_run() {
    : >"$work/systemctl.log"
    sh -c ". '$fns'; systemctl() { echo \"\$*\" >>'$work/systemctl.log'; }; bin='$stub/domovoi'; tmp='$work'; $1" \
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

# An edited runner unit is kept; the new text is written beside it.
mkdir -p /etc/systemd/system
echo "# edited" >/etc/systemd/system/domovoi-skill-runner@.service
sh -c ". '$fns'; runner_skills_dir=/etc/domovoi/skills; runner_packages_dir=/var/lib/domovoi-packages; bin=/usr/local/bin/domovoi; prefix='$work/prefix'; tmp='$work'; record_dir=/nonexistent; place_unit domovoi-skill-runner@.service render_runner_service 0" \
    >"$work/place.out" 2>&1 || true
check "an edited runner unit is kept" test "$(cat /etc/systemd/system/domovoi-skill-runner@.service)" = "# edited"
check "the kept runner unit is reported" grep -q "Kept /etc/systemd/system/domovoi-skill-runner@.service" "$work/place.out"
check "the new runner unit is saved beside it" \
    cmp -s "$work/prefix/share/domovoi/domovoi-skill-runner@.service" "$root/deploy/systemd/domovoi-skill-runner@.service"
rm -f /etc/systemd/system/domovoi-skill-runner@.service

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
    sh -c "sh '$installer' --print-unit agent-legacy | cmp -s - /etc/systemd/system/domovoi-agent.service"
check "agent next steps printed" grep -q "Add host" "$work/out"
# With no /run/systemd/system the agent keeps today's layout: no accounts, no helper, today's modes.
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

# Running again changes nothing but the binary; edits are kept.
echo "# edited" >>/etc/systemd/system/domovoi-hub.service
echo '[hub]' >/etc/domovoi/hub.toml
chmod 0700 /var/lib/domovoi
edited="$(cat /etc/systemd/system/domovoi-hub.service)"
check "second hub install succeeds" run_install --role hub
check "edited unit kept" test "$(cat /etc/systemd/system/domovoi-hub.service)" = "$edited"
check "kept unit reported" grep -q "Kept /etc/systemd/system/domovoi-hub.service" "$work/out"
check "new unit saved beside it" test -f /usr/local/share/domovoi/domovoi-hub.service
check "hub.toml kept" test "$(cat /etc/domovoi/hub.toml)" = "[hub]"
check "upgrade suggests restart" grep -q "systemctl restart domovoi-hub" "$work/out"
check "upgrade does not suggest enable --now" test -z "$(grep "enable --now" "$work/out")"
check "changed mode kept" test "$(mode_of /var/lib/domovoi)" = 700
check "unchanged unit reported up to date" sh -c "sh '$installer' --role agent 2>&1 | grep -q 'domovoi-agent.service is up to date'"


# --json on a real run, over an install that already exists.
sh "$installer" --role hub --json >"$work/json.out" 2>/dev/null
check "json install is only JSON lines ending in one result" check_json "$work/json.out"
check "json result names the binary" sh -c "grep -q '\"binary\":\"/usr/local/bin/domovoi\"' '$work/json.out'"

# The identity migration and its rollback need systemd and say so before touching anything.
if run_install --role agent --migrate-identity; then
    fail "migrate-identity is refused without systemd"
else
    pass "migrate-identity is refused without systemd"
fi
check "migrate-identity explains it needs systemd" grep -q "needs a systemd host" "$work/out"
check "migrate-identity changed nothing" test "$(mode_of /var/lib/domovoi-agent)" = 700
if run_install --rollback-identity; then
    fail "rollback-identity is refused without systemd"
else
    pass "rollback-identity is refused without systemd"
fi
# --yes never prompts, and it is not a way past the typed word of --migrate-identity.
if run_install --role agent --yes --migrate-identity; then
    fail "--yes does not bypass the migration's confirmation"
else
    pass "--yes does not bypass the migration's confirmation"
fi

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
