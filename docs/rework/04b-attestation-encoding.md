# 04b — Helper safety attestation: encoding, hashes and test vectors

Byte-level contract for RW-37. Status: **PROPOSAL**, part of R-10. Nothing here is implemented. The
vectors below were computed with a reference script (Python `tomllib`, `ipaddress`, `hashlib`) from
exactly the bytes shown. An implementation must reproduce every digest.

## 1. Field encoding

- **Lines.** UTF-8, each terminated by a single LF (0x0A). No CR, no trailing spaces, no blank
  lines, no BOM. A decoder rejects a report whose bytes are not valid UTF-8 (kept as an explicit
  rule step so that the contract's rule list stays complete and testable; synthesis 03 D12).
- **Line grammar.** `line = key "=" value`. `key` is one of the fixed keys listed in §3. `value`
  is one or more fields joined by `|` (0x7C). The one exception is `incomplete_reason`, whose
  value may be empty, and which is empty if and only if `complete=true`: the verifier rejects an
  empty reason on an incomplete report and a non-empty reason on a complete one (synthesis 03 D1,
  ADR 0032 C1).
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

**Value grammars (amended; synthesis 03 section 5.2 and 5.3, owner decision D9).** "Decoded" means
after percent-decoding.
- `table`, `priority`, `metric`: canonical decimal `u32` (at most 4294967295).
- `port`: canonical decimal, 1 to 65535.
- `ifname`: 1 to 15 decoded octets, carried as bytes everywhere (selector, `local_addr`, `route`,
  `endpoint_route`). The wire grammar does not enforce the kernel's own name rules (no `/`, no
  whitespace, no `:` or `%` in practice, not `.` or `..`) so that vector R4 stays a valid encoding
  vector; the helper emits only names the kernel returned. Measured on one host: the kernel refuses
  `:`, `%`, `.` and `..`, and accepts `;`, `|`, `=`, `,` and a backslash (synthesis 06 section 5.1).
- `ipproto`: 0 to 255. `sport`, `dport`: `a-b` with each at most 65535 and `a <= b`.
  `uidrange`: `a-b` with each at most 4294967295 and `a <= b`. `fwmark` values: lowercase hex, no
  leading zeros, at most 8 digits.
- `max_ttl` (helper configuration): digits then one of `s`, `m`, `h`, `d`; lowercase, no sign, no
  sum, value at least 1 (`0` is `config_invalid`), seconds must fit in 64 bits.
- `max_active`: non-negative integer; `0` means no containment is allowed; default `0`.
- Unit names: 1 to 255 characters from `A-Z a-z 0-9 : _ . @ -` with a known type suffix (`service`,
  `socket`, `target`, `mount`, `automount`, `swap`, `timer`, `path`, `slice`, `scope`, `device`);
  case-sensitive. The backslash is not in the set: systemd accepts it in unit names (the output of
  `systemd-escape` contains it), so escaped names are refused as `config_invalid` until the owner
  widens the grammar (synthesis 03 X-9, synthesis 06 section 5.2).
- Container names: `[A-Za-z0-9][A-Za-z0-9_.-]*`, at most 255 characters. Names stay names; ids
  resolve at apply time.
- Unknown keys under `[safety]` are refused (`config_invalid`); a missing `[safety]` equals an
  empty one.
- Hash fields in `expected_safety` (`config_hash`, `route_state_hash`, `fingerprint`): 64 lowercase
  hex characters; `boot_id`: lowercase UUID form.

**`rule` selector.**
- It is `all` when the rule matches everything.
- Otherwise it is the items present, in this fixed order, joined by `;`: `from=<cidr>`, `to=<cidr>`,
  `iif=<ifname-e>`, `oif=<ifname-e>`, `fwmark=0x<hex>/0x<hex>` (lowercase hex, no leading zeros),
  `ipproto=<n>`, `sport=<a>-<b>`, `dport=<a>-<b>`, `uidrange=<a>-<b>`, `not`.
- **Inner escaping.** `ifname-e` is the interface-name bytes encoded with the strict field encoder
  **before** the items are joined by `;`, so a name holding `;` or `=` cannot be read as another
  item (`a;not` and the pair `iif=a`, `not` were byte-identical without it). Names made only of
  literal characters are unchanged, so every vector below, R4 included, is unchanged (confirmed by
  running them; synthesis 06 section 3; ADR 0032 C4, synthesis 03 D4).
- The whole selector string is then percent-encoded as one field, so `=` becomes `%3D` and `;`
  becomes `%3B`. A name with an escaped byte is therefore encoded twice on the wire (`%` becomes
  `%25`); the 15-byte limit applies to the decoded name, not to the encoded text.

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
  report is the minimal incomplete report below, with `incomplete_reason=config_invalid`, and the
  helper refuses containment.
- **Routing state that cannot be represented** (a policy rule whose action is not lookup, a
  selected route whose type is not unicast) is never hashed with the item omitted: the helper emits
  an incomplete report with `incomplete_reason=route_unrepresentable`, and containment is refused
  on that host (synthesis 03 G-8, D15).
- Loopback and link-local addresses are excluded by helper code and are not in the report.

**Complete and incomplete reports (synthesis 03 D2, D3, section 5.4).**
- A report with `complete=true` has non-zero hashes equal to the recomputation from its own lines,
  `generation >= 1` and an empty `incomplete_reason`.
- A report with `complete=false` has exactly this fixed shape and no repeated lines. The hub
  requires the shape, skips hash recomputation, and uses nothing from it but the reason token and
  the receipt time:

```text
format=1
helper_key_id=<hex16>
nonce=<hex64>
boot_id=<boot-id>
generation=0
config_hash=0000000000000000000000000000000000000000000000000000000000000000
route_state_hash=0000000000000000000000000000000000000000000000000000000000000000
complete=false
incomplete_reason=<token>
computed_at=<timestamp>
max_ttl_s=0
max_active=0
```

- `incomplete_reason` is a lowercase token of 1 to 64 characters from `a-z 0-9 _`. Contract tokens:
  `state_missing`, `state_corrupt`, `state_key_mismatch`, `config_invalid`, `lookup_failed`;
  `route_unrepresentable` is added by the decision above.
- **Generation.** `init` writes the state file with `generation=0` and zero hashes (no observer or
  valid configuration is needed at install or recovery); the first complete attestation is
  generation 1. A complete report with generation 0 is invalid.
- **Retired keys.** `rotate-key` and `recover` delete the old private key once the new files are
  durable; the old public key, fingerprint and state are kept for audit (synthesis 03 D14).

## 4. Test vectors

The vectors prove encoding, sorting and hashing only. R1 contains a `local_addr` line (`wg0`) that no
`endpoint_route` names, so it is not a valid output of the route-selection rule, and R4 uses an
interface name (`eth|0 x`) that the kernel would refuse; selection logic is covered by separate
semantic vectors (snapshot to expected lines), not by these digests (synthesis 03 G-7).

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

### Selector escaping vectors (line level)

Rendered `rule=` values (family `inet`, priority 200, table 200, `not` set) before and after inner
escaping, as produced by the scratch run of synthesis 06 section 4. Digests were not computed for
these lines; only the lines are frozen here.

| Interface name | Line without inner escaping | Line with inner escaping (normative) |
|---|---|---|
| `br-lan` | `rule=inet\|200\|iif%3Dbr-lan%3Bnot\|200` | same (R4 unchanged) |
| `a;not` | `rule=inet\|200\|iif%3Da%3Bnot%3Bnot\|200` | `rule=inet\|200\|iif%3Da%253Bnot%3Bnot\|200` |
| `a=b` | `rule=inet\|200\|iif%3Da%3Db%3Bnot\|200` | `rule=inet\|200\|iif%3Da%253Db%3Bnot\|200` |

The first column shows the ambiguity: a rule with `iif=a` and the `not` flag renders as
`rule=inet|200|iif%3Da%3Bnot|200`, which equals the name `a;not` without the flag. With inner
escaping the two differ (`iif%3Da%253Bnot` against `iif%3Da%3Bnot`). Named tests proposed in
synthesis 03: `selector_encoding_is_injective_for_names_with_separators`,
`r4_digest_unchanged_by_inner_escaping`.

## 5. Hub verification rules that depend on the encoding

These restate the rules of RW-37 as amended by the owner decisions of synthesis 00 section 5a
(synthesis 03 sections 5.7 and 5.8).

- **Nonce.** The hub issues 32 random bytes bound to a host and stores the issue time. A nonce is
  valid for under 60 seconds (strict boundary). It is consumed late: only after the signature,
  parse and hash checks pass, so an unauthenticated message cannot burn it.
- **Freshness.** A verified attestation is fresh for under 300 seconds measured from the nonce
  **issue** time on the hub's own clock, not from receipt (strict boundary), so the observed state
  is bounded by the nonce lifetime plus the freshness window. `helper_key_id` is not cross-checked.
- **Wire tokens.** `unverified.reason` stays the closed set of nine: `key_not_pinned`,
  `key_mismatch`, `attestation_missing`, `attestation_stale`, `signature_invalid`,
  `report_incomplete`, `generation_regressed`, `path_uncovered`, `confirmation_stale`. Finer
  detail stays in the hub's audit log and is never put on the wire.
- **Key mismatch** is gated on the latest outcome: blocked while the latest received attestation
  was a mismatch, cleared by the next one that verifies under the pin. A signed incomplete report
  from the pinned key and a generation regression also revoke verified state.
- **High-water mark.** The hub stores the highest accepted `(generation, config_hash,
  route_state_hash)` per helper, in the transaction that accepts the report, and rejects a lower
  generation or an equal generation with different hashes (`generation_regressed`).
- **`expected_safety`** is required on every `Contain` request; the helper refuses a missing or
  malformed tag. The wire form is a closed tag with `deny_unknown_fields` and an enum for reasons.
  A TTL above the effective maximum is refused, not clamped; `max_active=0` allows no containment.
