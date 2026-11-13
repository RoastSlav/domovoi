# 04b — Helper safety attestation: encoding, hashes and test vectors

Byte-level contract for RW-37. Status: **PROPOSAL**, part of R-10. Nothing here is implemented. The
vectors below were computed with a reference script (Python `tomllib`, `ipaddress`, `hashlib`) from
exactly the bytes shown. An implementation must reproduce every digest.

## 1. Field encoding

- **Lines.** UTF-8, each terminated by a single LF (0x0A). No CR, no trailing spaces, no blank
  lines, no BOM.
- **Line grammar.** `line = key "=" value`. `key` is one of the fixed keys listed in §3. `value`
  is one or more fields joined by `|` (0x7C).
- **Field encoding (percent-encoding, strict).**
  - Bytes in `A–Z a–z 0–9 . _ : / @ -` appear literally.
  - Every other byte, including `|`, `=`, `%`, `;`, `,`, space, CR, LF and any non-ASCII byte, is
    written as `%XX` with **uppercase** hex.
  - A decoder rejects:
    - lowercase hex;
    - an encoded byte from the literal set (for example `%41`);
    - a truncated escape;
    - any raw byte outside the literal set plus `%`.

  The rules apply to every field. So `|` can only ever be a field separator, and no value can
  inject a line or a field.
- **Typed fields** (already within the literal set once canonical):
  - `ip`: IPv4 dotted decimal without leading zeros, or IPv6 RFC 5952 lowercase.
  - `cidr`: network address with host bits zeroed, then `/prefix`. This is a normalisation:
    `192.168.1.7/24` becomes `192.168.1.0/24`.
  - `ifaddr`: interface address keeping its host bits, then `/prefix`.
  - integers: decimal, no sign, no leading zeros (`0` allowed).
  - `family`: `inet` or `inet6`.
  - `-`: absent optional value (gateway).
- **Repeated lines.** Within one key, lines are sorted bytewise ascending on the full encoded line.
  - The helper removes exact duplicates before emitting.
  - The hub rejects a report that contains duplicates or unsorted lines.
  - An empty group emits no lines.

## 2. Structured values

| Key | Fields, in order |
|---|---|
| `cidr` | cidr |
| `protected_unit` | unit (percent-encoded) |
| `protected_container` | container name (percent-encoded; Docker name grammar) |
| `max_ttl_s`, `max_active` | integer |
| `hub_addr` | ip |
| `link_peer` | ip, port |
| `local_addr` | ifaddr, ifname |
| `route` | family, table, cidr (destination), ip or `-` (gateway), ifname, metric |
| `rule` | family, priority, selector, table |
| `endpoint_route` | ip (endpoint), table, ip or `-` (gateway), ifname, ip (source) |

**`rule` selector.**
- It is `all` when the rule matches everything.
- Otherwise it is the items present, in this fixed order, joined by `;`: `from=<cidr>`, `to=<cidr>`,
  `iif=<ifname>`, `oif=<ifname>`, `fwmark=0x<hex>/0x<hex>` (lowercase hex, no leading zeros),
  `ipproto=<n>`, `sport=<a>-<b>`, `dport=<a>-<b>`, `uidrange=<a>-<b>`, `not`.
- The whole selector string is then percent-encoded as one field, so `=` becomes `%3D` and `;`
  becomes `%3B`.

**Unit canonicalisation.**
- A configured name without a unit-type suffix gets `.service` appended.
- Names are case-sensitive.
- Alias resolution is not part of the config hash. It happens at apply time.

## 3. Report layout and what each hash covers

The report keys appear in this order, which is unchanged from RW-37. Config lines carry the
effective configuration; route lines carry the attested network state.

```text
format, helper_key_id, nonce, boot_id, generation, config_hash, route_state_hash,
complete, incomplete_reason, computed_at,
cidr*, protected_unit*, protected_container*, max_ttl_s, max_active,      <- config lines
hub_addr*, link_peer*, local_addr*, route*, rule*, endpoint_route*        <- route lines
```

- **`config_hash`** = SHA-256 of the line `domovoi-safety-config-v1` (with its LF) followed by the
  config lines, in the order above, each LF-terminated.
- **`route_state_hash`** = SHA-256 of the line `domovoi-safety-routes-v1` (with its LF) followed by
  the route lines, in the order above, each LF-terminated.
- **Excluded from both:** `format`, `helper_key_id`, `nonce`, `boot_id`, `generation`, the two
  hashes themselves, `complete`, `incomplete_reason`, `computed_at`.
- **Hub check.** The hub recomputes both hashes from the report's own lines and rejects a mismatch.
- **Signed report.** The signature input stays the line `domovoi-safety-attest-v1` (with its LF)
  followed by the full report bytes (RW-37). The three prefixes are distinct.

**Effective configuration (`config_hash` input):**
- `cidr`: configured `management_cidrs`, normalised and de-duplicated.
- `protected_unit`: configured `protected_units`, canonicalised, **unioned with the mandatory set**
  compiled into the helper. The mandatory set is `domovoi-agent.service`, `domovoi-helper.socket`,
  `domovoi-skill-runner.socket` and `sshd.service`, and it cannot be removed.
- `protected_container`: configured names, de-duplicated. Names are resolved to ids at apply time.
- `max_ttl_s`: `[safety.containment] max_ttl` in seconds; default `3600` when missing.
- `max_active`: `[safety.containment] max_active`; default `0` when missing.
- Comments, whitespace, TOML key and table order, and duplicate list entries do **not** affect the
  hash, because only normalised values are hashed.
- A TOML file that fails to parse, or holds an invalid CIDR or name, yields **no** hash. The
  report is `complete=false`, `incomplete_reason=config_invalid`, and the helper refuses
  containment.

## 4. Test vectors

The inputs for C1–C3 are these TOML files (helper `[safety]` only).

C1 input:

```toml
[safety]
management_cidrs = ["192.168.1.7/24", "2001:DB8:0:0:0:0:0:0/32", "100.64.0.0/10"]
protected_units = ["plex", "sshd.service"]
protected_containers = ["homeassistant"]

[safety.containment]
max_ttl = "24h"
max_active = 50
```

C2 input (equivalent: reordered, duplicate entry, explicit suffixes, a mandatory unit listed, other
spellings):

```toml
# same effective configuration, written differently
[safety.containment]
max_active = 50
max_ttl = "1440m"

[safety]
protected_containers = ["homeassistant", "homeassistant"]
protected_units = ["sshd.service", "plex.service", "domovoi-agent.service"]
management_cidrs = ["100.64.0.0/10", "192.168.1.0/24", "2001:db8::/32"]
```

C3 is C1 with `100.64.0.0/10` replaced by `100.64.0.0/11`. C4 is `[safety]` with nothing else.

The route-state inputs are given directly as their canonical lines.

**Invalid-input vectors (must be rejected, no hash computed):**
- `local_addr=192.168.1.20/24|eth|0`: the raw `|` gives three fields where two are expected.
- `local_addr=192.168.1.20/24|eth%7c0`: lowercase hex.
- `local_addr=192.168.1.20/24|%65th0`: `%65` encodes `e`, which is in the literal set.
- `cidr=192.168.1.7/24`: host bits not zero in a report line. Normalisation happens before
  emitting, so the hub never normalises.
- `hub_addr=2001:DB8::10`: IPv6 not RFC 5952 lowercase.
- Two identical `route=` lines, or `route=` lines out of order.

### Config hash vectors

#### C1: representative `[safety]` (normalized effective configuration)

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-config-v1
cidr=100.64.0.0/10
cidr=192.168.1.0/24
cidr=2001:db8::/32
protected_unit=domovoi-agent.service
protected_unit=domovoi-helper.socket
protected_unit=domovoi-skill-runner.socket
protected_unit=plex.service
protected_unit=sshd.service
protected_container=homeassistant
max_ttl_s=86400
max_active=50
```

- Length: 320 bytes
- SHA-256: `96f1584f01f025fa82b1e57c3ebf8d220b87d6d88543d08add3dc273cbf4b9ea`

#### C2: semantically equivalent TOML rewrite of C1

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-config-v1
cidr=100.64.0.0/10
cidr=192.168.1.0/24
cidr=2001:db8::/32
protected_unit=domovoi-agent.service
protected_unit=domovoi-helper.socket
protected_unit=domovoi-skill-runner.socket
protected_unit=plex.service
protected_unit=sshd.service
protected_container=homeassistant
max_ttl_s=86400
max_active=50
```

- Length: 320 bytes
- SHA-256: `96f1584f01f025fa82b1e57c3ebf8d220b87d6d88543d08add3dc273cbf4b9ea`

#### C3: C1 with one CIDR narrowed (100.64.0.0/11)

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-config-v1
cidr=100.64.0.0/11
cidr=192.168.1.0/24
cidr=2001:db8::/32
protected_unit=domovoi-agent.service
protected_unit=domovoi-helper.socket
protected_unit=domovoi-skill-runner.socket
protected_unit=plex.service
protected_unit=sshd.service
protected_container=homeassistant
max_ttl_s=86400
max_active=50
```

- Length: 320 bytes
- SHA-256: `494a37a86856bb2b2f6e80e13efc654e8b7fbaf9e73245277f6d34993d42dc4f`

#### C4: empty `[safety]` (defaults and mandatory units only)

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-config-v1
protected_unit=domovoi-agent.service
protected_unit=domovoi-helper.socket
protected_unit=domovoi-skill-runner.socket
protected_unit=sshd.service
max_ttl_s=3600
max_active=0
```

- Length: 198 bytes
- SHA-256: `7f1db0e04eb12b4211166be055486a71e44be9e00c2a089cc9e51f4f06556baa`

### Route-state hash vectors

#### R1: two families, two tables, three interfaces, policy rules

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-routes-v1
hub_addr=192.168.1.10
hub_addr=2001:db8::10
link_peer=192.168.1.10|7443
local_addr=10.8.0.2/24|wg0
local_addr=192.168.1.20/24|eth0
local_addr=2001:db8::20/64|eth0
route=inet6|254|::/0|2001:db8::1|eth0|1024
route=inet|100|0.0.0.0/0|10.8.0.1|wg0|0
route=inet|254|0.0.0.0/0|192.168.1.1|eth0|100
route=inet|254|192.168.1.0/24|-|eth0|100
rule=inet6|32766|all|254
rule=inet|0|all|255
rule=inet|100|fwmark%3D0x1/0xffffffff|100
rule=inet|32766|all|254
endpoint_route=192.168.1.10|254|-|eth0|192.168.1.20
endpoint_route=2001:db8::10|254|2001:db8::1|eth0|2001:db8::20
```

- Length: 583 bytes
- SHA-256: `082d1afdf657096d1b3a97215142239f6910b74f13f158fbdda7f4d9fd4d1515`

#### R2: R1 with the table-100 default gateway replaced

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-routes-v1
hub_addr=192.168.1.10
hub_addr=2001:db8::10
link_peer=192.168.1.10|7443
local_addr=10.8.0.2/24|wg0
local_addr=192.168.1.20/24|eth0
local_addr=2001:db8::20/64|eth0
route=inet6|254|::/0|2001:db8::1|eth0|1024
route=inet|100|0.0.0.0/0|10.8.0.254|wg0|0
route=inet|254|0.0.0.0/0|192.168.1.1|eth0|100
route=inet|254|192.168.1.0/24|-|eth0|100
rule=inet6|32766|all|254
rule=inet|0|all|255
rule=inet|100|fwmark%3D0x1/0xffffffff|100
rule=inet|32766|all|254
endpoint_route=192.168.1.10|254|-|eth0|192.168.1.20
endpoint_route=2001:db8::10|254|2001:db8::1|eth0|2001:db8::20
```

- Length: 585 bytes
- SHA-256: `7eb46fcc67a00328df9e33578f42a4a890e6797223d6676a162c0b4ca1777ad9`

#### R3: empty route state (no lines)

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-routes-v1
```

- Length: 25 bytes
- SHA-256: `7947b31b2c89edab797c3f6a1251b42ca4452d64d06dd786d68570d721996ec8`

#### R4: delimiter and space in an interface name; compound rule selector

Hashed bytes (UTF-8, every line ends with LF, including the prefix line):

```text
domovoi-safety-routes-v1
hub_addr=192.168.1.10
local_addr=192.168.1.20/24|eth%7C0%20x
rule=inet|200|iif%3Dbr-lan%3Bnot|200
```

- Length: 123 bytes
- SHA-256: `530d9a9e286ca63ee69147d0a7e0326bceb4199f114824f1969e4126e7ac925a`

