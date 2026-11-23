# G-10 check: does escaping the interface name inside a selector item change the R4 vector?

Addendum to `03-r10-safety-attestation-synthesis.md` (G-10, E4) and to ADR 0032 (C4). It adds one
measurement and two host checks. Nothing here edits a design document. `04b` stays as written.

## 1. Question

`04b` §2 joins the selector items with `;` and percent-encodes the whole selector once. An interface
name may contain `;` and `=`, so the name `a;not` and the pair `iif=a`, `not` give the same bytes.
The synthesis proposes (G-10, E4) to encode the name with the strict field encoder before the items
are joined, and states that the R4 digest is unchanged. ADR 0032 C4 says the proposal changes R4.
Which is right?

## 2. Method

A scratch copy of the harness (`docs/rework/investigations/probes/r10`) was built in WSL (Ubuntu,
Rust, zig as C compiler). The repository copy was not touched. In the copy, `Selector::render`
percent-encodes the `iif` and `oif` names with `encode_field` before joining, and `Selector::parse`
decodes them strictly. Two runs of the same tests, one with the original `lines.rs` and one with the
change:

- `tests/encoding_vectors.rs` (C1 to C4, R1 to R4, compared with the fixtures byte for byte and with
  the published digests);
- a probe test that renders `rule=inet|200|<selector>|200` for a set of names, with `not` set.

## 3. Result: R4 and every other vector

Before (original harness) and after (inner escaping) are identical for all eight vectors:

| Vector | Length | SHA-256 (both runs) |
|---|---|---|
| C1, C2 | 320 | `96f1584f01f025fa82b1e57c3ebf8d220b87d6d88543d08add3dc273cbf4b9ea` |
| C3 | 320 | `494a37a86856bb2b2f6e80e13efc654e8b7fbaf9e73245277f6d34993d42dc4f` |
| C4 | 198 | `7f1db0e04eb12b4211166be055486a71e44be9e00c2a089cc9e51f4f06556baa` |
| R1 | 583 | `082d1afdf657096d1b3a97215142239f6910b74f13f158fbdda7f4d9fd4d1515` |
| R2 | 585 | `7eb46fcc67a00328df9e33578f42a4a890e6797223d6676a162c0b4ca1777ad9` |
| R3 | 25 | `7947b31b2c89edab797c3f6a1251b42ca4452d64d06dd786d68570d721996ec8` |
| R4 | 123 | `530d9a9e286ca63ee69147d0a7e0326bceb4199f114824f1969e4126e7ac925a` |

The harness printed `result=PASS` for all eight in both runs. R4's rule line, before and after, is:

```text
rule=inet|200|iif%3Dbr-lan%3Bnot|200
```

The reason is in the vector itself. The delimiter-and-space name of R4 (`eth|0 x`) is in the
`local_addr` line, which the proposal does not touch. The selector of R4 uses `br-lan`, whose bytes
are all in the literal set, so the inner encoding returns them unchanged.

## 4. How the inner escape treats names

Line shown is the full `rule=` value with `not` set (`inet|200|<selector>|200`).

| Name | Inner encoding | Line before | Line after |
|---|---|---|---|
| `br-lan` | `br-lan` | `iif%3Dbr-lan%3Bnot` | same |
| `a;not` | `a%3Bnot` | `iif%3Da%3Bnot%3Bnot` | `iif%3Da%253Bnot%3Bnot` |
| `a\|b` | `a%7Cb` | `iif%3Da%7Cb%3Bnot` | `iif%3Da%257Cb%3Bnot` |
| `a%b` | `a%25b` | `iif%3Da%25b%3Bnot` | `iif%3Da%2525b%3Bnot` |
| `a b` | `a%20b` | `iif%3Da%20b%3Bnot` | `iif%3Da%2520b%3Bnot` |
| `caf` + U+00E9 | `caf%C3%A9` | `iif%3Dcaf%C3%A9%3Bnot` | `iif%3Dcaf%25C3%25A9%3Bnot` |
| `a=b` | `a%3Db` | `iif%3Da%3Db%3Bnot` | `iif%3Da%253Db%3Bnot` |
| `eth\|0 x` | `eth%7C0%20x` | `iif%3Deth%7C0%20x%3Bnot` | `iif%3Deth%257C0%2520x%3Bnot` |
| empty | (empty) | `iif%3D%3Bnot` | same |
| 15 bytes (`abcdefghijklmno`) | unchanged | accepted | same |
| 16 bytes | unchanged | `check_ifname` refuses | same |
| fifteen `;` | 45 bytes | line cannot be parsed back | line parses back |

Observations:

- A name with any escaped byte is encoded twice on the wire (`%` becomes `%25`). This is a change for
  those names only. Names made of letters, digits and `. _ : / @ -` do not change.
- Injectivity: before, `a;not` (no `not` flag) and `iif=a` plus `not` both gave
  `rule=inet|200|iif%3Da%3Bnot|200`, and `x;oif=y` and the pair `iif=x`, `oif=y` both gave
  `rule=inet|200|iif%3Dx%3Boif%3Dy|200`. After, the pairs differ
  (`iif%3Da%253Bnot` against `iif%3Da%3Bnot`; `iif%3Dx%253Boif%253Dy` against `iif%3Dx%3Boif%3Dy`).
- Round trip: before, `render(parse(x)) == x` failed for names holding `;` (the selector parser
  splits on `;`). After, it holds for every name above except the empty one.
- The 15-byte limit applies to the decoded name (`check_ifname`), not to the encoded text. A 15-byte
  name of `;` becomes 45 bytes in the selector and 105 bytes in the line. The encoded size is not
  bounded by 15; the report cap is 64 KiB (synthesis X-6).
- Empty name: `Selector::parse` refuses `iif=` with an empty value. The harness can render such a line
  but cannot read it back, in both runs. A rule with an empty interface name does not exist in the
  kernel; the check belongs in the observer, not in the encoder.
- The harness carries `iif` and `oif` as `String`. Non-UTF-8 names (legal in the kernel) cannot be
  represented without changing the type to bytes (synthesis X-8). Not changed in the scratch copy.
- `eth|0 x` is not kernel-legal (see 5.1: the `ip` tool refuses a space); R4 uses it for the encoder,
  not as a realistic name.

## 5. Two beliefs the synthesis took from general knowledge

### 5.1 Interface names in the kernel

Host: WSL 2 Ubuntu (systemd 259), `sudo -n` available. Each name was created with
`ip link add "<name>" type dummy` inside `sudo -n unshare -n` (a throwaway network namespace that
disappears with the shell), then deleted. `ip netns list` was empty before and after.

| Name | Exit code | Output |
|---|---|---|
| `a:b` | 2 | `RTNETLINK answers: Invalid argument` |
| `a;b` | 0 | |
| `a\|b` | 0 | |
| `a%b` | 2 | `RTNETLINK answers: Invalid argument` |
| `a b` | 255 | `Error: argument "a b" is wrong: "dev" not a valid ifname` |
| `a/b` | 255 | `Error: argument "a/b" is wrong: "dev" not a valid ifname` |
| `a\b` (backslash) | 0 | |
| `a=b` | 0 | |
| `a,b` | 0 | |
| `.` and `..` | 2 | `RTNETLINK answers: Invalid argument` |
| `ab` | 0 | |

So the belief holds: the kernel rejects `:` (exit 2 from the netlink call). The same check rejects
`%` in this name, and `.` and `..`. `;`, `|`, `=`, `,` and a backslash are accepted, so the
ambiguity of G-10 is real: `a;not` is a name the kernel allows. Space and `/` were refused by the
`ip` program before the request reached the kernel, so this run does not show what the kernel does
with them (it is known to refuse both, but that was not tested here).

Consequence: `:` and `%` are in the literal-or-escaped set of `04b` but cannot occur in a real name.
`:` is also a legal literal character in the `04b` field grammar, so a name containing it would be
written unescaped; the kernel rules that case out.

### 5.2 Backslash in a systemd unit name

Run in a temporary directory under `$HOME`; no unit was installed.

```text
$ systemd-escape 'a\b'
a\x5cb
$ systemd-escape --mangle 'a\b'
a\b.service
$ systemd-escape --path /srv/a-b/c
srv-a\x2db-c
$ systemd-escape --unescape 'srv-a\x2db-c'
srv/a-b/c
```

`systemd-analyze verify` on a file with a literal backslash in its name (`a\b.service`, one
backslash, minimal `[Service] ExecStart=/bin/true`) printed nothing and returned 0. For comparison,
`a b.service` and `a;b.service` returned 1 (`Failed to prepare filename ... Invalid argument`), and a
missing unit returned 1 (`Unit nonexist.service not found.`), so the command does detect problems.

So the belief holds: systemd accepts `\` in a unit name, and the output of `systemd-escape` itself
contains `\x2d`-style sequences. The harness grammar for unit names (`[A-Za-z0-9:_.@-]`) would
refuse every escaped name as `config_invalid` (synthesis X-9). The `04b` field encoding handles
`\` without trouble (`%5C`); only the unit-name grammar needs the character.

## 6. Conclusion

- The claim of the synthesis holds: the inner escaping leaves R4 and the other seven vectors
  unchanged, byte for byte and by digest. ADR 0032 C4 is wrong on this point: the R4 vector does not
  change, because R4's selector name is made of literal characters. (Its other statement, that the
  selector needs per-item escaping, is confirmed: the ambiguity was reproduced.)
- The change is a wire change for any rule whose selector name holds a byte outside the literal set:
  the first `%` of the inner encoding becomes `%25`. Such reports have never been signed in the
  field, so the change is free until the first release; after that it is a format change (`04b`
  protocol rule R14).
- For ADR 0032: item C4 can be taken with "escape the interface name inside the item", and the line
  "changes the R4 vector" can be removed. The test names the synthesis proposes
  (`selector_encoding_is_injective_for_names_with_separators`, `r4_digest_unchanged_by_inner_escaping`)
  match what was run here.
- For synthesis G-10 and E4: the "should not change, to be confirmed by running" can become "confirmed
  by running" with the numbers above. The remark about bytes (X-8) stays open.

## 7. Not verified

- Only `iif` was exercised in the probe. `oif` shares the code path in the scratch copy but was not
  rendered separately.
- The scratch change was limited to render and parse in `lines.rs`. The rest of the harness tests
  (for example parser strictness, which refers to the selector ambiguity) were not re-run with the
  change, so whether any of them asserts the old ambiguity is unknown.
- The kernel behaviour for a space or `/` in an interface name was not reached (the `ip` tool refuses
  first). Names longer than 15 bytes, non-UTF-8 names and altnames were not tried on the host.
- Only one host and kernel (WSL 2) and one systemd (259) were used. `systemd-analyze verify` does not
  prove that a manager accepts and runs a unit with a backslash in its name.
- A selector that has both `iif` and `oif` with escaped bytes was shown only through the `x;oif=y`
  example, not as a general injectivity proof. The encoder is the strict percent encoding, which is
  injective by itself, and the items are separated by a byte that the encoding never emits raw, but
  that argument was not checked by a property test.
