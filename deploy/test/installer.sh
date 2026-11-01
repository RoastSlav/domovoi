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

for path in /usr/local/bin/domovoi /etc/domovoi /var/lib/domovoi /var/lib/domovoi-agent; do
    [ ! -e "$path" ] || {
        echo "$path exists; run this on a clean machine" >&2
        exit 2
    }
done

# The units embedded in install.sh are the ones in deploy/systemd/.
for role in hub agent; do
    check "embedded $role unit matches deploy/systemd" \
        sh -c "sh '$installer' --print-unit $role | cmp -s - '$root/deploy/systemd/domovoi-$role.service'"
done

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
check "hub next steps printed" grep -q "systemctl enable --now domovoi-hub" "$work/out"
check "setup hint printed" grep -q "journalctl -u domovoi-hub | grep setup" "$work/out"
check "attestation hint printed" grep -q "gh attestation verify" "$work/out"

# An agent on the same machine, at the latest version.
check "agent install succeeds" run_install --role agent
check "latest resolved to v9.9.1" sh -c '/usr/local/bin/domovoi version | grep -q v9.9.1'
check "/var/lib/domovoi-agent is 0700" test "$(mode_of /var/lib/domovoi-agent)" = 700
check "/etc/domovoi kept its group" test "$(owner_of /etc/domovoi)" = "root:domovoi"
check "no agent.toml written" test ! -e /etc/domovoi/agent.toml
check "agent unit installed" cmp -s /etc/systemd/system/domovoi-agent.service "$root/deploy/systemd/domovoi-agent.service"
check "agent next steps printed" grep -q "Add host" "$work/out"

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
check "changed mode kept" test "$(mode_of /var/lib/domovoi)" = 700
check "unchanged unit reported up to date" sh -c "sh '$installer' --role agent 2>&1 | grep -q 'domovoi-agent.service is up to date'"

# A bad checksum stops the install before anything is replaced.
before="$(sha256sum /usr/local/bin/domovoi)"
echo "0000000000000000000000000000000000000000000000000000000000000000  domovoi-v9.9.0-$target.tar.gz" \
    >"$work/srv/download/v9.9.0/domovoi-v9.9.0-$target.tar.gz.sha256"
if run_install --role hub --version v9.9.0; then
    fail "bad checksum is refused"
else
    pass "bad checksum is refused"
fi
check "bad checksum explained" grep -q "does not match" "$work/out"
check "binary untouched after a bad checksum" test "$(sha256sum /usr/local/bin/domovoi)" = "$before"

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
