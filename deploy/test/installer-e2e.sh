#!/bin/sh
# End-to-end test of install.sh for the hub on a real systemd machine, with a REAL domovoi
# binary. It is not part of CI. Run it as root on a throwaway machine (a VM or a fresh
# container with systemd as PID 1), because it creates the domovoi user, /etc/domovoi,
# /var/lib/domovoi and the hub unit, starts the hub on 127.0.0.1:7420, and leaves them there.
#
# What it does:
#   1. serves a fake release that holds the binary named by $E2E_BINARY;
#   2. runs install.sh --role hub --bind loopback --admin-password-file FILE --json;
#   3. checks the exit code and the JSON result (started, admin_created, the web URL, no setup
#      URL), that the unit is active and that GET /readyz answers;
#   4. logs in as admin with POST /api/v1/auth/login (with an Origin header), and checks that a
#      wrong password is refused;
#   5. checks that the password is nowhere in the install log;
#   6. runs the installer again (an upgrade) and checks it keeps the admin and the hub running.
#
# With E2E_BOTH=1 it then runs a second section on top of that hub, for the machine that runs
# the hub and should be watched by it:
#   7. runs install.sh --role both --name e2e-host --json: the hub is restarted, stopped for
#      the token, started again, and the agent is enrolled, started and connected;
#   8. checks the result (enrolled_host, agent_started), both units, `agent status`, and after
#      a login that GET /api/v1/hosts lists the host; the token is in no log;
#   9. runs --role both again (an upgrade): nothing is enrolled a second time, both services
#      are restarted and the host is still listed and connected.
#
#   E2E_BINARY=/path/to/target/release/domovoi sudo -E deploy/test/installer-e2e.sh
#   E2E_BOTH=1 E2E_BINARY=... sudo -E deploy/test/installer-e2e.sh    # adds the --role both section
#
# Needs: curl, python3, tar, sha256sum, systemd. The binary must run on this machine.
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

[ "$(id -u)" -eq 0 ] || {
    echo "run this as root, on a machine you can throw away" >&2
    exit 2
}
[ -d /run/systemd/system ] || {
    echo "this machine is not running systemd" >&2
    exit 2
}
[ -n "${E2E_BINARY:-}" ] && [ -x "$E2E_BINARY" ] || {
    echo "set E2E_BINARY to a built domovoi binary (cargo build --release -p domovoi)" >&2
    exit 2
}
"$E2E_BINARY" version >/dev/null 2>&1 || {
    echo "$E2E_BINARY does not run on this machine" >&2
    exit 2
}
for path in /usr/local/bin/domovoi /etc/domovoi /var/lib/domovoi /etc/systemd/system/domovoi-hub.service; do
    [ ! -e "$path" ] || {
        echo "$path exists; run this on a clean machine" >&2
        exit 2
    }
done
! id domovoi >/dev/null 2>&1 || {
    echo "the domovoi user exists; run this on a clean machine" >&2
    exit 2
}
for tool in curl python3 tar sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is needed" >&2
        exit 2
    }
done

case "$(uname -m)" in
x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
*)
    echo "unsupported machine $(uname -m)" >&2
    exit 2
    ;;
esac

# The fake release: download/<tag>/<asset> and its .sha256. Its tag is the binary's own version,
# so the installer sees the same version it installed and a re-run is an upgrade, not a downgrade.
tag="v$("$E2E_BINARY" version | sed -n '1s/^domovoi //p')"
case "$tag" in
v[0-9]*.[0-9]*.[0-9]*) ;;
*)
    echo "cannot read a version from: $E2E_BINARY version" >&2
    exit 2
    ;;
esac
name="domovoi-$tag-$target"
dir="$work/srv/download/$tag"
mkdir -p "$dir" "$work/build/$name"
cp "$E2E_BINARY" "$work/build/$name/domovoi"
chmod 0755 "$work/build/$name/domovoi"
tar -C "$work/build" -czf "$dir/$name.tar.gz" "$name"
(cd "$dir" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")

port=18735
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$work/srv" >/dev/null 2>&1 &
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
install_log="$work/install.log"
export DOMOVOI_INSTALL_LOG="$install_log"

password="e2e-$(head -c 12 /dev/urandom | od -An -tx1 | tr -d ' \n')"
pwfile="$work/admin.pw"
(umask 077 && printf '%s\n' "$password" >"$pwfile")
(umask 077 && printf '{"username":"admin","password":"%s"}' "$password" >"$work/login.json")
(umask 077 && printf '{"username":"admin","password":"wrong-password-here"}' >"$work/login-bad.json")

result_field() { # FILE EXPRESSION on r, the last JSON line
    python3 -I -c '
import json, sys
r = [json.loads(l) for l in open(sys.argv[1]).read().splitlines()][-1]
print(eval(sys.argv[2]))
' "$1" "$2"
}

rc=0
sh "$installer" --role hub --version "$tag" --bind loopback --admin-password-file "$pwfile" --json \
    >"$work/result.json" 2>"$work/install.err" || rc=$?
check "the installer exits 0" test "$rc" -eq 0
check "the result is ok" test "$(result_field "$work/result.json" 'r["ok"]')" = "True"
check "the hub was started" test "$(result_field "$work/result.json" 'r["started"]')" = "True"
check "the admin was created" test "$(result_field "$work/result.json" 'r["admin_created"]')" = "True"
check "the web URL is loopback" test "$(result_field "$work/result.json" 'r["urls"]["web"]')" = "http://127.0.0.1:7420"
check "there is no setup URL" test "$(result_field "$work/result.json" '"setup" in r["urls"]')" = "False"
check "the unit is active" systemctl is-active --quiet domovoi-hub
check "the unit is enabled" systemctl is-enabled --quiet domovoi-hub
check "/readyz answers 200" test "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:7420/readyz)" = 200

login() { # BODYFILE: prints the HTTP status
    curl -s -o "$work/login.out" -D "$work/login.hdr" -w '%{http_code}' -X POST \
        -H 'Origin: http://127.0.0.1:7420' -H 'Content-Type: application/json' \
        --data-binary "@$1" http://127.0.0.1:7420/api/v1/auth/login
}
check "the admin can log in" test "$(login "$work/login.json")" = 200
check "the login sets a session cookie" grep -qi '^set-cookie:' "$work/login.hdr"
check "a wrong password is refused" test "$(login "$work/login-bad.json")" = 401
check "the password is not in the install log" sh -c "! grep -q '$password' '$install_log'"
check "the password is not in the installer error output" sh -c "! grep -q '$password' '$work/install.err'"

# An upgrade keeps the admin and the running hub.
rc=0
sh "$installer" --role hub --version "$tag" --json >"$work/result2.json" 2>"$work/install2.err" || rc=$?
check "a second run exits 0" test "$rc" -eq 0
if [ "$rc" -ne 0 ]; then
    echo "the second run exited $rc; its result and its error output follow"
    cat "$work/result2.json" "$work/install2.err"
fi
check "a second run restarts and starts the hub" test "$(result_field "$work/result2.json" 'r["started"]')" = "True"
check "a second run creates no admin" test "$(result_field "$work/result2.json" 'r["admin_created"]')" = "False"
check "the unit is still active" systemctl is-active --quiet domovoi-hub
check "the admin can still log in" test "$(login "$work/login.json")" = 200

# --role both: the agent of the machine the hub runs on (opt-in: E2E_BOTH=1).
if [ "${E2E_BOTH:-0}" = 1 ]; then
    host="e2e-host"
    cookies="$work/cookies"
    login_cookie() { # logs in as admin and keeps the session cookie; prints the HTTP status
        curl -s -o /dev/null -w '%{http_code}' -c "$cookies" -X POST \
            -H 'Origin: http://127.0.0.1:7420' -H 'Content-Type: application/json' \
            --data-binary "@$work/login.json" http://127.0.0.1:7420/api/v1/auth/login
    }
    agent_connected() { "/usr/local/bin/domovoi" agent status 2>&1 | grep -q '^connected since'; }
    wait_connected() { # up to 30 s: the link may take a moment after a restart
        wc_i=0
        until agent_connected; do
            wc_i=$((wc_i + 1))
            [ "$wc_i" -lt 30 ] || return 1
            sleep 1
        done
    }
    rc=0
    sh "$installer" --role both --version "$tag" --name "$host" --json >"$work/result3.json" 2>"$work/install3.err" || rc=$?
    check "both: the installer exits 0" test "$rc" -eq 0
    if [ "$rc" -ne 0 ]; then
        echo "the --role both run exited $rc; its result and its error output follow"
        cat "$work/result3.json" "$work/install3.err"
    fi
    check "both: the result is ok" test "$(result_field "$work/result3.json" 'r["ok"]')" = "True"
    check "both: the role and layout are reported" test "$(result_field "$work/result3.json" '(r["role"], r["layout"])')" = "('both', 'hub')"
    check "both: the hub was started" test "$(result_field "$work/result3.json" 'r["started"]')" = "True"
    check "both: the agent was started" test "$(result_field "$work/result3.json" 'r["agent_started"]')" = "True"
    check "both: the enrolled host is reported" test "$(result_field "$work/result3.json" 'r["enrolled_host"]')" = "$host"
    check "both: the hub unit is active" systemctl is-active --quiet domovoi-hub
    check "both: the agent unit is active" systemctl is-active --quiet domovoi-agent
    check "both: the agent unit is enabled" systemctl is-enabled --quiet domovoi-agent
    check "both: /readyz answers 200" test "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:7420/readyz)" = 200
    check "both: the agent reports a connected link" wait_connected
    check "both: the admin can log in" test "$(login_cookie)" = 200
    check "both: the host is listed by GET /api/v1/hosts" sh -c "curl -s -b '$cookies' http://127.0.0.1:7420/api/v1/hosts | grep -q '$host'"
    check "both: no enrollment token is in the install log" sh -c "! grep -Eq -- '--token +[A-Za-z0-9_-]{16,}' '$install_log' '$work/result3.json' '$work/install3.err'"
    check "both: no temporary token file is left behind" sh -c "! find /tmp -name enroll.tok -o -name token.out 2>/dev/null | grep -q ."
    # A second run is an upgrade: nothing is enrolled again.
    rc=0
    sh "$installer" --role both --version "$tag" --json >"$work/result4.json" 2>"$work/install4.err" || rc=$?
    check "both: a second run exits 0" test "$rc" -eq 0
    if [ "$rc" -ne 0 ]; then
        echo "the second --role both run exited $rc; its result and its error output follow"
        cat "$work/result4.json" "$work/install4.err"
    fi
    check "both: a second run restarts the hub" test "$(result_field "$work/result4.json" 'r["started"]')" = "True"
    check "both: a second run enrolls nothing" test "$(result_field "$work/result4.json" '(r["enrolled_host"], r["agent_started"])')" = "('', False)"
    check "both: both units are still active" sh -c "systemctl is-active --quiet domovoi-hub && systemctl is-active --quiet domovoi-agent"
    check "both: the agent connects again" wait_connected
    check "both: the admin can log in again" test "$(login_cookie)" = 200
    check "both: the host is still listed" sh -c "curl -s -b '$cookies' http://127.0.0.1:7420/api/v1/hosts | grep -q '$host'"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    echo "the hub log: journalctl -u domovoi-hub -n 50 --no-pager; the agent: journalctl -u domovoi-agent -n 50 --no-pager"
    exit 1
fi
echo "all end-to-end checks passed"
