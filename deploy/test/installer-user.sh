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

# --- Hub settings: URLs, hub.env, admin password, start and readiness --------------------
# Flag checks happen before the preflight, so they run without root.
expect_rc "--public-url with --role agent is refused" 2 --role agent --public-url https://h.example
expect_rc "--bind with --no-service is refused" 2 --role hub --no-service --prefix "$fresh" --bind lan
expect_rc "an ftp public URL is refused" 2 --role hub --public-url ftp://h.example
expect_rc "a public URL with a path is refused" 2 --role hub --public-url http://h.example/x
expect_rc "a public URL with a space is refused" 2 --role hub --public-url "http://h.example x"
expect_rc "an https agent URL is refused" 2 --role hub --agent-url https://h.example:7421
expect_rc "a bind that is not an address is refused" 2 --role hub --bind nope
expect_rc "a bind with a port out of range is refused" 2 --role hub --bind 10.0.0.5:99999
check "a bad DOMOVOI_READY_TIMEOUT is refused with exit 2" sh -c "DOMOVOI_READY_TIMEOUT=x sh '$installer' --role hub --dry-run >/dev/null 2>'$work/err'; test \$? -eq 2 && grep -q READY_TIMEOUT '$work/err'"
if [ "$(id -u)" -ne 0 ]; then
    expect_rc "--no-start is a known flag" 10 --role hub --no-start --public-url https://h.example
    expect_rc "a good public URL passes validation (then the preflight needs root)" 10 \
        --role hub --public-url https://domovoi.example --agent-url wss://domovoi.example:7421 --bind 10.0.0.5:7420
fi
check "usage lists the new flags" sh -c "sh '$installer' --help | grep -q -- '--public-url' && sh '$installer' --help | grep -q -- '--admin-password-file' && sh '$installer' --help | grep -q DOMOVOI_READY_TIMEOUT && sh '$installer' --help | grep -q -- '--no-start'"

# The admin password file: a regular file, mode 0600 or 0400, owned by the caller. The
# password is never in the log or the output.
secret="correct horse battery staple"
pwf="$work/admin.pw"
printf '%s\n' "$secret" >"$pwf"
chmod 0644 "$pwf"
expect_rc "a password file with mode 0644 is refused" 2 --role hub --admin-password-file "$pwf"
check "the mode refusal says how to fix it" grep -q "chmod 600" "$work/err"
ln -sf "$pwf" "$work/admin.link"
chmod 0600 "$pwf"
expect_rc "a link to a password file is refused" 2 --role hub --admin-password-file "$work/admin.link"
expect_rc "a directory as password file is refused" 2 --role hub --admin-password-file "$work"
expect_rc "a missing password file is refused" 2 --role hub --admin-password-file "$work/none"
expect_rc "a file and stdin together are refused" 2 --role hub --admin-password-file "$pwf" --admin-password-stdin
: >"$work/empty.pw"
chmod 0600 "$work/empty.pw"
expect_rc "an empty password file is refused" 2 --role hub --admin-password-file "$work/empty.pw"
expect_rc "the admin password goes with --role hub" 2 --role agent --admin-password-file "$pwf"
if [ "$(id -u)" -ne 0 ]; then
    expect_rc "a 0600 password file passes validation (then the preflight needs root)" 10 --role hub --admin-password-file "$pwf"
    chmod 0400 "$pwf"
    expect_rc "a 0400 password file passes validation" 10 --role hub --admin-password-file "$pwf"
    rc=0
    printf '%s\n' "$secret" | sh "$installer" --role hub --admin-password-stdin >"$work/out" 2>"$work/err" || rc=$?
    if [ "$rc" -eq 10 ]; then pass "a piped password passes validation"; else fail "a piped password passes validation (exit $rc)"; fi
fi
check "the password is not in the log" sh -c "! grep -q 'horse battery' '$DOMOVOI_INSTALL_LOG'"
check "the password is not in the output" sh -c "! grep -rq 'horse battery' '$work/out' '$work/err'"
chmod 0600 "$pwf"

# The functions that write hub.env, create the admin and start the hub, taken out of the
# script as written and run with stubs. sleep is a no-op, so nothing here waits.
hfns="$work/hub-functions.sh"
: >"$hfns"
for fn in say log_line say_always err warn fail_with fail show_file_err env_get env_set detect_ip url_host \
    check_url resolve_net prepare_admin_password write_hub_env create_admin local_web_url user_web_url \
    probe_ready redact_tokens wait_ready start_hub journal_setup_token find_setup_link; do
    if grep -q "^$fn() {.*}\$" "$installer"; then
        grep "^$fn() {.*}\$" "$installer" >>"$hfns"
    else
        sed -n "/^$fn() {/,/^}/p" "$installer" >>"$hfns"
    fi
done
check "the hub functions were extracted" test "$(grep -c '^[a-z_]*() {' "$hfns")" -eq 25
hubrun() { # SCRIPT: runs it with the extracted functions, no-op sleep and chown, and a temp dir
    mkdir -p "$work/ht"
    sh -c "set -eu; . '$hfns'; sleep() { :; }; chown() { :; }; tmp='$work/ht'; log_file='$work/hub.log'; hub_env='$work/hub.env'; quiet=0; json=0; upgrade=0; hub_fresh_data=1; admin_created=0; started=0; setup_url=''; public_url=''; bind_opt=''; agent_url=''; $1" \
        >"$work/hub.out" 2>"$work/hub.err"
}
net_run() { # PUBLIC BIND AGENT: resolve the flags and write hub.env
    hubrun "public_url='$1'; bind_opt='$2'; agent_url='$3'; resolve_net; write_hub_env"
}
rm -f "$work/hub.env" "$work/hub.log"
check "a fresh hub.env is written" net_run https://domovoi.example loopback wss://domovoi.example:7421
check "hub.env has the bind" grep -qx 'DOMOVOI_WEB__BIND=127.0.0.1:7420' "$work/hub.env"
check "hub.env has the public URL" grep -qx 'DOMOVOI_WEB__PUBLIC_URL=https://domovoi.example' "$work/hub.env"
check "hub.env has the agent URL" grep -qx 'DOMOVOI_AGENT_SERVER__PUBLIC_URL=wss://domovoi.example:7421' "$work/hub.env"
check "hub.env is mode 0640" test "$(stat -c %a "$work/hub.env")" = 640
cp "$work/hub.env" "$work/hub.env.first"
net_run https://domovoi.example loopback wss://domovoi.example:7421
check "a second run changes nothing" cmp -s "$work/hub.env" "$work/hub.env.first"
net_run "" "" ""
check "no flags leave hub.env alone" cmp -s "$work/hub.env" "$work/hub.env.first"
printf '# mine\nDISCORD_TOKEN=abc\nDOMOVOI_WEB__BIND=1.2.3.4:1\nOTHER=1\nDOMOVOI_WEB__BIND=9.9.9.9:9\n' >"$work/hub.env"
net_run "" loopback ""
check "a flag replaces the old value and drops the duplicate" test "$(grep -c '^DOMOVOI_WEB__BIND=' "$work/hub.env")" -eq 1
check "the new bind is in place" grep -qx 'DOMOVOI_WEB__BIND=127.0.0.1:7420' "$work/hub.env"
check "other lines and comments are kept" sh -c "grep -qx '# mine' '$work/hub.env' && grep -qx 'DISCORD_TOKEN=abc' '$work/hub.env' && grep -qx 'OTHER=1' '$work/hub.env'"
check "a key no flag names is not added" sh -c "! grep -q PUBLIC_URL '$work/hub.env'"
printf 'DOMOVOI_WEB__PUBLIC_URL=http://keep.example:7420\n' >"$work/hub.env"
ip_stub='ip() { echo "1.1.1.1 via 10.0.0.1 dev eth0 src 10.0.0.5 uid 0"; }'
hubrun "$ip_stub; public_url=''; bind_opt=lan; agent_url=''; resolve_net; write_hub_env"
check "--bind lan keeps an existing public URL" grep -qx 'DOMOVOI_WEB__PUBLIC_URL=http://keep.example:7420' "$work/hub.env"
check "--bind lan writes the bind" grep -qx 'DOMOVOI_WEB__BIND=0.0.0.0:7420' "$work/hub.env"
check "--bind lan takes the agent host from the existing public URL" grep -qx 'DOMOVOI_AGENT_SERVER__PUBLIC_URL=wss://keep.example:7421' "$work/hub.env"
rm -f "$work/hub.env"
hubrun "$ip_stub; public_url=''; bind_opt=lan; agent_url=''; resolve_net; write_hub_env"
check "--bind lan derives the public URL from the route address" grep -qx 'DOMOVOI_WEB__PUBLIC_URL=http://10.0.0.5:7420' "$work/hub.env"
check "--bind lan derives the agent URL from the same host" grep -qx 'DOMOVOI_AGENT_SERVER__PUBLIC_URL=wss://10.0.0.5:7421' "$work/hub.env"
check "the derivation is announced" grep -q "Derived the web address http://10.0.0.5:7420" "$work/hub.out"
check "plain http on a network address warns once" test "$(grep -c 'unencrypted' "$work/hub.err")" -eq 1
rm -f "$work/hub.env"
hubrun "ip() { return 1; }; hostname() { echo '127.0.0.1 172.16.0.9 '; }; public_url=''; bind_opt=lan; agent_url=''; resolve_net; write_hub_env"
check "--bind lan falls back to hostname -I" grep -qx 'DOMOVOI_WEB__PUBLIC_URL=http://172.16.0.9:7420' "$work/hub.env"
rm -f "$work/hub.env"
if hubrun "ip() { return 1; }; hostname() { :; }; public_url=''; bind_opt=lan; agent_url=''; resolve_net"; then
    fail "--bind lan with no detectable address is refused"
else
    pass "--bind lan with no detectable address is refused"
fi
check "that refusal asks for --public-url" grep -q "pass --public-url" "$work/hub.err"
check "--bind lan with --public-url needs no detection" hubrun "ip() { return 1; }; public_url=https://domovoi.example; bind_opt=lan; agent_url=''; resolve_net; test \"\$set_agent_url\" = wss://domovoi.example:7421"
check "https and loopback addresses do not warn" sh -c "! grep -q unencrypted '$work/hub.err'"
hubrun "public_url=http://127.0.0.1:7420; resolve_net"
check "plain http on loopback does not warn" sh -c "! grep -q unencrypted '$work/hub.err'"

# create_admin: the password goes to the command's stdin and never to its arguments.
printf '%s\n' "$secret" >"$work/ht/admin.pw"
admin_stub='run_as_domovoi() { cat >"'"$work"'/pw.seen"; printf "%s\n" "$*" >"'"$work"'/argv.seen"; }'
rm -f "$work/pw.seen" "$work/argv.seen" "$work/hub.log"
check "create_admin runs set-password on a fresh hub" hubrun "$admin_stub; bin=/x/domovoi; create_admin; test \"\$admin_created\" = 1"
check "the password reaches the command through stdin" test "$(cat "$work/pw.seen")" = "$secret"
check "the command is hub set-password --password-stdin" grep -qx '/x/domovoi hub set-password --password-stdin' "$work/argv.seen"
check "the password is not in the arguments" sh -c "! grep -q 'horse' '$work/argv.seen'"
check "the password is not in the log" sh -c "! grep -q 'horse' '$work/hub.log' '$work/hub.out' '$work/hub.err'"
printf '%s\n' "$secret" >"$work/ht/admin.pw"
rm -f "$work/pw.seen"
check "create_admin skips an upgrade" hubrun "$admin_stub; bin=/x/domovoi; upgrade=1; create_admin; test \"\$admin_created\" = 0"
check "the skipped upgrade says why" grep -q "never reset" "$work/hub.out"
check "the skipped upgrade did not run the command" test ! -e "$work/pw.seen"
check "create_admin skips a data directory that is not empty" hubrun "$admin_stub; bin=/x/domovoi; hub_fresh_data=0; create_admin; test \"\$admin_created\" = 0"
check "the skipped run did not run the command" test ! -e "$work/pw.seen"
if hubrun 'run_as_domovoi() { echo "Error: the password must be at least 12 characters"; return 1; }; bin=/x/domovoi; create_admin'; then
    fail "a rejected password stops the install"
else
    pass "a rejected password stops the install"
fi
check "the hub's message is shown" grep -q "at least 12 characters" "$work/hub.err"
check "the failure still says nothing about the password itself" sh -c "! grep -q 'horse' '$work/hub.log' '$work/hub.out' '$work/hub.err'"

# Readiness: poll GET /readyz, print progress, and on a timeout show the journal and exit 40.
check "the local URL is loopback by default" test "$(hubrun 'local_web_url' && cat "$work/hub.out")" = "http://127.0.0.1:7420"
printf 'DOMOVOI_WEB__BIND=0.0.0.0:7431\n' >"$work/hub.env"
hubrun 'local_web_url'
check "a wildcard bind is probed on loopback" test "$(cat "$work/hub.out")" = "http://127.0.0.1:7431"
printf 'DOMOVOI_WEB__BIND=[::]:7432\n' >"$work/hub.env"
hubrun 'local_web_url'
check "an IPv6 wildcard bind is probed on ::1" test "$(cat "$work/hub.out")" = "http://[::1]:7432"
rm -f "$work/hub.env"
ready_after() { # N: probe_ready fails N times, then succeeds
    # shellcheck disable=SC2016 # the function body is for the child shell
    printf 'probe_ready() { n=$(cat %s/n 2>/dev/null || echo 0); echo $((n + 1)) >%s/n; [ "$n" -ge %s ]; }' "$work" "$work" "$1"
}
rm -f "$work/n"
check "wait_ready returns once the hub answers" hubrun "$(ready_after 7); DOMOVOI_READY_TIMEOUT=60; wait_ready http://127.0.0.1:7420"
check "wait_ready prints a progress line" grep -q "still waiting for the hub" "$work/hub.out"
check "wait_ready says the hub is ready" grep -q "The hub is ready" "$work/hub.out"
rm -f "$work/n" "$work/hub.log"
journal_stub='journalctl() { echo "hub: starting"; echo "  http://127.0.0.1:7420/setup?token=0123456789abcdef"; }'
if hubrun "$journal_stub; probe_ready() { return 1; }; DOMOVOI_READY_TIMEOUT=3; wait_ready http://127.0.0.1:7420"; then
    fail "a hub that never answers stops the install"
else
    pass "a hub that never answers stops the install"
fi
hub_rc=0
hubrun "$journal_stub; probe_ready() { return 1; }; DOMOVOI_READY_TIMEOUT=3; wait_ready http://127.0.0.1:7420" || hub_rc=$?
check "the readiness timeout exits 40" test "$hub_rc" -eq 40
check "the timeout shows the journal" grep -q "hub: starting" "$work/hub.out" "$work/hub.err"
check "the timeout hides the setup token on screen" sh -c "! grep -q 0123456789abcdef '$work/hub.out' '$work/hub.err'"
check "the timeout keeps the setup token out of the log" sh -c "! grep -q 0123456789abcdef '$work/hub.log'"
check "the timeout log holds the redacted line" grep -q "token=REDACTED" "$work/hub.log"

# start_hub: enable --now on a fresh install, restart only a running hub on an upgrade.
# shellcheck disable=SC2016 # the stub is for the child shell
sysctl_stub='systemctl() { echo "$*" >>"'"$work"'/systemctl.log"; case "$1" in is-active) [ "${ACTIVE:-0}" = 1 ];; *) :;; esac; }; probe_ready() { return 0; }'
: >"$work/systemctl.log"
hubrun "$sysctl_stub; start_hub; test \"\$started\" = 1"
check "a fresh install enables and starts the hub" grep -qx 'enable --now domovoi-hub' "$work/systemctl.log"
: >"$work/systemctl.log"
ACTIVE=1 hubrun "$sysctl_stub; upgrade=1; start_hub; test \"\$started\" = 1"
check "an upgrade restarts a running hub" grep -qx 'restart domovoi-hub' "$work/systemctl.log"
check "an upgrade does not enable the hub again" sh -c "! grep -q enable '$work/systemctl.log'"
: >"$work/systemctl.log"
ACTIVE=0 hubrun "$sysctl_stub; upgrade=1; start_hub; test \"\$started\" = 0"
check "an upgrade leaves a stopped hub stopped" sh -c "! grep -qE 'restart|enable|start' '$work/systemctl.log'"
check "the stopped hub is reported" grep -q "left stopped" "$work/hub.out"

# The setup link: found in the journal, rebuilt on the address people open.
journal_stub='journalctl() { echo "old"; echo "  http://127.0.0.1:7420/setup?token=aaaa1111"; echo "  http://127.0.0.1:7420/setup?token=bbbb2222"; }'
hubrun "$journal_stub; web_url=https://domovoi.example; find_setup_link; echo \"\$setup_url\""
check "the newest setup token is used on the public URL" test "$(cat "$work/hub.out")" = "https://domovoi.example/setup?token=bbbb2222"
# shellcheck disable=SC2016
hubrun 'journalctl() { echo nothing; }; web_url=https://domovoi.example; find_setup_link; echo "[$setup_url]"'
check "no setup link in the journal gives an empty result" test "$(cat "$work/hub.out")" = "[]"

if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "all installer checks passed"
