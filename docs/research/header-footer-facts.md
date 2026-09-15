# Kickstart ROM header/footer facts — Milestone 1

Established by (a) reading permissively-licensed/public documentation and
(b) treating amitools' `romtool` (GPL-3) purely as a black-box differential
oracle: feeding it synthetic images built from scratch and observing which
of its printed checks flip. **No amitools `.py` source was read** at any
point in this work (not `KickRomAccess.py`, not `RomImage.py`, nothing
under `amitools/rom/`) — only the installed `romtool` CLI binary was
invoked, and real Kickstart ROM byte values already present on this
machine were read locally (never included here beyond the small field
values documented below).

**Tooling**: `python3 -m pip install --user 'amitools==0.8.1'` into a venv
under the task scratchpad; all commands below are `romtool info <path>`
(and default subcommands) from that install.

**Real ROMs used** (all pre-existing on this machine, legally the owner's
own dumps — identified by public version numbers only, no content
reproduced beyond a handful of individual field values):
- `~/kicksmash32/amiga-os-130-a3000.rom` — Kickstart 1.3 (34.5), 256 KiB
- `~/src/external/Copperline/test-assets/KICK13.ROM` — same 1.3 image,
  padded/duplicated to 512 KiB (kickety-split layout)
- `~/Downloads/kickstart-1.3.rom` — a second, independently-sourced 1.3
  256 KiB dump (used only for the checksum cross-check)
- `~/kicksmash32/amiga-os-140-a3000.rom` — Kickstart 1.4 beta (36.16), 512 KiB
- `~/kicksmash32/amiga-os-204-a3000.rom` — Kickstart 2.04 (37.175), 512 KiB
- `~/kicksmash32/amiga-os-310-a3000.rom` — Kickstart 3.1 (40.68), 512 KiB
- `~/kicksmash32/amiga-os-3x0-a3000.rom` — Kickstart 3.0-ish (45.66), 512 KiB
- `~/kicksmash32/kick322.A3000.47.111.rom` — Kickstart 3.2.2 (47.111), 512 KiB
- `~/src/amitools/test/roms/aros-20130502.rom` — AROS ROM (46.10), 512 KiB
  (AROS's own binary test fixture; not a Commodore/Amiga ROM, useful as a
  "deliberately different" cross-check)

Synthetic images were built entirely in a scratch Python script
(`build_rom2.py`) from the hypotheses below, with no reference to any GPL
source — only to the byte values already independently observed in the
real ROMs above and to the public documentation cited per section.

---

## 1. ROM header layout

**Fact.** The header occupies the first 0x18 (24) bytes:

| Offset | Width | Content |
|---|---|---|
| 0x00 | u16 BE | Header-type marker word. Small enumerated set of values seen across real ROMs (`0x1111`, `0x1114`); exact enumeration is the separate "boot-vector signature table" milestone-1 item (kicksmash32, BSD-2). Not a checksum or era code — see oracle result below. |
| 0x02 | u16 BE | `0x4EF9` — m68k opcode for `JMP <abs.L>`. Required exactly; any other value fails the header check. |
| 0x04 | u32 BE | Absolute jump target = **`boot_pc`**. This is the whole first instruction: `JMP $boot_pc`. |
| 0x08 | u32 BE | Constant `0x0000FFFF` in every real ROM sampled (1.3/256K, 1.3/512K-padded, 2.04, 3.1, AROS). Meaning not established (not exercised by the header oracle test); role UNRESOLVED. |
| 0x0C | u16+u16 BE | **`rom_rev`**: major, minor. |
| 0x10 | u16+u16 BE | **`exec_rev`**: major, minor. |
| 0x14 | u32 BE | Constant `0xFFFFFFFF` in every real ROM sampled — separator before the ASCII banner/copyright string that follows at 0x18. |

**`base_addr` derivation.** `base_addr` is **not** stored separately and
is **not** looked up from file size/era. It is computed as the top 16
bits of the `boot_pc` JMP target: `base_addr = boot_pc & 0xFFFF0000`.
Confirmed by oracle: synthetic images with the JMP target's high word set
to `0x00FC`, `0x00F8`, and `0x00F0` all report `base_addr` equal to that
value shifted, regardless of whether the synthetic image was 256 KiB or
512 KiB — i.e. the 256 KiB-@-0xFC0000 vs 512 KiB-@-0xF80000 split is a
*consequence* of which address a given ROM's linker chose, not a rule the
checker applies from file size. Real ROMs confirm the two conventional
eras (1.3/256K → `0x00FC0000`; 2.04/3.1/3.2.2 → `0x00F80000`), and also
show the rule generalizes: a genuine 1.4-beta dump in the wild
(`amiga-os-140-a3000.rom`) reports `base_addr 00FF0000` — a third, oddball
value that the checker still computes correctly from the JMP target
(that ROM fails `is_kick` for unrelated reasons — see §2/§4).

`boot_pc` is read directly as the 32-bit JMP target — no separate
"boot_pc field" exists; it *is* offset 0x04's value.

**Oracle experiments (offset 0x00 marker word):**
- `first_word = 0x1114`, size 256 KiB or 512 KiB, any `base_hi`: `header: ok`.
- `first_word = 0x1111`, size 256 KiB: `header: ok` (matches real 1.3 ROM).
- `first_word = 0x1111`, size 512 KiB: `header: NOK` — i.e. `0x1111` is
  *not* universally accepted; it depends on image size, not just being in
  some flat allow-list. `0x1114` was accepted at every size tried.
- `first_word = 0x2222/0x9999/0x1234/0x0000/0xFFFF`: `header: NOK` at
  every size tried.
- Opcode at 0x02 changed from `0x4EF9` to `0x4E71` (NOP): `header: NOK`.
- Mutating the real 1.3 ROM's own first word from `0x1111` to `0x1114`
  in place: `header` stays `ok` (only `is_kick` flips, because the
  mutation broke the stored checksum) — confirms both values are
  independently valid, and that this specific marker word, not the rest
  of the header, is what's being probed.

**Sources.** Structural facts (JMP-opcode header, revision word pairs,
base-address-from-vector convention) match the well-known community
description of the Kickstart ROM header format (e.g. as referenced from
kicksmash32's `rom_signature`-style tables and multiple Amiga ROM
write-ups); the *exact* offsets and widths above are established here
independently via the oracle, not copied from any such write-up.

**Confidence: CONFIRMED** for offsets 0x00–0x17, `boot_pc`, and
`base_addr` derivation (each backed by ≥2 real ROMs plus oracle mutation).
**UNRESOLVED**: semantic meaning of the constant `0x0000FFFF` at 0x08 (its
presence/format is confirmed, its purpose is not).

---

## 2. ROM footer layout

**Fact.** The footer occupies the last 24 bytes of the image:

| Offset from end | Width | Content |
|---|---|---|
| `len-24` | u32 BE | Stored **checksum** (see §3). |
| `len-20` | u32 BE | **Size field**: expected to equal the image's actual byte length (`0x00040000` for 256 KiB, `0x00080000` for 512 KiB). Confirmed against every real ROM sampled. |
| `len-16` .. `len-1` | 8 × u16 BE | Trailing "vector" words. In every 512 KiB 2.0+-era ROM sampled (2.04, 3.1, 3.2.2), these are exactly `0x0018, 0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F` — i.e. the m68k exception-vector *indices* 24–31 (spurious interrupt + the seven autovectors) written as literal data, not the vector table itself. |

**1.x/256 KiB deviation.** In both 1.3 dumps (256 KiB original and the
512 KiB kickety-split-padded copy), the **first** of those eight words
(`len-16`) is `0xFFC0`, not `0x0018` — the remaining seven words
(`0x0019`..`0x001F`) are present and correct. `romtool` still reports
`footer: ok` for these ROMs, so the footer check tolerates variation in
that first word.

**Oracle isolation of what the footer check actually requires**, on a
synthetic 512 KiB image with all eight trailing words correct:
- Corrupt only the **last** word (`len-2`, i.e. the `0x001F` slot) →
  `footer: NOK`.
- Corrupt only a **middle** word (`len-8`, the `0x001C` slot) →
  `footer: NOK`.
- Corrupt only the **first** word (`len-16`, the `0x0018` slot) →
  `footer: ok` (unchanged) — consistent with the real 1.3 ROMs' own
  deviation there.

So the footer check validates (at least) the seven trailing words from
`len-14` through `len-1`, and is indifferent to the word at `len-16`.

**`is_kick` conjunction gotcha (footer/size-field interaction).** The
printed `size_field` status line **never flipped to `NOK`** in any
mutation tried — values `0`, `1`, `0x12345678`, `0xFFFFFFFF`, and every
size not matching the real file length all still printed
`size_field: ok`. Yet `is_kick` correctly went `NOK` whenever the stored
size field did not equal the image's real length, and `chk_sum` stayed
`ok` regardless (because the checksum is sealed over the whole buffer
independent of what the size field claims). **This means `is_kick`'s
conjunction includes a requirement — stored size field equals actual
image length — that is not exposed by any individually-printed check
line**, `size_field` included. This is a genuinely surprising finding for
milestone 2's `is_kick_rom` transcription: it cannot be implemented as
"AND of the 8 printed booleans" alone.

**Sources.** Same community description referenced in §1 for the general
shape (size field + checksum + trailing vector-index words); the
exact tolerance behaviour (which of the 8 trailing words matter) and the
`is_kick`/`size_field` discrepancy are established here purely via the
oracle, not from any write-up.

**Confidence: CONFIRMED** for offsets, widths, and the "words len-14..len-1
must be exact, len-16 doesn't matter" tolerance (2+ real ROMs, oracle
isolation). **CONFIRMED** (surprising) for the size-field/`is_kick` gap.

---

## 3. Stored-checksum location and KickSum convention

**Fact.** The stored checksum is a big-endian `u32` at file offset
`len-24` (immediately preceding the size field described in §2).

**Convention, verified empirically, not quoted from any write-up:**
computing the end-around-carry ones'-complement sum of the *entire*
image (all bytes, consecutive big-endian `u32` words, including the
checksum field itself) yields exactly `0xFFFFFFFF` for every real ROM
tested:

| ROM | end-around-carry sum of whole image | stored checksum |
|---|---|---|
| 1.3 / 256 KiB (`amiga-os-130-a3000.rom`) | `0xFFFFFFFF` | `0x150B7DB3` |
| 1.3 / 256 KiB (`~/Downloads/kickstart-1.3.rom`, independent dump) | `0xFFFFFFFF` | `0x15267DB3` |
| 2.04 / 512 KiB | `0xFFFFFFFF` | `0x54876DAB` |
| 3.1 / 512 KiB | `0xFFFFFFFF` | `0x8F4C0C67` |
| AROS / 512 KiB | `0xFFFFFFFF` | `0x707E5FE9` |

The "negated-sum variant" mentioned as an alternative hypothesis in
`PLAN.md` was checked directly rather than assumed distinct: zeroing the
stored checksum field, re-summing the rest of the image with the same
end-around-carry algorithm, and taking the bitwise complement of that sum
reproduces the stored checksum value exactly, for all five files above.
**These are the same formula, not two competing conventions** — end-
around-carry ones'-complement addition makes "total sum == 0xFFFFFFFF"
and "stored value == complement of the sum of everything else"
algebraically identical (no wraparound edge case separates them, since
the partial sum is always in `0..=0xFFFFFFFF`). `PLAN.md`'s existing
`checksum_ones_complement` primitive is exactly this operation and needs
no variant branch.

Oracle cross-check: flipping a single bit anywhere in the stored
checksum field of a synthetic (correctly-sealed) image flips `chk_sum`
to `NOK` and changes the reported `check_sum` value by exactly that bit,
confirming `romtool`'s own reported `check_sum` is this same longword
read back, not a recomputation presented differently.

**Sources.** SKick's `.RTB` files (BSD/no-reuse-but-observable per
`PLAN.md`) are noted there as opening with "a 4-byte KickSum header" —
consistent with this being the same well-known checksum, though not
separately re-verified here (out of scope: this fact was to be nailed
empirically against real ROMs, which is done above).

**Confidence: CONFIRMED** — 5 independent real ROM files, both formulas
shown algebraically and numerically identical, oracle bit-flip cross-check.

---

## 4. "Kickety split"

**Fact.** `kickety_split` checks for a **second copy of the exact same
two header fields examined in §1** (marker word + JMP opcode), located
at the image's exact **midpoint** (`offset = size / 2`, e.g. `+0x40000`
on a 512 KiB image) rather than at offset 0.

Oracle isolation on a synthetic 512 KiB image (real header already valid
at offset 0):
- Writing `0x1111` at `mid+0x00` and `0x4EF9` at `mid+0x02` (with any
  address, even a bogus one, at `mid+0x04`) → `kickety_split: ok`.
- Writing `0x1114` (the *other* value that's valid at offset 0!) at
  `mid+0x00`, real JMP opcode at `mid+0x02` → `kickety_split: NOK`.
  **So the midpoint check is stricter than the main header check: only
  `0x1111` counts, not the full set of values §1 accepts at offset 0.**
- Omitting the JMP opcode at `mid+0x02` (leaving `0x0000`) → `NOK` even
  with `0x1111` present.
- The address word at `mid+0x04` is **not checked** at all — a
  deliberately wrong target (pointing into the *other* ROM half's
  address range) still yields `kickety_split: ok`.

**Which real ROMs have it.** Sampled: `ok` on the 1.3 image padded/
duplicated to 512 KiB (`KICK13.ROM` — unsurprising, its second half is a
literal copy of its own header) and on the 3.1 (40.68) ROM; `NOK` on
2.04 (37.175), 3.0-ish (45.66), and 3.2.2 (47.111). This does **not**
correlate cleanly with "1.x vs 2.0+" as a rule — 3.1 has it, 2.04/3.0/3.2
sampled here don't — it is simply whatever bytes happen to sit at that
ROM's own midpoint; some AmigaOS 2.0+ 512 KiB ROMs (3.1 here) happen to
have been built in a way that leaves a valid-looking header signature
there (plausibly a genuine leftover from a two-256K-chip layout on that
particular ROM revision/vendor build), others don't.

**Not part of `is_kick`.** Confirmed directly: the 2.04 real ROM has
`kickety_split: NOK` and `is_kick: ok` simultaneously; synthetic images
with `kickety_split` forced `NOK` (no midpoint header at all) still show
`is_kick: ok` whenever header/footer/size-field/checksum are otherwise
valid. This matches `PLAN.md`'s expectation that `romtool`'s own docs
describe kickety-split as informational, not gating.

**Sources.** Working hypothesis in `PLAN.md` (second header at the
midpoint of a 512 KiB image); confirmed and made precise (exact bytes,
exact tolerance, non-membership in `is_kick`) purely via the oracle above.

**Confidence: CONFIRMED** for the mechanism (midpoint offset, exact two
fields checked, address not checked, exclusion from `is_kick`).
**PROBABLE** (not fully resolved) for "which ROM eras have it" as a
general rule — the sample here (5 ROMs) shows it's not era-determined,
but a broader survey would be needed to characterize it further; not
blocking, since the mechanism itself is settled.

---

## 5. Magic reset opcode

**Fact.** `magic_reset` checks for the m68k `RESET` opcode, `0x4E70`, at
the **fixed absolute file offset `0x000000D0`** — independent of
`base_addr` and independent of `boot_pc`'s value.

Real ROMs: all four distinct-content ROMs sampled (1.3/256K, 2.04, 3.1,
AROS) have `0x4E70` at exactly offset `0xD0`, immediately followed at
`0xD2` by the address that `boot_pc` happens to point at in three of the
four (1.3, 2.04, 3.1 all have `boot_pc = base_addr + 0xD2`; AROS's
`boot_pc = base_addr + 0xF8` is different, showing the RESET-at-0xD0
convention holds even when `boot_pc` doesn't literally point right after
it).

Oracle: moving the synthetic image's `boot_pc` JMP target far away
(`base_addr + 0x120`) while leaving the `RESET` opcode at `0xD0` →
`magic_reset: ok` unchanged (rules out "checked relative to boot_pc").
Removing the `0x4E70` opcode from `0xD0` entirely (`place_reset=False`,
nothing else changed) → `magic_reset: NOK`.

**Not part of `is_kick`.** Confirmed directly: forcing `magic_reset: NOK`
in an otherwise-valid synthetic image left `is_kick: ok`.

**Sources.** `PLAN.md`'s working hypothesis (`m68k RESET = 0x4E70`)
confirmed as exactly correct; the fixed offset `0xD0` and non-membership
in `is_kick` are new findings from the oracle, not previously stated in
`PLAN.md`.

**Confidence: CONFIRMED** — 4 real ROMs + 2 independent oracle mutations
(remove-opcode, move-boot_pc-away).

---

## 6. Where `exec_rev` comes from

**Fact.** `exec_rev` is read from the **fixed header offset `0x10`**
(the same offset established in §1), not walked from exec.library's
`Resident`/version structures.

**Oracle test (the decisive one for this fact, per the task's
instructions — decided empirically, no GPL source read):**
1. Built three synthetic images differing *only* in the two `u16` words
   at offset `0x10` (`(1,2)`, `(99,255)`, `(0,0)`), everything else
   (including `rom_rev` at `0x0C`) held constant. `romtool`'s reported
   `exec_rev` tracked the field 1:1 in all three cases (`1.2`, `99.255`,
   `0.0`).
2. Built a synthetic image with a normal, valid header (`exec_rev =
   40.10` at offset `0x10`), then additionally injected a fake `Resident`
   structure elsewhere in the image (`RTC_MATCHWORD 0x4AFC` at an
   arbitrary offset, a self-referencing `rt_MatchTag` pointer, and a
   bogus `rt_Version` byte of `99` at the structure's version-field
   position) — i.e. exactly the kind of structure a resident-walking
   reader would need to find and would report `99` from. `romtool`
   still reported `exec_rev: 40.10`, completely unaffected by the planted
   fake resident.
3. Same 1:1-tracking test repeated for `rom_rev` at offset `0x0C`
   (values `1.2` and `99.255`), confirming the sibling field behaves
   identically.
4. Cross-checked against 5 real ROMs (§1 table): the header-offset value
   equals `romtool`'s reported `exec_rev` for every one, including the
   AROS ROM where `rom_rev` and `exec_rev` happen to be numerically
   identical (`46.10`/`46.10`) — a case that would be easy to
   misattribute to a shared/derived source if only one ROM were checked,
   which is why the multi-ROM + injected-fake-resident combination above
   was needed to fully rule out resident-walking.

**Consequence for the plan.** Per `PLAN.md`'s own branching instruction:
since this is fixed-offset, **`exec_rev` belongs in milestone 2** (with
`rom_rev`, `base_addr`, `boot_pc` — a direct offset read), **not**
milestone 3's resident-scan work. `RomInfo`'s `exec_rev` field is a plain
header read, no `ResidentScan` dependency.

**Sources.** Resolved entirely by the oracle method specified in the
task (vary bytes, observe `romtool`'s output) — deliberately not decided
by reading amitools source, matching the task's explicit instruction.

**Confidence: CONFIRMED** — 3 direct-value oracle trials, 1 negative-
control oracle trial (fake resident, no effect), 5 real-ROM cross-checks,
plus the sibling `rom_rev` field behaving identically.

---

## Remaining unknowns

- **Offset 0x08's `0x0000FFFF` constant** (§1): presence and exact value
  confirmed across 5 real ROMs; its semantic purpose and whether any
  `romtool` check depends on it were not established (no mutation test
  run against it — worth a follow-up oracle pass before milestone 2 if a
  check ever needs it).
- **`kickety_split`'s real-world distribution** (§4): confirmed the exact
  mechanism and its two checked fields; did not survey enough ROMs to
  state a reliable rule for "which eras/vendors have it" beyond "it's
  whichever ROMs happen to have `0x1111 0x4EF9` at their own midpoint" —
  PROBABLE, not CONFIRMED, for the era-correlation question specifically
  (the mechanism itself is CONFIRMED).
- **The header-marker-word allow-list** (§1, offset 0x00): shown to be a
  small set (`0x1111`, `0x1114` both seen valid, with size-dependent
  acceptance — `0x1111` only accepted for 256 KiB images in the oracle),
  but the *complete* enumeration (DiagROM, ROM Switcher, Logica-Dialoga,
  AROS, etc., per `PLAN.md`'s separate "boot-vector signature table"
  item) was intentionally left to that item, not duplicated here.
- **Footer tolerance boundary precision**: confirmed the check accepts a
  wrong value at `len-16` and rejects wrong values at `len-14`..`len-2`
  and `len-1`, but did not test every one of the 7 remaining words
  individually (only one representative middle word and the last word) —
  PROBABLE that all 7 are independently required, not fully exhaustive.
- **Hi/lo EPROM interleave, Cloanto container framing precision, and the
  byte-order signature table** are separate `PLAN.md` milestone-1 items
  outside this task's scope (facts 1–6 only) and were not investigated
  here.

---

## Addendum: broader local sweep (parent-session review, 2026-09-15)

The facts above were re-verified by the reviewing session against the
~/Documents/Amiberry/Kickstarts collection: 57 images, of which 52 carry
the JMP-style header. **49 of those 52 pass every check** (whole-image
end-around-carry sum == 0xFFFFFFFF, size field == actual length, marker/
JMP layout, `rom_rev`/`exec_rev` at 0x0C/0x10 matching publicly known
version numbers) across Kickstart 0.7, 1.0, 1.1 (PAL+NTSC), 1.2, 1.3,
1.4β, 2.04, 2.05, 3.0, 3.1 (six machine variants), CD32, Walker 43.1,
3.X 45.64, 3.1.4, 3.2 46.143, 3.2.x 47.102/111/115 (five machine
variants each), and AROS. New facts this wider sample adds:

- **`0x1111` on 512 KiB images exists in genuine ROMs**: the 1.4-beta
  A3000 ROM (36.16, base `0x00FF0000`) and the Logica Dialoga ROM are
  both 512 KiB with marker `0x1111`. Per §1's oracle result, romtool
  reports `header: NOK` for that combination — i.e. **romtool's
  size-dependent marker rule rejects a genuine Commodore beta ROM.**
  Milestone-2 decision recorded in PLAN.md: match romtool for `info`
  parity, but document the known false-negative.
- **`rom_rev` = `0xFFFF.0xFFFF` on pre-1.2 ROMs**: 0.7, 1.0, and both
  1.1 dumps carry an unpopulated (all-ones) `rom_rev` field while
  `exec_rev` is real (27.6, 1.2, 31.34). The 0x0C field only became
  populated from Kickstart 1.2 (33.180) on. Value methods must not
  assume the field is meaningful on early ROMs.
- **A third and fourth real base address**: CDTV/A570 extended ROMs sit
  at `0x00F00000` and the 1.4β at `0x00FF0000` — both handled correctly
  by the `boot_pc & 0xFFFF0000` derivation, further confirming
  base_addr is computed, not enumerated.
- **Extended/diagnostic ROMs are header-valid but not KickSum-sealed**:
  the CDTV 1.3 extended ROM and Logica Dialoga carry valid-looking JMP
  headers but fail checksum and size-field — correct `is_kick: false`
  material, and good hostile-ish fixtures for the env-gated harness.
  (CD32's extended ROM *is* sealed; its `exec_rev` slot holds garbage
  (`19196.224`), since extended ROMs have no exec.)

Sweep method: the same from-scratch Python checks as §§1–6 (no GPL code
involved), run over every `*.rom`/`*.bin` in the directory; images
without the JMP header (A4091/A590/Picasso IV expansion ROMs, CD32 FMV
module) skipped as expected non-Kickstart formats.
