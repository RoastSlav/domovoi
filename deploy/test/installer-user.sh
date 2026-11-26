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
    probe_ready redact_tokens wait_ready start_hub journal_setup_token find_setup_link manifest_add rollback_disarm; do
    if grep -q "^$fn() {.*}\$" "$installer"; then
        grep "^$fn() {.*}\$" "$installer" >>"$hfns"
    else
        sed -n "/^$fn() {/,/^}/p" "$installer" >>"$hfns"
    fi
done
check "the hub functions were extracted" test "$(grep -c '^[a-z_]*() {' "$hfns")" -eq 27
hubrun() { # SCRIPT: runs it with the extracted functions, no-op sleep and chown, and a temp dir
    mkdir -p "$work/ht"
    sh -c "set -eu; . '$hfns'; sleep() { :; }; chown() { :; }; tmp='$work/ht'; log_file='$work/hub.log'; hub_env='$work/hub.env'; quiet=0; json=0; upgrade=0; hub_fresh_data=1; rollback_armed=0; admin_created=0; started=0; setup_url=''; public_url=''; bind_opt=''; agent_url=''; $1" \
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

# --- Agent enrollment and --role both: flags, token handling, ordering -------------------------
# The enrollment flags are checked before the preflight, so these run without root.
fp="$(printf 'ab%.0s' $(seq 32))"
fpc="$(printf 'AB:%.0s' $(seq 31))AB"
tok="tok_SECRET0123456789abcd"
tokf="$work/enroll.tok"
printf '%s\n' "$tok" >"$tokf"
chmod 0600 "$tokf"
expect_trio() { # DESCRIPTION CODE ARGS...: the three enrollment flags are added
    et_desc="$1"
    et_code="$2"
    shift 2
    expect_rc "$et_desc" "$et_code" --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf" "$@"
}
if [ "$(id -u)" -ne 0 ]; then
expect_trio "a full trio passes validation (then the preflight needs root)" 10 --role agent --name nas
expect_rc "colons in the fingerprint are accepted" 10 --role agent --hub wss://hub.lan:7421 --fingerprint "$fpc" --token-file "$tokf"
expect_rc "an IPv4 hub address is accepted" 10 --role agent --hub wss://10.0.0.5:7421 --fingerprint "$fp" --token-file "$tokf"
expect_rc "a literal --token is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token "$tok"
check "the --token refusal points to --token-file" grep -q -- "--token-file" "$work/err"
check "the --token refusal points to DOMOVOI_ENROLL_TOKEN" grep -q "DOMOVOI_ENROLL_TOKEN" "$work/err"
expect_rc "--token=VALUE is refused too" 2 --role agent --token="$tok"
expect_rc "--token with --role both is refused" 2 --role both --token "$tok"
check "the token is not in the output of a refused --token" sh -c "! grep -rq '$tok' '$work/out' '$work/err' '$DOMOVOI_INSTALL_LOG'"
expect_rc "--hub alone is refused" 2 --role agent --hub wss://hub.lan:7421
check "the partial set is explained" grep -q "all three or none" "$work/err"
expect_rc "--fingerprint alone is refused" 2 --role agent --fingerprint "$fp"
expect_rc "--token-file alone is refused" 2 --role agent --token-file "$tokf"
expect_rc "hub and fingerprint without a token are refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$fp"
expect_rc "hub and token without a fingerprint are refused" 2 --role agent --hub wss://hub.lan:7421 --token-file "$tokf"
check "the token alone in the environment is refused" sh -c "DOMOVOI_ENROLL_TOKEN='$tok' sh '$installer' --role agent >/dev/null 2>'$work/err'; test \$? -eq 2"
check "the token in the environment completes the set" sh -c "DOMOVOI_ENROLL_TOKEN='$tok' sh '$installer' --role agent --hub wss://hub.lan:7421 --fingerprint '$fp' >/dev/null 2>'$work/err'; test \$? -eq 10"
check "a token in the file and in the environment is refused" sh -c "DOMOVOI_ENROLL_TOKEN='$tok' sh '$installer' --role agent --hub wss://hub.lan:7421 --fingerprint '$fp' --token-file '$tokf' >/dev/null 2>'$work/err'; test \$? -eq 2"
expect_rc "an https hub address is refused" 2 --role agent --hub https://hub.lan:7421 --fingerprint "$fp" --token-file "$tokf"
check "the address refusal shows a good one" grep -q "wss://hub.lan:7421" "$work/err"
expect_rc "a hub address with a path is refused" 2 --role agent --hub wss://hub.lan:7421/agent --fingerprint "$fp" --token-file "$tokf"
expect_rc "a hub address with a space is refused" 2 --role agent --hub "wss://hub.lan:7421 x" --fingerprint "$fp" --token-file "$tokf"
expect_rc "a short fingerprint is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint abcd --token-file "$tokf"
expect_rc "a fingerprint that is not hex is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$(printf 'zz%.0s' $(seq 32))" --token-file "$tokf"
expect_rc "--name without the trio is refused" 2 --role agent --name nas
expect_rc "--name goes with agent or both" 2 --role hub --name nas
expect_trio "a name with capitals is refused" 2 --role agent --name NAS
expect_trio "a name with an underscore is refused" 2 --role agent --name my_nas
expect_trio "a name that starts with a dash is refused" 2 --role agent --name=-nas
expect_trio "--no-start with the trio is refused" 2 --role agent --no-start
expect_trio "--no-service with the trio is refused" 2 --role agent --no-service --prefix "$fresh"
expect_trio "--migrate-identity with the trio is refused" 2 --role agent --migrate-identity
expect_trio "the trio does not go with --role hub" 2 --role hub
rc=0
DOMOVOI_AGENT_TIMEOUT=soon sh "$installer" --role agent --dry-run >"$work/out" 2>"$work/err" || rc=$?
check "a bad DOMOVOI_AGENT_TIMEOUT exits 2 and is named" sh -c "test $rc -eq 2 && grep -q DOMOVOI_AGENT_TIMEOUT '$work/err'"
# The token file follows the rules of the admin password file.
chmod 0644 "$tokf"
expect_trio "a token file with mode 0644 is refused" 2 --role agent
check "the token mode refusal says how to fix it" grep -q "chmod 600" "$work/err"
chmod 0400 "$tokf"
expect_trio "a token file with mode 0400 is accepted" 10 --role agent
chmod 0600 "$tokf"
ln -sf "$tokf" "$work/enroll.link"
expect_rc "a link to a token file is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$work/enroll.link"
expect_rc "a directory as token file is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$work"
expect_rc "a missing token file is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$work/none"
: >"$work/empty.tok"
chmod 0600 "$work/empty.tok"
expect_rc "an empty token file is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$work/empty.tok"
printf 'not a token!\n' >"$work/odd.tok"
chmod 0600 "$work/odd.tok"
expect_rc "a token with odd characters is refused" 2 --role agent --hub wss://hub.lan:7421 --fingerprint "$fp" --token-file "$work/odd.tok"
check "no refusal printed the token" sh -c "! grep -rq '$tok' '$work/out' '$work/err' '$DOMOVOI_INSTALL_LOG'"
# --role both.
expect_rc "--role both is a known role (then the preflight needs root)" 10 --role both --bind loopback
expect_rc "--role both takes --name" 10 --role both --name nas
expect_rc "--role both takes the hub flags" 10 --role both --public-url https://domovoi.example --agent-url wss://domovoi.example:7421
expect_rc "--role both refuses --no-start" 2 --role both --no-start
check "the --no-start refusal says why" grep -q "hub has to run" "$work/err"
expect_rc "--role both refuses --hub" 2 --role both --hub wss://hub.lan:7421
expect_rc "--role both refuses --fingerprint" 2 --role both --fingerprint "$fp"
expect_rc "--role both refuses --token-file" 2 --role both --token-file "$tokf"
expect_rc "--role both refuses a bad name" 2 --role both --name Bad_Name
expect_rc "--role both takes an admin password file" 10 --role both --admin-password-file "$pwf"
expect_rc "a role other than hub, agent or both is refused" 2 --role agents
expect_rc "--accept-data-reset is refused for an agent" 2 --role agent --accept-data-reset
expect_rc "--accept-data-reset is known for the hub" 10 --role hub --accept-data-reset
expect_rc "--accept-data-reset is known for both" 10 --role both --accept-data-reset
check "usage lists the enrollment flags and variables" sh -c "sh '$installer' --help | grep -q -- '--hub wss' && sh '$installer' --help | grep -q -- '--fingerprint' && sh '$installer' --help | grep -q -- '--token-file' && sh '$installer' --help | grep -q DOMOVOI_ENROLL_TOKEN && sh '$installer' --help | grep -q DOMOVOI_AGENT_TIMEOUT && sh '$installer' --help | grep -q -- '--accept-data-reset' && sh '$installer' --help | grep -q -- '--role hub|agent|both'"
check "usage says a token is never taken from a flag" sh -c "sh '$installer' --help | grep -q -- '--token VALUE is refused'"
expect_rc "a json install carries the new fields" 0 --role agent --no-service --prefix "$p" --version v9.9.1 --json
check "the json result has an empty enrolled_host" test "$(result_field "$work/out" 'r["enrolled_host"]')" = ""
check "the json result says the agent was not started" test "$(result_field "$work/out" 'r["agent_started"]')" = "False"
fi

# The enrollment functions, taken out of the script as written and run with stubs. sleep is a
# no-op; the binary is a stub that records its arguments and the token it was given.
efns="$work/enroll-functions.sh"
: >"$efns"
for fn in say log_line say_always err warn fail_with fail show_file show_file_err ns env_get \
    local_web_url probe_ready redact_tokens wait_ready check_url read_secret_file default_host_name \
    resolve_enroll_name resolve_enroll scrub_token agent_hint wait_agent_link enroll_and_start_agent \
    both_make_token check_schema_reset ver_cmp agent_state_dir journal_exists ensure_agent_journal heal_agent_journal manifest_add rollback_disarm; do
    if grep -q "^$fn() {.*}\$" "$installer"; then
        grep "^$fn() {.*}\$" "$installer" >>"$efns"
    else
        sed -n "/^$fn() {/,/^}/p" "$installer" >>"$efns"
    fi
done
# record_next has a line that starts with a closing brace, so it is stubbed rather than extracted.
printf '%s\n' 'record_next() { :; }' >>"$efns"
check "the enrollment functions were extracted" test "$(grep -c '^[a-z_]*() {' "$efns")" -eq 34
elog="$work/elog"
jdir="$work/agent-state"
mkdir -p "$elog" "$work/et" "$work/estub"
fp_colon="$fpc"
cat >"$work/estub/domovoi" <<STUB
#!/bin/sh
echo "\$*" >>"$elog/argv.log"
case "\$*" in
"agent enroll "*)
    printf '%s' "\${DOMOVOI_ENROLL_TOKEN:-unset}" >"$elog/enroll.env"
    if [ -e "$elog/enroll-fails" ]; then
        echo "error: the token $tok was refused (--token $tok)" >&2
        exit 1
    fi
    echo "Wrote /etc/domovoi/agent.toml."
    echo "Enrolled as host 1 (agent 2). It echoes $tok by mistake."
    ;;
"agent journal init")
    if [ -e "$elog/journal-fails" ]; then
        echo "error: the action journal could not be created" >&2
        exit 1
    fi
    mkdir -p "$jdir"
    : >"$jdir/action-journal.json"
    : >"$jdir/action-journal.bin"
    echo "The action journal at $jdir now uses epoch 1."
    ;;
"agent status")
    if [ -e "$elog/connected" ]; then
        echo "connected since 2030-01-01T00:00:00Z."
    elif [ -e "$elog/incompatible" ]; then
        echo "incompatible: this agent speaks protocol 1.0, the hub speaks 2.0. Upgrade the agent, then restart it. (updated now)"
    else
        echo "reconnecting: connection refused (updated now)"
    fi
    ;;
"hub token create "*)
    if [ -e "$elog/hub-active" ]; then echo "Error: another hub holds the lock" >&2; exit 1; fi
    if [ -e "$elog/token-fails" ]; then echo "Error: cannot open the database" >&2; exit 1; fi
    if [ -e "$elog/token-odd" ]; then echo "something else entirely"; exit 0; fi
    echo "Enrollment token for \$4, valid until 2030-01-01T00:15:00Z."
    echo "Hub fingerprint: $fp_colon"
    echo "Run this on \$4:"
    echo
    echo "  domovoi agent enroll --hub wss://127.0.0.1:7421/ --token $tok --fingerprint $fp_colon --name \$4"
    ;;
esac
STUB
chmod 0755 "$work/estub/domovoi"
# shellcheck disable=SC2016 # the stubs are for the child shell
estubs='systemctl() { echo "systemctl $*" >>"'"$elog"'/argv.log"; case "$1 $2" in "stop domovoi-hub") rm -f "'"$elog"'/hub-active" ;; "start domovoi-hub") touch "'"$elog"'/hub-active" ;; esac; }; journalctl() { echo "agent: dialing wss://hub.lan:7421"; echo "agent: enroll --token '"$tok"'"; }; run_as_domovoi() { "$@"; }; probe_ready() { return 0; }; step() { echo "$*" >>"'"$elog"'/steps.log"; }'
erun() { # SCRIPT: runs it with the extracted functions and the stubs
    rm -rf "$work/et"
    mkdir -p "$work/et"
    printf 'state_dir = "%s"\n' "$jdir" >"$work/agent.toml"
    : >"$work/agent.unit"
    if [ -z "${keep_journal:-}" ]; then rm -rf "$jdir"; fi
    printf '%s\n' "$tok" >"$work/et/enroll.tok"
    chmod 0600 "$work/et/enroll.tok"
    sh -c "set -eu; . '$efns'; sleep() { :; }; $estubs; tmp='$work/et'; log_file='$work/e.log'; hub_env='$work/e.env'; quiet=0; json=0; bin='$work/estub/domovoi'; agent_toml='$work/agent.toml'; agent_unit_path='$work/agent.unit'; enroll_hub=wss://hub.lan:7421; enroll_fp='$fp'; enroll_name=nas; agent_timeout=3; enrolled_host=''; agent_started=0; hub_stopped=0; rollback_armed=0; role=agent; $1" \
        >"$work/e.out" 2>"$work/e.err"
}
no_token_anywhere() {
    ! grep -rq "$tok" "$work/e.out" "$work/e.err" "$work/e.log" 2>/dev/null
}

# The host name default.
for pair in "My_Host.Example:my-host-example" "NAS_01:nas-01" "--odd--:odd" "plain:plain" "a b:a-b"; do
    raw="${pair%%:*}"
    want="${pair##*:}"
    check "default host name of '$raw' is '$want'" \
        test "$(sh -c ". '$efns'; hostname() { echo '$raw'; }; default_host_name")" = "$want"
done
long="$(printf 'x%.0s' $(seq 70))-end"
check "a long host name is cut to 63 characters" test "$(sh -c ". '$efns'; hostname() { echo '$long'; }; default_host_name" | wc -c)" -eq 64
check "a host name of only symbols gives nothing" test -z "$(sh -c ". '$efns'; hostname() { echo '!!!'; }; default_host_name")"
check "the default name is used and announced when --name is absent" \
    sh -c ". '$efns'; hostname() { echo 'Media_Box'; }; log_file=''; quiet=0; enroll_name=''; resolve_enroll_name >/dev/null; test \"\$enroll_name\" = media-box"

# Enrolling: the token reaches the command only through its environment.
rm -f "$elog"/*
check "enrolling and starting the agent succeeds when it connects" erun "touch '$elog/connected'; enroll_and_start_agent; test \"\$agent_started\" = 1 && test \"\$enrolled_host\" = nas"
check "the enroll command gets the hub, fingerprint and name" grep -qx "agent enroll --hub wss://hub.lan:7421 --fingerprint $fp --name nas" "$elog/argv.log"
check "the token reaches enroll through its environment" test "$(cat "$elog/enroll.env")" = "$tok"
check "the token is in no argument list" sh -c "! grep -q '$tok' '$elog/argv.log'"
check "the token is not in the output or the log" no_token_anywhere
check "enroll came before the unit was enabled" sh -c "test \"\$(grep -n 'agent enroll' '$elog/argv.log' | cut -d: -f1)\" -lt \"\$(grep -n 'enable --now domovoi-agent' '$elog/argv.log' | cut -d: -f1)\""
check "the journal was created after enroll and before the unit was enabled" sh -c "test \"\$(grep -n 'agent enroll' '$elog/argv.log' | cut -d: -f1)\" -lt \"\$(grep -n 'agent journal init' '$elog/argv.log' | cut -d: -f1)\" && test \"\$(grep -n 'agent journal init' '$elog/argv.log' | cut -d: -f1)\" -lt \"\$(grep -n 'enable --now domovoi-agent' '$elog/argv.log' | cut -d: -f1)\""
check "the journal step was announced" grep -qx "journal Creating the agent's action journal" "$elog/steps.log"
check "the journal files exist after the start" test -e "$jdir/action-journal.json" -a -e "$jdir/action-journal.bin"
check "the status was asked after the unit was enabled" sh -c "test \"\$(grep -n 'enable --now domovoi-agent' '$elog/argv.log' | cut -d: -f1)\" -lt \"\$(grep -n 'agent status' '$elog/argv.log' | cut -d: -f1)\""
check "a leaked token in the enroll output is hidden" grep -q "REDACTED" "$work/e.out"
rm -f "$elog"/*
touch "$elog/enroll-fails"
e_rc=0
erun "enroll_and_start_agent" || e_rc=$?
check "a failed enroll exits 40" test "$e_rc" -eq 40
check "a failed enroll does not enable the unit" sh -c "! grep -q 'enable --now domovoi-agent' '$elog/argv.log'"
check "a failed enroll shows the message with the token hidden" grep -q "was refused (--token REDACTED" "$work/e.out"
check "a failed enroll leaves the token out of everything" no_token_anywhere
check "a failed enroll says what to do by hand" grep -q "To finish the agent by hand" "$work/e.out"
# The journal: skipped when it exists, exit 40 when init fails, healed on a re-run.
rm -f "$elog"/*
keep_journal=1
mkdir -p "$jdir"
: >"$jdir/action-journal.bin"
check "an existing journal is left alone" erun "touch '$elog/connected'; enroll_and_start_agent"
check "init is not run when a journal file exists" sh -c "! grep -q 'agent journal init' '$elog/argv.log'"
check "the unit is still enabled when the journal exists" grep -q 'enable --now domovoi-agent' "$elog/argv.log"
keep_journal=
rm -f "$elog"/*
touch "$elog/journal-fails"
e_rc=0
erun "enroll_and_start_agent" || e_rc=$?
check "a failed journal init exits 40" test "$e_rc" -eq 40
check "a failed journal init does not enable the unit" sh -c "! grep -q 'enable --now domovoi-agent' '$elog/argv.log'"
check "a failed journal init shows its message" grep -q "action journal could not be created" "$work/e.out" "$work/e.err"
check "a failed journal init names the command to run by hand" grep -q "sudo $work/estub/domovoi agent journal init" "$work/e.out"
check "a failed journal init names the unit to enable" grep -q "sudo systemctl enable --now domovoi-agent" "$work/e.out"
rm -f "$elog"/*
check "a re-run creates a journal an older installer left out" erun "heal_agent_journal; test \"\$ej_created\" = 1"
check "the heal ran init before the restart" sh -c "test \"\$(grep -n 'agent journal init' '$elog/argv.log' | cut -d: -f1)\" -lt \"\$(grep -n 'restart domovoi-agent' '$elog/argv.log' | cut -d: -f1)\""
check "the heal did not enroll again" sh -c "! grep -q 'agent enroll' '$elog/argv.log'"
rm -f "$elog"/*
keep_journal=1
: >"$jdir/action-journal.json"
: >"$elog/argv.log"
check "a re-run with a journal changes nothing" erun "heal_agent_journal; test \"\$ej_created\" = 0"
check "a re-run with a journal neither inits nor restarts" sh -c "! grep -q 'journal init\|restart' '$elog/argv.log'"
keep_journal=
# Waiting for the link.
rm -f "$elog"/*
e_rc=0
erun "enroll_and_start_agent" || e_rc=$?
check "an agent that never connects exits 40" test "$e_rc" -eq 40
check "the timeout shows the last journal lines" grep -q "agent: dialing wss://hub.lan:7421" "$work/e.out"
check "the timeout hides the token in the journal" grep -q "enroll --token REDACTED" "$work/e.out"
check "the timeout keeps the token out of everything" no_token_anywhere
check "the timeout names what to check" grep -q "agent status" "$work/e.out"
check "the timeout says how long it waited" grep -q "within 3 s" "$work/e.err"
check "the status was polled again and again" test "$(grep -c 'agent status' "$elog/argv.log")" -ge 3
rm -f "$elog"/*
touch "$elog/incompatible"
e_rc=0
erun "wait_agent_link" || e_rc=$?
check "a refused protocol stops the wait at once with exit 40" test "$e_rc" -eq 40
check "the refusal is shown" grep -q "speaks protocol 1.0" "$work/e.err"
check "the refusal did not wait for the timeout" test "$(grep -c 'agent status' "$elog/argv.log")" -eq 1

# --role both: stop the hub, make the token as the hub user, start it again.
rm -f "$elog"/*
touch "$elog/hub-active"
check "a token is made with the hub stopped" erun "both_make_token; test \"\$enroll_fp\" = '$fpc'"
check "the order is stop, token create, start" sh -c "grep -v '^agent' '$elog/argv.log' | tr '\n' '|' | grep -qx 'systemctl stop domovoi-hub|hub token create nas --ttl 15m|systemctl start domovoi-hub|'"
check "the token is in the private file only" test "$(cat "$work/et/enroll.tok")" = "$tok"
check "the token file is mode 0600" test "$(stat -c %a "$work/et/enroll.tok")" = 600
check "the raw token output is gone" sh -c "! test -e '$work/et/token.out'"
check "making the token printed and logged nothing secret" no_token_anywhere
check "the hub was waited for after the second start" grep -q "The hub is ready" "$work/e.out"
rm -f "$elog"/*
touch "$elog/hub-active" "$elog/token-fails"
e_rc=0
erun "both_make_token" || e_rc=$?
check "a failed token command exits 40" test "$e_rc" -eq 40
check "a failed token command still starts the hub again" grep -qx "systemctl start domovoi-hub" "$elog/argv.log"
check "a failed token command shows the hub's message" grep -q "cannot open the database" "$work/e.out"
check "a failed token command says the hub is running" grep -q "the hub is running" "$work/e.err"
check "a failed token command says what to do by hand" grep -q "To finish the agent by hand" "$work/e.out"
rm -f "$elog"/*
touch "$elog/hub-active" "$elog/token-odd"
e_rc=0
erun "both_make_token" || e_rc=$?
check "an unreadable token output exits 40" test "$e_rc" -eq 40
check "an unreadable token output still starts the hub again" grep -qx "systemctl start domovoi-hub" "$elog/argv.log"

# The cross-boundary rule: a hub database from before 0.1.0 meets 0.1.0 or later.
cdata="$work/cdata"
mkdir -p "$cdata"
: >"$cdata/hub.db"
schema_run() { # INSTALLED TARGET HUB_PART ACCEPT
    sh -c ". '$efns'; tmp='$work/et'; mkdir -p \"\$tmp\"; : >\"\$tmp/preflight\"; pf_add() { printf '%s|%s|%s\n' \"\$1\" \"\$2\" \"\$3\" >>\"\$tmp/preflight\"; }; hub_data_dir='$cdata'; installed_version='$1'; version='$2'; hub_part='$3'; accept_reset='$4'; reset_pending=0; check_schema_reset; echo \"pending=\$reset_pending\"; cat \"\$tmp/preflight\""
}
for case_ in "0.0.10:v0.1.0" "0.0.10:v0.1.1" "0.0.1:v1.0.0" "v0.0.10-rc.1:v0.1.0"; do
    inst="${case_%%:*}"
    targ="${case_##*:}"
    schema_run "$inst" "$targ" 1 0 >"$work/schema.out"
    check "$inst to $targ fails the preflight without the flag" grep -q '^fail|schema|' "$work/schema.out"
    check "$inst to $targ does not set reset_pending without the flag" grep -qx 'pending=0' "$work/schema.out"
done
schema_run 0.0.10 v0.1.0 1 0 >"$work/schema.out"
check "the refusal names the data directory" grep -q "$cdata" "$work/schema.out"
check "the refusal names the flag" grep -q -- "--accept-data-reset" "$work/schema.out"
check "the refusal says the schema is replaced" grep -q "replaces the whole database schema" "$work/schema.out"
check "the refusal says the data is not changed" grep -q "changes none of the data" "$work/schema.out"
check "the refusal says to move or delete the directory" grep -q "Move $cdata aside or delete it" "$work/schema.out"
schema_run 0.0.10 v0.1.0 1 1 >"$work/schema.out"
check "the flag turns the refusal into a warning" grep -q '^warn|schema|' "$work/schema.out"
check "the flag leaves no failed row" sh -c "! grep -q '^fail' '$work/schema.out'"
check "the flag sets reset_pending" grep -qx 'pending=1' "$work/schema.out"
check "the warning says the data is left untouched and the hub refuses it" sh -c "grep -q 'left untouched' '$work/schema.out' && grep -q 'cannot open it' '$work/schema.out'"
for case_ in "0.1.0:v0.1.1:1" "0.0.9:v0.0.10:1" "0.0.10:v0.1.0:0" "0.1.0-rc.1:v0.1.0:1"; do
    inst="${case_%%:*}"
    rest="${case_#*:}"
    targ="${rest%%:*}"
    part="${rest##*:}"
    schema_run "$inst" "$targ" "$part" 0 >"$work/schema.out"
    check "$inst to $targ (hub part $part) is allowed" sh -c "! grep -q '|schema|' '$work/schema.out' && grep -qx 'pending=0' '$work/schema.out'"
done
schema_run "" v0.1.0 1 0 >"$work/schema.out"
check "no installed binary means no refusal" sh -c "! grep -q '|schema|' '$work/schema.out'"
rm -f "$cdata/hub.db"
schema_run 0.0.10 v0.1.0 1 0 >"$work/schema.out"
check "a machine with no hub database is not refused" sh -c "! grep -q '|schema|' '$work/schema.out'"


if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed"
    exit 1
fi
echo "all installer checks passed"
