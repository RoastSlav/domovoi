#!/bin/sh
# Installs Domovoi from a GitHub release.
#
#   curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh | sudo sh -s -- --role hub
#
# It downloads the release archive for this machine, checks its SHA-256, installs the
# binary, and for --role hub or --role agent creates the user, directories and systemd
# unit. It never writes a config file and never starts a service. Running it again
# upgrades the binary and leaves everything else alone.
set -eu

REPO="RoastSlav/domovoi"
RELEASE_BASE="${DOMOVOI_RELEASE_BASE:-https://github.com/$REPO/releases}"

usage() {
    cat <<'EOF'
Usage: install.sh --role hub|agent [options]

Options:
  --role hub|agent     what this machine runs (required)
  --version vX.Y.Z     the release to install (default: the latest)
  --prefix DIR         where the binary goes, as DIR/bin/domovoi (default: /usr/local)
  --no-service         install only the binary and examples: no user, directories or unit
  --print-unit ROLE    print the systemd unit for hub or agent and exit
  -h, --help           show this help

Environment:
  DOMOVOI_RELEASE_BASE  download from this base URL instead of GitHub releases
EOF
}

say() { printf '%s\n' "$*"; }
fail() {
    printf 'install.sh: %s\n' "$*" >&2
    exit 1
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

role=""
version="latest"
prefix="/usr/local"
service=1

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
    --print-unit)
        [ $# -ge 2 ] || fail "--print-unit needs hub or agent"
        case "$2" in
        hub) hub_unit ;;
        agent) agent_unit ;;
        *) fail "--print-unit takes hub or agent, not $2" ;;
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

case "$role" in
hub | agent) ;;
"")
    usage >&2
    fail "--role is required"
    ;;
*) fail "--role takes hub or agent, not $role" ;;
esac

case "$version" in
latest | v*) ;;
*) version="v$version" ;;
esac

bin_dir="$prefix/bin"
bin="$bin_dir/domovoi"

if [ "$(id -u)" -ne 0 ]; then
    if [ "$service" -eq 1 ]; then
        fail "run as root (sudo), or pass --no-service with a --prefix you can write to"
    fi
    mkdir -p "$bin_dir" 2>/dev/null || true
    [ -w "$bin_dir" ] || fail "cannot write to $bin_dir; run as root or choose another --prefix"
fi

# The release archive for this machine.
case "$(uname -s)" in
Linux) ;;
*) fail "only Linux is supported; this is $(uname -s)" ;;
esac
case "$(uname -m)" in
x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
*) fail "no release build for $(uname -m); build from source instead" ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL --proto '=https,http' -o "$2" "$1"; }
    resolve_latest() { curl -fsSLI -o /dev/null -w '%{url_effective}' "$RELEASE_BASE/latest"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q -O "$2" "$1"; }
    resolve_latest() {
        wget -S --spider "$RELEASE_BASE/latest" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1 | tr -d '\r'
    }
else
    fail "curl or wget is needed"
fi
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is needed"
command -v tar >/dev/null 2>&1 || fail "tar is needed"

if [ "$version" = "latest" ]; then
    url="$(resolve_latest)" || fail "could not find the latest release at $RELEASE_BASE/latest"
    url="${url%/}"
    version="${url##*/}"
    case "$version" in
    v*) ;;
    *) fail "could not find the latest release at $RELEASE_BASE/latest (got $url)" ;;
    esac
fi

name="domovoi-$version-$target"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' INT TERM

say "Downloading $name.tar.gz"
fetch "$RELEASE_BASE/download/$version/$name.tar.gz" "$tmp/$name.tar.gz" ||
    fail "could not download $RELEASE_BASE/download/$version/$name.tar.gz"
fetch "$RELEASE_BASE/download/$version/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" ||
    fail "could not download the checksum for $name.tar.gz"
(cd "$tmp" && sha256sum -c "$name.tar.gz.sha256" >/dev/null 2>&1) ||
    fail "the SHA-256 of $name.tar.gz does not match its .sha256 file; nothing was installed"
say "Checksum verified. To check where the archive was built, download it and run"
say "  gh attestation verify $name.tar.gz --repo $REPO"

tar -C "$tmp" -xzf "$tmp/$name.tar.gz"
[ -f "$tmp/$name/domovoi" ] || fail "$name.tar.gz has no domovoi binary"
"$tmp/$name/domovoi" version >/dev/null 2>&1 || fail "the downloaded binary does not run on this machine"

mkdir -p "$bin_dir"
install -m 0755 "$tmp/$name/domovoi" "$bin"
say "Installed $("$bin" version | head -n 1) to $bin"

if [ -d "$tmp/$name/examples" ]; then
    examples="$prefix/share/domovoi/examples"
    mkdir -p "$examples"
    cp -R "$tmp/$name/examples/." "$examples/"
fi

if [ "$service" -eq 0 ]; then
    say "Skipped the user, directories and systemd unit (--no-service)."
    exit 0
fi

# Creates DIR with MODE and OWNER only when it does not exist, so a second run keeps
# whatever was changed since.
make_dir() {
    if [ ! -d "$1" ]; then
        mkdir -p "$1"
        chmod "$2" "$1"
        chown "$3" "$1"
    fi
}

if [ "$role" = "hub" ]; then
    if ! id domovoi >/dev/null 2>&1; then
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
        say "Created the domovoi system user."
    fi
    make_dir /etc/domovoi 0750 root:domovoi
    make_dir /var/lib/domovoi 0750 domovoi:domovoi
else
    make_dir /etc/domovoi 0750 root:root
    make_dir /var/lib/domovoi-agent 0700 root:root
fi

# The unit, with ExecStart pointing at this --prefix.
unit_name="domovoi-$role.service"
unit_path="/etc/systemd/system/$unit_name"
"${role}_unit" | sed "s|/usr/local/bin/domovoi|$bin|" >"$tmp/$unit_name"
if [ ! -e "$unit_path" ]; then
    mkdir -p /etc/systemd/system
    install -m 0644 "$tmp/$unit_name" "$unit_path"
    say "Installed $unit_path"
elif cmp -s "$tmp/$unit_name" "$unit_path"; then
    say "$unit_path is up to date."
else
    say "Kept $unit_path: it differs from this release's unit. The new one is"
    say "  $prefix/share/domovoi/$unit_name"
    mkdir -p "$prefix/share/domovoi"
    install -m 0644 "$tmp/$unit_name" "$prefix/share/domovoi/$unit_name"
fi
if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload
fi

say ""
say "Next steps:"
if [ "$role" = "hub" ]; then
    say "  systemctl enable --now domovoi-hub"
    say "  journalctl -u domovoi-hub | grep setup     # open this link to create the admin account"
    say "  sudo -u domovoi $bin selftest --offline    # optional: checks the pipeline end to end"
    say ""
    say "No config file is needed: everything is set in the web UI."
else
    say "  1. In the hub's web UI, open Hosts, then Add host, and run the enroll command it shows here."
    say "  2. systemctl enable --now domovoi-agent"
fi
