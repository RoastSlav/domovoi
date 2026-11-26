#!/usr/bin/env bash
# Records `nft -j` listings and error text from the nft on this machine, in a throwaway network
# namespace (rootless: `unshare -Urn`). Documentation addresses only. Usage:
#   unshare -Urn bash record.sh <output directory>
# The output directory is named for the version, such as v1.1.7.
set -u
out="$1"
mkdir -p "$out"
cd "$out"
ip link set lo up
nft -f - <<EOF
create table inet domovoi
add set inet domovoi blocked4 { type ipv4_addr; flags timeout; timeout 30s; size 2; }
add set inet domovoi blockednet4 { type ipv4_addr; flags interval,timeout; timeout 30s; size 1000; }
add set inet domovoi excl4 { type ipv4_addr; flags interval; }
add chain inet domovoi contain { type filter hook prerouting priority -310; policy accept; }
add rule inet domovoi contain counter
add rule inet domovoi contain ip saddr @excl4 accept
add rule inet domovoi contain ip saddr @blocked4 counter drop
add rule inet domovoi contain ip saddr @blockednet4 counter drop
add element inet domovoi excl4 { 192.0.2.0-192.0.2.255 }
create element inet domovoi blocked4 { 203.0.113.7 timeout 30s }
create element inet domovoi blockednet4 { 198.51.100.0/24 timeout 30s }
EOF
nft -j list table inet domovoi > list_table.json
nft --version > version.txt
run() { # name, script
  printf '%s\n' "$2" | nft -f - > "$1.out" 2> "$1.err"
  echo "$? $1" >> exits.txt
}
: > exits.txt
run create_exists "create element inet domovoi blocked4 { 203.0.113.7 timeout 30s }"
run delete_absent "delete element inet domovoi blocked4 { 203.0.113.9 }"
run delete_interval_absent "delete element inet domovoi blockednet4 { 198.51.100.64/26 }"
run add_interval_overlap "add element inet domovoi blockednet4 { 198.51.100.64/26 timeout 5s }"
run create_table_exists "create table inet domovoi"
run set_full "create element inet domovoi blocked4 { 203.0.113.20 timeout 30s }
create element inet domovoi blocked4 { 203.0.113.21 timeout 30s }"
run missing_table_script "delete element inet nosuchtable blocked4 { 203.0.113.9 }"
nft -j list table inet nosuchtable > missing_table.out 2> missing_table.err
echo "$? missing_table" >> exits.txt
