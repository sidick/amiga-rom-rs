# Byte-order facts from kicksmash32

Source repo: `https://github.com/cdhooper/kicksmash32` (BSD-2-Clause).
Commit read: `a34355083ce89d72ab022547a58d59ebfc133aa1` (cloned
2026-09-15; this was `HEAD` of the default branch at clone time — the
repo's own `git log -1 --date=short` reports the tip commit dated
2026-09-15, i.e. it was read at the tip of history, not a tagged
release). All facts below are transcribed from `sw/hostsmash.c` and the
`doc/` tree at that commit; permalink form:
`https://github.com/cdhooper/kicksmash32/blob/a34355083ce89d72ab022547a58d59ebfc133aa1/sw/hostsmash.c`.

Scope note: this document covers Milestone-1 items "Boot-vector
signature table for byte-order detection" and "Byte-swapped output
conventions" only. Header/footer field semantics and hi/lo EPROM
interleave are other workers' deliverables and are only touched here
incidentally (item 4, "Dual Kickstart" confirm/deny).

---

## 1. The signature table — CONFIRMED (read directly from code)

`detect_byte_order()`, `sw/hostsmash.c` lines 2004–2031:

```c
static const uint32_t comp[][2] = {
    { BESWAP(0x11144ef9), BESWAP(0x00f800d2) },  // 2.04+
    { BESWAP(0x11114ef9), BESWAP(0x00fc00d2) },  // 1.3
    { BESWAP(0x612e4447), BESWAP(0x00f80190) },  // DiagROM 2.x Beta
    { BESWAP(0x11144447), BESWAP(0x00f800d6) },  // DiagROM 2.x
    { BESWAP(0x11144ef9), BESWAP(0x00f80010) },  // ROM Switcher
    { BESWAP(0x11114ef9), BESWAP(0x00f8048c) },  // Logica-Dialoga
    { BESWAP(0x11114ef9), BESWAP(0x00f800f8) },  // AROS
};
```

`BESWAP(x)` is a host-endianness normalizer (`SWAP32(x)` on a
little-endian build host, identity on a big-endian host) — it exists so
the constants below can be written in the big-endian order they'd
appear in the ROM file, regardless of what CPU `hostsmash` itself runs
on. The values as written are therefore the big-endian (canonical,
unswapped) byte sequence.

| Family | Longword 0 (bytes, big-endian) | Longword 1 (bytes, big-endian) | Meaning (CONFIRMED bytes; PROBABLE semantic reading) |
|---|---|---|---|
| Kickstart 2.04+ (regular Kickstart) | `11 14 4e f9` | `00 f8 00 d2` | LW0 = initial SSP `$00111444`(? — see note) high word `1114`... actually LW0 is `0x11144ef9`: top 16 bits `0x1114` look like part of an SSP value pattern shared by all "real OS" ROMs (`1114` vs `1111` for 1.3), bottom 16 bits `0x4ef9` is m68k opcode `JMP xxx.L` (`4ef9`) — i.e. LW0 is *not* the SSP itself but the second half of a `dc.l initial_SSP` immediately followed by the reset PC opcode, per standard m68k reset-vector layout (SSP at offset 0, PC at offset 4). LW1 `0x00f800d2` reads as the reset **PC** operand for the `JMP`: `$00F800D2`, a valid `0xF8xxxx` Kickstart-ROM-space address (2.04+ era mapped at `$F80000`). |
| Kickstart 1.3 | `11 11 4e f9` | `00 fc 00 d2` | Same `4ef9` (`JMP`) opcode in LW0's low word; high word `0x1111` differs from 2.04+'s `0x1114`. LW1 `$00FC00D2` is a `0xFCxxxx`-space reset PC, matching 1.3-era ROMs being mapped at `$FC0000` (256 KiB ROM, top of the 512 KiB `$F80000`-`$FFFFFF` mirror region) rather than `$F80000`. |
| DiagROM 2.x Beta | `61 2e 44 47` | `00 f8 01 90` | LW0 is *not* a JMP — `0x612e` is opcode `BSR.W` (branch-to-subroutine, word displacement `0x4447`), i.e. DiagROM's reset vector executes a short branch instead of the `JMP xxxxxxxx.L` regular Kickstarts use; DiagROM's own boot sequence is unrelated to exec's resident chain at this point. LW1 `$00F80190` is a `0xF8xxxx`-space address, consistent with it living at the same `$F80000` base as 2.x Kickstarts. |
| DiagROM 2.x | `11 14 44 47` | `00 f8 00 d6` | High word `0x1114` matches the 2.04+ SSP-high-word pattern; low word `0x4447` is *not* `4ef9` (not a `JMP`) — DiagROM's production build apparently starts differently from the Beta build's `BSR`. LW1 is again `$00F8xxxx`. |
| ROM Switcher | `11 14 4e f9` | `00 f8 00 10` | Identical LW0 to plain 2.04+ Kickstart (same SSP-high-word + `JMP` opcode) — only LW1 (the JMP target) differs: `$00F80010`, a very low offset into the `$F80000` ROM space, consistent with the ROM Switcher's tiny selector code living right at the front of its own image before jumping into a chosen Kickstart bank. |
| Logica-Dialoga | `11 11 4e f9` | `00 f8 04 8c` | LW0 matches the 1.3-era SSP-high-word (`0x1111`) + `JMP` opcode. LW1 target `$00F8048C` is `0xF8xxxx`-space (not `0xFCxxxx` like real 1.3), i.e. this is a 1.3-compatible/1.3-derived ROM relocated to load at `$F80000`. |
| AROS | `11 11 4e f9` | `00 f8 00 f8` | Same LW0 as Logica-Dialoga/1.3-pattern (`0x1111` + `JMP`). LW1 target `$00F800F8` — again `$F8xxxx`-space, small offset. |

**Important code-behavior fact (CONFIRMED):** although the table is
declared as pairs `comp[cur][0]`/`comp[cur][1]`, the matching loop
(lines 2020–2029) **only ever compares `comp[cur][0]` against
`buf32[0]`** — the first longword of the buffer. `comp[cur][1]` (the
second longword / boot-PC value) is present in the table's data
declaration but is **not read anywhere in `detect_byte_order()`** in
this revision. Detection is therefore effectively single-longword
matching in the current code, even though the table is shaped as
pairs and the second value is clearly meant as the corroborating
reset-PC check. Anyone porting this table should decide independently
whether to also check the second longword (stricter, matches the
table's evident intent) or to replicate kicksmash32's current
single-longword behavior exactly (loosest, matches observed code).
This is a fact worth flagging to whoever implements `Loader::detect`,
since it affects false-positive rate on the header/footer research
item, not just this one.

For every table row, the code checks `buf32[0]` against the stored
value under all four permutations (see §2) and returns the name of
whichever permutation matched; no match returns the sentinel `0xffff`.

**Rating: CONFIRMED** — table transcribed verbatim from
`sw/hostsmash.c:2008-2016`; the "why" column beyond the bare hex is
**PROBABLE** — inferred from m68k reset-vector convention (SSP at
offset 0, PC/JMP at offset 4) and opcode tables, not from an explicit
kicksmash32 comment (the source has no prose explanation of *why*
these particular constants, only the trailing `//` ROM-family labels
quoted in the table above).

---

## 2. The four swap-mode permutations — CONFIRMED

`execute_swapmode()`, `sw/hostsmash.c` lines 2076–2216, and the
`SWAP_xxxx` macros at lines 1982–1985.

Let input bytes of one 4-byte group be `[b0, b1, b2, b3]` (`b0` = the
lowest file offset). "Output byte i = input byte P(i)" for each mode,
matching the `for` loops exactly:

- **`0123` — normal / no swap.** Output = `[b0, b1, b2, b3]`. Identity;
  `execute_swapmode` takes no action (`break` immediately, line
  2078-2079).
- **`1032` — swap adjacent bytes within each 16-bit word.** Output =
  `[b1, b0, b3, b2]`. Implemented as a pairwise swap over the whole
  buffer 2 bytes at a time (`buf[pos+0] <-> buf[pos+1]`, `pos += 2`),
  lines 2082–2087. Equivalently the macro
  `SWAP_1032(x) = ((x & 0x00ff00ff) << 8) | ((x >> 8) & 0x00ff00ff)`.
- **`2301` — swap the two 16-bit words within each 32-bit group.**
  Output = `[b2, b3, b0, b1]`. Implemented as `buf[pos+0] <-> buf[pos+2]`
  and `buf[pos+1] <-> buf[pos+3]`, `pos += 4`, lines 2092–2099.
  Equivalent macro `SWAP_2301(x) = (x << 16) | (x >> 16)`.
- **`3210` — full 32-bit byte reversal (big-endian⇄little-endian
  longword swap).** Output = `[b3, b2, b1, b0]`. Implemented as
  `buf[pos+0] <-> buf[pos+3]` and `buf[pos+1] <-> buf[pos+2]`,
  `pos += 4`, lines 2104–2111. Equivalent macro
  `SWAP_3210(x) = (x<<24)|(x>>24)|((x&0xff00)<<8)|((x>>8)&0xff00)`.

All three swap loops guard against a trailing partial group
(`pos < len - 1` for `1032`, `pos < len - 3` for `2301`/`3210`) — an
odd-length or non-multiple-of-4 tail is simply left unswapped past the
last full group, no error is raised.

**Detection-logic structure (CONFIRMED):** `detect_byte_order()` does
*not* try the four permutations against the *buffer*; instead, for each
table entry it applies each of the four `SWAP_xxxx` macros to the
*stored constant* and compares the result against the buffer's raw
`buf32[0]` (lines 2020–2028: literal comparison, then `SWAP_1032`,
`SWAP_2301`, `SWAP_3210` of `comp[cur][0]`). This is mathematically
equivalent to un-swapping the buffer and comparing to the constant, but
implemented as "swap the known-good value four ways and see which one
equals what's on disk" rather than "swap the disk value four ways and
see which one equals a known-good value." The loop order is: outer
loop over table rows, inner loop (unrolled as four `if`s) over swap
modes, in the fixed order `0123, 1032, 2301, 3210`; first match wins
and returns immediately.

`execute_swapmode()` then uses `detect_byte_order()`'s result plus a
target `swapmode` (explicit `-s` flag or auto-derived from
`kicksmash_mode`, see §3) to decide which single swap to actually
apply (or none) — it is not a "try all four, pick one" search at write
time; it's "detect current order once, then compute the one swap
needed to reach the desired order," implemented as a chain of
`if (byteorder == X) goto swap_Y` per source/destination pair
(lines 2113–2216).

**Rating: CONFIRMED** — permutations and detection structure both read
directly from `sw/hostsmash.c`.

---

## 3. Hardware mapping / output conventions

### 3.1 Mode constants (CONFIRMED, `sw/hostsmash.c` lines 263–272)

```c
#define KICKSMASH_MODE_32     0  // Swap mode 3210 (Amiga 3000/4000)
#define KICKSMASH_MODE_16     1  // Swap mode 1032 (Amiga 500/2000)
#define KICKSMASH_MODE_16HI   2  // Swap mode 1032 (Amiga 500/2000)
#define KICKSMASH_MODE_AUTO   3  // Swap mode 3210 (Amiga 3000/4000)
#define KICKSMASH_MODE_32SWAP 4  // Swap mode 1032 (Amiga 3000T)

#define SWAPMODE_AUTO   0xa040   // Automatic mode
#define SWAPMODE_16     0x0010   // Amiga 16-bit ROM format
#define SWAPMODE_32SWAP 0x1032   // Amiga 32-bit ROM format hi-lo swapped
#define SWAPMODE_32     0x3210   // Amiga 32-bit ROM format
```

These comments are kicksmash32's own words — direct source quotes, not
my inference.

### 3.2 CLI documentation (CONFIRMED, `doc/sw_hostsmash.txt` lines 124–143)

Direct quote:

> `-s 3210` 32-bit big-endian with little-endian swapping. This mode is
> useful for the Amiga 3000 and Amiga 4000 ROM images.
> `-s 0123` No byte swapping. This is the default.
> `-s 1032` Swap odd and even adjacent bytes. This mode may be useful
> when dealing with ROM images for the Amiga 500 and 2000 computers. Not
> verified at this point. It is likely also the mode required for A1200
> images.
> `-s 2301` Swap adjacent 16-bit words.

and, worked example against a real downloaded ROM (`od -An -t x1
322.rom -N 4` → `11 14 4e f9`, i.e. already in `0123`/normal order as
distributed):

> you can see that "3210" should be the proper swapping mode because
> byte 0 is where byte 3 should be; byte 1 is where byte 2 should be;
> byte 2 is where byte 1 should be; and byte 3 is where byte 0 should
> be.

(i.e., to go from the distributed `0123` file to what an A3000/A4000
KickSmash flash part wants, apply the `3210` permutation.)

### 3.3 `prom mode` firmware modes (CONFIRMED, `doc/sw_kicksmash.txt` lines 199–226)

> Mode 0 - 32-bit … (A3000 and A4000)
> Mode 1 - 16-bit low … (A500/A2000) … selects only the low flash part
> Mode 2 - 16-bit high (no Amiga) … selects only the high flash part
> Mode 3 - auto … automatically choose 32-bit or 16-bit depending on the
> detected Amiga

This is the firmware-side (KickSmash board) notion of 16-bit vs 32-bit
access width, distinct from but related to the host-side `-s` swap
modes: a 32-bit-socket machine (A3000/A4000-class) reads all four ROM
data lines at once per access, so the flash must hold data in true
32-bit-longword order (`3210`, i.e. reversed from the file's `0123`
distribution order because the ROM sockets are wired little-endian
relative to the file's big-endian byte stream — this is the
`SWAP_3210` "full longword reversal" case). A 16-bit-socket machine
(A500/A600/A2000-class) only ever reads 16 bits per access, so what
matters is getting the *word* order right for that narrower bus, which
is the `1032` byte-pair swap.

### 3.4 A3000T variant (CONFIRMED — explicit hardware erratum, not a general rule)

`rev7_3kt/errata.txt` line 1 (direct quote):

> ROM left-right order is wrong for 3000T (need -s 1032), but correct
> for AA3K+

`doc/WHICH.md` table row: `A3000T | KickSmash 3KT | 21.38 mm (est.) |
hostsmash -s 1032`. Also `doc/getting_started_windows.md` line 308:

> **Model note:** the A3000T uses a swapped layout and needs `-s 1032`.

So the A3000T is a documented **exception** to the general "A3000/A4000
= 3210" rule: despite being a 32-bit-socket tower machine in the same
family as the desktop A3000, its physical ROM socket wiring (rev 7 of
the KickSmash 3KT board, per the errata) requires the `1032` swap
instead of `3210` — i.e. its hi/lo 16-bit halves are laid out swapped
relative to the desktop A3000/A4000, even though the access width is
still 32-bit. `KICKSMASH_MODE_32SWAP` (`sw/hostsmash.c` line 267,
comment "Amiga 3000T") is exactly this mode, mapped to `SWAPMODE_32SWAP
= 0x1032`.

### 3.5 `dd conv=swab` — PROBABLE / not found in kicksmash32

No occurrence of `swab` anywhere in the kicksmash32 tree (checked with
`grep -rin swab .` against the cloned commit — zero hits). This is a
community-documentation convention (Unix `dd conv=swab` performs
exactly the `1032` byte-pair-swap-per-16-bit-word operation, since
`swab` is defined as swapping adjacent byte pairs across the whole
buffer), not something kicksmash32 documents or uses. It is included
here only as a cross-reference for milestone-1's ask; **rate this line
PROBABLE, sourced from general Unix `dd`/`swab(3)` semantics, not
kicksmash32.** If PLAN.md or later code wants an authoritative citation
for the `dd conv=swab` ⇄ `1032` equivalence, it should come from a
`dd`/POSIX reference, not this document's primary source.

### 3.6 Byte-swap / device summary table

| Swap mode | Permutation (output = input at index) | When a dump in this order arises | What wants it |
|---|---|---|---|
| `0123` (normal) | `[0,1,2,3]` (identity) | Standard distributed `.rom` file format (Cloanto/Hyperion-style, and what AmigaOS itself produces if you copy ROM space to a file while running) — CONFIRMED via the worked example in `doc/sw_hostsmash.txt` (`od` of a real downloaded ROM shows `11 14 4e f9`, i.e. already normal order). | Not itself a socket target; this is the canonical/reference order everything else is measured against. `hostsmash -s 0123` is also documented as "no byte swapping... the default" for reads/writes when no swap is desired. |
| `3210` (full longword reversal) | `[3,2,1,0]` | Result of reversing byte order within each 32-bit group of a normal-order file. | **A3000 / A4000-class (32-bit ROM socket) KickSmash boards**, per `SWAPMODE_32` comment "Amiga 32-bit ROM format" and `KICKSMASH_MODE_32`/`_AUTO` comment "Amiga 3000/4000", and the CLI doc's explicit worked example concluding "3210 should be the proper swapping mode" for that case. This is `prom mode 0` (32-bit) hardware. |
| `1032` (adjacent-byte-in-word swap) | `[1,0,3,2]` | Result of swapping each pair of bytes within 16-bit words of a normal-order file (equivalent to a `dd conv=swab`-style operation — PROBABLE cross-reference, not sourced from kicksmash32). | **A500/A600/A2000-class (16-bit ROM socket) machines**, per `SWAPMODE_16`/`KICKSMASH_MODE_16`/`_16HI` comments "Amiga 500/2000" — CLI doc: "may be useful when dealing with ROM images for the Amiga 500 and 2000 computers... likely also the mode required for A1200 images" (doc hedges this as unverified). **Also the A3000T tower variant** specifically (documented hardware erratum, `SWAPMODE_32SWAP`/`KICKSMASH_MODE_32SWAP` "Amiga 3000T") — a 32-bit-socket machine that nonetheless needs the 16-bit-style swap due to board wiring. Two distinct real-world causes land on the same permutation. |
| `2301` (adjacent-word swap) | `[2,3,0,1]` | Result of swapping the two 16-bit halves of each 32-bit group. | No kicksmash32 doc/comment names a specific real Amiga model that natively wants `2301` as its target hardware order; it appears in the code as one of the four detectable/settable permutations (and as an intermediate step `execute_swapmode` passes through when converting between `0123`/`3210`/`1032`/`32SWAP` targets, e.g. `SWAPMODE_16`'s `SWAP_TO_ROM` path goes `0123 → (swap 2301) → ...` toward `1032`). **PROBABLE**: not confirmed to correspond to any single real device in kicksmash32's own documentation; treat as "the fourth permutation, present for completeness / as a detection possibility" rather than a named hardware target. |

Additional device-specific ROM-socket notes from `doc/WHICH.md`
(CONFIRMED, direct table read): A500/A600/A2000/A1000 are **not
supported** by any current KickSmash board ("Single 16-bit socket" /
"Pair of ROMs form 16-bit data" for the A1000 specifically) — so the
16-bit swap-mode facts above describe the *target ROM image format*
for those machines' sockets, not a KickSmash product that programs
them; A1200/A3000/A3000T/A4000/A4000CR/A4000T/A4000TX/AmigaPCI all have
dedicated KickSmash boards.

---

## 4. "Dual Kickstart" — CONFIRMED DENIAL of a hi/lo split reading

Searched the full kicksmash32 tree (`grep -rin "dual"` across
`doc/*.txt doc/*.md README.md`) at the pinned commit: **zero
occurrences of "Dual Kickstart" or "dual kickstart" anywhere in the
repository.** The phrase does not appear in kicksmash32's code or docs
at all, so there is nothing in this source to confirm or deny about a
term it doesn't use.

What the repo *does* document, and what PLAN.md's working hypothesis
almost certainly refers to, is the **flash bank** mechanism
(`doc/sw_smash.txt` lines 182–284, and the `smash bank` /
`prom bank` CLI commands): KickSmash's flash is partitioned into eight
independently-selectable 512 KiB **banks**, each holding a complete,
independent ROM image (e.g. one bank has an OS 3.2 Kickstart, another
DiagROM, another a spare/backup Kickstart); a `smash bank current N`
command selects which bank the Amiga boots from, and banks can be
merged in hardware to form larger 1/2/4 MiB images. Direct quote,
`doc/sw_smash.txt` line 182-186:

> partitions the flash address space into eight banks. Each bank is
> 512 KB. Certain banks may be merged together to form a larger 1MB or
> 2MB or even 4MB bank, but this requires additional hardware in your
> Amiga...

This is unambiguously a **selectable-ROM-image / multi-bank** feature
(soft-kicking between whole Kickstart images), **not** a two-file
hi/lo EPROM byte-split. The hi/lo split is a completely separate,
unrelated concept in this same codebase: it's the `-lo.bin`/`-hi.bin`
pair format for programming discrete 16-bit-wide EPROM chips
(`doc/sw_hostsmash.txt` lines 196–211, `doc/sw_kicksmash.txt` lines
199–226, `prom mode 1`/`prom mode 2` = "16-bit low"/"16-bit high" =
select one physical flash chip at a time for programming). kicksmash32
explicitly treats bank-selection and hi/lo-chip-splitting as two
unrelated axes: banks are about *which whole ROM image is active*;
hi/lo is about *which physical chip half of one 32-bit-wide flash pair
you're addressing*.

**Rating: CONFIRMED** (the source doesn't use the term "Dual
Kickstart" at all; the feature that plausibly earned that plain-English
description elsewhere is the flash-bank mechanism, confirmed here to
be about selectable whole-image banks, not hi/lo splitting).
PLAN.md's characterization stands: **hi/lo splitting is a distinct,
out-of-scope concept**, correctly left to the other worker's
deliverable (Milestone 1's "Hi/lo EPROM interleave width" item).

---

## Summary for the parent session

- Signature table: 7 rows, transcribed verbatim above (§1). Detection
  in the current code only checks the first longword per row, not the
  documented pair — worth a decision when porting.
- Four permutations, unambiguous byte-index form, in §2.
- Hardware mapping table in §3.6: `3210` → A3000/A4000 32-bit sockets;
  `1032` → A500/A600/A2000/A1200 16-bit sockets **and** the A3000T
  tower exception; `2301` → no confirmed real-device target in
  kicksmash32's own docs (present as the fourth permutation only);
  `0123` → the distributed/reference file format. `dd conv=swab` is a
  PROBABLE community cross-reference for `1032`, not found in
  kicksmash32 itself.
- "Dual Kickstart" as a term does not appear in kicksmash32; the
  feature it likely refers to (flash banks) is confirmed to be
  selectable whole-ROM banks, not a hi/lo split — PLAN.md's assertion
  holds.
