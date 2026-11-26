#!/bin/sh
# Tests the parts of install.sh that need no root, systemd or Docker: the preflight, --dry-run,
# --json, the input checks, the exit codes and the downgrade rule. It serves a fake release
# from a local web server and installs with --no-service into a temporary prefix as the
# current user, so it runs on a developer machine. Needs curl and python3.
#
#   deploy/test/installer-user.sh
set -eu

root="$(cd "$(dirname "$0")/../.." && pwd)"
installer="$root/install.sh"
work="$(mktemp -d)"
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
# Runs the installer with the given arguments; sets $rc to its exit status and keeps its
# output in $work/out (stdout) and $work/err (stderr).
run() {
    rc=0
    sh "$installer" "$@" >"$work/out" 2>"$work/err" || rc=$?
}
expect_rc() { # DESCRIPTION CODE ARGS...
    er_desc="$1"
    er_want="$2"
    shift 2
    run "$@"
    if [ "$rc" -eq "$er_want" ]; then pass "$er_desc"; else fail "$er_desc (exit $rc, wanted $er_want)"; fi
}

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
    name="domovoi-$tag-$target"
    dir="$work/srv/download/$tag"
    mkdir -p "$dir" "$work/build/$name/examples"
    printf '#!/bin/sh\necho "domovoi %s (test build)"\n' "$tag" >"$work/build/$name/domovoi"
    chmod 0755 "$work/build/$name/domovoi"
    echo "# example" >"$work/build/$name/examples/hub.toml"
    tar -C "$work/build" -czf "$dir/$name.tar.gz" "$name"
    (cd "$dir" && sha256sum "$name.tar.gz" >"$name.tar.gz.sha256")
}
release v9.9.0
release v9.9.1

port=18734
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
export DOMOVOI_INSTALL_LOG="$work/install.log"
unset NO_COLOR DOMOVOI_ALLOW_HTTP DOMOVOI_NONINTERACTIVE DOMOVOI_MIN_FREE_MB

p="$work/prefix"

# The usage text.
run --help
check "--help exits 0" test "$rc" -eq 0
for word in "Exit codes:" "--dry-run" "--json" "--yes" "--quiet" "--verbose" "--force-downgrade" \
    "DOMOVOI_ALLOW_HTTP" "DOMOVOI_NONINTERACTIVE" "NO_COLOR"; do
    check "usage lists $word" grep -qF -- "$word" "$work/out"
done
for code in 2 10 20 30 40 50; do
    check "usage documents exit code $code" grep -Eq "^  $code  " "$work/out"
done

# --dry-run changes nothing, downloads nothing, and prints the plan.
expect_rc "dry run of a named version exits 0 with no network" 0 \
    --role agent --no-service --prefix "$p" --dry-run --version v9.9.0
check "dry run did not create the prefix" test ! -e "$p"
check "dry run prints the preflight table" grep -q "^Preflight:" "$work/out"
check "dry run prints the plan with the version" grep -q "create   $p/bin/domovoi (domovoi v9.9.0)" "$work/out"
check "dry run says nothing was changed" grep -q "nothing was changed" "$work/out"
expect_rc "dry run can resolve latest" 0 --role agent --no-service --prefix "$p" --dry-run
check "dry run resolved latest to v9.9.1" grep -q "domovoi v9.9.1" "$work/out"

# A real install, then the state rules.
expect_rc "install of v9.9.1 succeeds" 0 --role agent --no-service --prefix "$p" --version v9.9.1
check "binary installed" test -x "$p/bin/domovoi"
check "install prints a numbered step" grep -q "^\[2/4\] Downloading" "$work/out"
check "install keeps the Installed line" grep -q "^Installed domovoi v9.9.1 (test build) to $p/bin/domovoi" "$work/out"
check "install prints the log location" grep -q "^Log file: $DOMOVOI_INSTALL_LOG" "$work/out"
check "the log holds the installer text" grep -q "Checksum verified" "$DOMOVOI_INSTALL_LOG"
expect_rc "installing the same version again succeeds" 0 --role agent --no-service --prefix "$p" --version v9.9.1
check "the same version is called an upgrade" grep -q "upgrade: v\{0,1\}9.9.1 is installed" "$work/out"
expect_rc "a downgrade is refused without --force-downgrade" 10 --role agent --no-service --prefix "$p" --version v9.9.0
check "the downgrade refusal names the flag" grep -q "force-downgrade" "$work/err"
check "the refused downgrade left v9.9.1" sh -c "'$p/bin/domovoi' version | grep -q v9.9.1"
expect_rc "a downgrade works with --force-downgrade" 0 --role agent --no-service --prefix "$p" --version v9.9.0 --force-downgrade
check "the forced downgrade installed v9.9.0" sh -c "'$p/bin/domovoi' version | grep -q v9.9.0"
expect_rc "an upgrade to latest works" 0 --role agent --no-service --prefix "$p"
check "the upgrade installed v9.9.1" sh -c "'$p/bin/domovoi' version | grep -q v9.9.1"

# The inputs are checked before anything else happens.
fresh="$work/fresh"
expect_rc "a version that is not a tag is refused" 2 --role agent --no-service --prefix "$fresh" --version "v1;rm"
expect_rc "a version with a missing part is refused" 2 --role agent --no-service --prefix "$fresh" --version 1.2
expect_rc "a bare number is a good version" 0 --role agent --no-service --prefix "$fresh" --version 9.9.1 --dry-run
expect_rc "a pre-release tag is a good version" 0 --role agent --no-service --prefix "$fresh" --version v1.0.0-rc.1 --dry-run
expect_rc "a relative prefix is refused" 2 --role agent --no-service --prefix relative/dir
expect_rc "an unknown flag is refused" 2 --role agent --frobnicate
expect_rc "a missing role is refused" 2 --no-service
expect_rc "--quiet with --verbose is refused" 2 --role agent --no-service --prefix "$p" --quiet --verbose
DOMOVOI_RELEASE_BASE="http://example.invalid/releases"
export DOMOVOI_RELEASE_BASE
expect_rc "a plain-http base that is not loopback is refused" 2 --role agent --no-service --prefix "$fresh" --dry-run --version v1.0.0
check "the http refusal names DOMOVOI_ALLOW_HTTP" grep -q "DOMOVOI_ALLOW_HTTP" "$work/err"
DOMOVOI_ALLOW_HTTP=1 expect_rc "DOMOVOI_ALLOW_HTTP=1 allows a plain-http base" 0 --role agent --no-service --prefix "$fresh" --dry-run --version v1.0.0
DOMOVOI_RELEASE_BASE="http://127.0.0.1.example.invalid/releases"
expect_rc "a host that only starts like loopback is refused" 2 --role agent --no-service --prefix "$fresh" --dry-run --version v1.0.0
DOMOVOI_RELEASE_BASE="http://localhost:1/x"
expect_rc "a localhost base needs no opt-in" 0 --role agent --no-service --prefix "$fresh" --dry-run --version v1.0.0
DOMOVOI_RELEASE_BASE="http://localhost@evil.example/x"
expect_rc "a base with a user name is refused" 2 --role agent --no-service --prefix "$fresh" --dry-run --version v1.0.0
DOMOVOI_RELEASE_BASE="ftp://example.invalid/x"
expect_rc "a base that is not http or https is refused" 2 --role agent --no-service --prefix "$fresh" --dry-run --version v1.0.0
DOMOVOI_RELEASE_BASE="$base"
export DOMOVOI_RELEASE_BASE

# Preflight failures use exit code 10, downloads 20.
if [ "$(id -u)" -ne 0 ]; then
    expect_rc "a service install without root fails preflight" 10 --role agent --prefix "$fresh"
    check "the preflight failure shows the table" grep -q "FAIL\|fail  root" "$work/out"
fi
DOMOVOI_MIN_FREE_MB=999999999 expect_rc "too little free space fails preflight" 10 --role agent --no-service --prefix "$p" --version v9.9.1
check "no prefix was created by a refused run" test ! -e "$fresh"
echo "0000000000000000000000000000000000000000000000000000000000000000  domovoi-v9.9.0-$target.tar.gz" \
    >"$work/srv/download/v9.9.0/domovoi-v9.9.0-$target.tar.gz.sha256"
expect_rc "a wrong checksum exits 20" 20 --role agent --no-service --prefix "$fresh" --version v9.9.0
check "a wrong checksum is explained" grep -q "does not match" "$work/err"
check "a wrong checksum installed nothing" test ! -e "$fresh/bin/domovoi"
expect_rc "a missing release exits 20" 20 --role agent --no-service --prefix "$fresh" --version v9.9.9

# --quiet prints the summary only.
expect_rc "a quiet install succeeds" 0 --role agent --no-service --prefix "$p" --version v9.9.1 --quiet
check "a quiet install prints no step lines" sh -c "! grep -q '^\[' '$work/out'"
check "a quiet install prints the summary" grep -q "^Done: domovoi v9.9.1" "$work/out"
check "a quiet install is still logged" grep -q "Installed domovoi v9.9.1" "$DOMOVOI_INSTALL_LOG"
expect_rc "a verbose install succeeds" 0 --role agent --no-service --prefix "$p" --version v9.9.1 --verbose
check "text has no colour codes off a terminal" sh -c "! grep -q \"\$(printf '\\033')\" '$work/out'"
expect_rc "--yes and DOMOVOI_NONINTERACTIVE are accepted" 0 --role agent --no-service --prefix "$p" --version v9.9.1 --yes
DOMOVOI_NONINTERACTIVE=1 expect_rc "DOMOVOI_NONINTERACTIVE=1 is accepted" 0 --role agent --no-service --prefix "$p" --version v9.9.1
expect_rc "--yes does not replace the typed word of --migrate-identity" 2 --role agent --no-service --prefix "$p" --yes --migrate-identity

# --json: only JSON on stdout, one result at the end.
check_json() { # FILE: every line is JSON, the last is the one result
    python3 -I -c '
import json, sys
lines = open(sys.argv[1]).read().splitlines()
assert lines, "no output"
objs = [json.loads(l) for l in lines]
assert all(o["event"] in ("step", "result") for o in objs)
assert [o["event"] for o in objs].count("result") == 1 and objs[-1]["event"] == "result"
for o in objs[:-1]:
    assert set(("n", "total", "name", "status", "message")) <= set(o), o
    assert o["status"] in ("start", "ok", "warn", "fail"), o
r = objs[-1]
assert isinstance(r["ok"], bool) and isinstance(r["exit_code"], int)
assert set(("role", "version", "layout", "paths", "next_steps")) <= set(r)
assert set(("binary", "log")) <= set(r["paths"])
' "$1"
}
result_field() { # FILE EXPRESSION on r
    python3 -I -c '
import json, sys
r = [json.loads(l) for l in open(sys.argv[1]).read().splitlines()][-1]
print(eval(sys.argv[2]))
' "$1" "$2"
}
expect_rc "a json install succeeds" 0 --role agent --no-service --prefix "$p" --version v9.9.1 --json
check "json output is only JSON lines with one result" check_json "$work/out"
check "the json result says ok" test "$(result_field "$work/out" 'r["ok"]')" = "True"
check "the json result carries the binary path" test "$(result_field "$work/out" 'r["paths"]["binary"]')" = "$p/bin/domovoi"
check "the json result carries the version" test "$(result_field "$work/out" 'r["version"]')" = "v9.9.1"
check "the json result carries the log path" test "$(result_field "$work/out" 'r["paths"]["log"]')" = "$DOMOVOI_INSTALL_LOG"
check "json mode keeps the text in the log" grep -q "Checksum verified" "$DOMOVOI_INSTALL_LOG"
check "json mode prints no human text" sh -c "! grep -q 'Checksum verified' '$work/out'"
expect_rc "a json dry run succeeds" 0 --role agent --no-service --prefix "$p" --version v9.9.1 --json --dry-run
check "the json dry run is only JSON lines" check_json "$work/out"
check "the json dry run lists its plan" test "$(result_field "$work/out" 'len(r["plan"]) > 3')" = "True"
expect_rc "a json failure exits with its code" 2 --role agent --no-service --prefix "$p" --version nonsense --json
check "the json failure is only JSON lines" check_json "$work/out"
check "the json failure says not ok with code 2" test "$(result_field "$work/out" '(r["ok"], r["exit_code"])')" = "(False, 2)"
check "the json failure carries the message" test "$(result_field "$work/out" '"not a release tag" in r["error"]')" = "True"
expect_rc "a json download failure exits 20" 20 --role agent --no-service --prefix "$p" --version v9.9.9 --json
check "the json download failure is only JSON lines" check_json "$work/out"
check "the json download failure ends the open step as fail" test "$(result_field "$work/out" '[json.loads(l) for l in open("'"$work"'/out").read().splitlines()][-2]["status"]')" = "fail"

# The helpers, taken out of the script as written.
fns="$work/functions.sh"
for fn in json_escape lines_to_json ver_cmp port_state; do
    sed -n "/^$fn() {/,/^}/p" "$installer" >>"$fns"
done
check "json_escape produces valid JSON for quotes, backslashes and newlines" \
    sh -c ". '$fns'; printf '{\"v\":\"%s\"}' \"\$(json_escape \"a\\\"b\\\\c
d	e\")\" | python3 -I -c 'import json,sys; v=json.load(sys.stdin)[\"v\"]; assert v == \"a\\\"b\\\\c\\nd e\", repr(v)'"
check "lines_to_json makes an array" \
    test "$(printf 'a\nb "q"\n' | sh -c ". '$fns'; lines_to_json")" = '["a","b \"q\""]'
for pair in "v1.2.3:v1.2.4:lt" "v1.10.0:v1.9.9:gt" "1.2.3:v1.2.3:eq" "v2.0.0-rc.1:v1.9.9:gt" "v1.2.3:v1.2.3.1:lt"; do
    a="${pair%%:*}"
    rest="${pair#*:}"
    b="${rest%%:*}"
    want="${rest##*:}"
    check "ver_cmp $a $b is $want" test "$(sh -c ". '$fns'; ver_cmp '$a' '$b'")" = "$want"
done
listener="$work/listener.py"
cat >"$listener" <<'PY'
import socket, sys, time
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", 0))
s.listen(1)
print(s.getsockname()[1], flush=True)
time.sleep(30)
PY
python3 "$listener" >"$work/listen.port" &
listen_pid=$!
j=0
until [ -s "$work/listen.port" ]; do
    j=$((j + 1))
    [ "$j" -lt 50 ] || break
    sleep 0.1
done
busy="$(cat "$work/listen.port")"
check "port_state sees a listening port" test "$(sh -c ". '$fns'; port_state '$busy'")" = "0"
kill "$listen_pid" 2>/dev/null || true
wait "$listen_pid" 2>/dev/null || true
check "port_state sees a free port" test "$(sh -c ". '$fns'; port_state '$busy'")" = "1"

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "all installer checks passed"
