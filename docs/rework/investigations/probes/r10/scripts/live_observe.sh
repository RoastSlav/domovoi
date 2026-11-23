#!/usr/bin/env bash
# O1: optional, strictly read-only observation of the host's routing state. Output is printed;
# nothing is added, replaced or deleted. Not a validation of the hash contract.
set -u
export PATH="/usr/sbin:/sbin:/usr/bin:/bin"
echo "uname -r: $(uname -r)"; echo "id -u: $(id -u)"
for c in "ip -4 rule show" "ip -6 rule show" "ip -4 route show table all" "ip -6 route show table all" "ip -br addr show"; do
  echo "--- $c"; $c 2>&1 | head -40
done
