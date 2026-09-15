# Milestone-1 facts: Cloanto container framing, and hi/lo EPROM interleave

Research for PLAN.md milestone 1, items "Cloanto container framing,
precisely" and "Hi/lo EPROM interleave width". Findings below correct a
wrong working assumption baked into the current `src/lib.rs` scaffolding
(see "Correction to current code" at the end).

## Sources used

- **Empirical, local**: seven genuine Cloanto/Amiga Forever `.rom` files
  and a `rom.key` at `~/kicksmash32/cloanto_roms/` (also duplicated under
  `~/Library/CloudStorage/Dropbox/.../FS-UAE/Kickstarts/` and
  `~/Documents/.../FS-UAE/Kickstarts/`), spanning AmigaOS 1.3/1.4/2.04/
  3.1/3.X A3000 kickstarts plus two "bonus" ROM modules. Inspected with
  `xxd`/`python3` (byte-level, independent re-implementation of the
  decode), never copying key or ROM bytes into this report beyond a
  handful of individual illustrative byte values.
- **AmigaROMUtil** (MIT License, Copyright (c) 2021/2026 Christopher
  Gelatt), `github.com/Kreeblah/AmigaROMUtil` — **not** Hyperkid123 as
  the task brief guessed; that name did not turn up a matching repo,
  Kreeblah/AmigaROMUtil did and its `LICENSE` file confirms MIT. Cloned
  at commit `26645e86e020bc13e421cb02143f9f5b2e0d000a` (2026-06-26).
  Read directly and cited by file/function/line below, and also **built
  and run** against the local real ROMs as a second, independent
  behavioural cross-check (round-tripped decrypt → split → merge →
  un-swap back to the original bytes).
- **Cloanto's own documentation**: `cloanto.com/amiga/roms/help/type.html`
  ("Kickstart ROM Types Summary") — primary-source prose, fetched and
  quoted below.
- **amitools' `romtool`** (GPL-3, run only as a black-box oracle, source
  never read): installed in a throwaway venv
  (`amitools==0.8.1`), run as `romtool -k rom.key info <file>` against
  one of the real local ROMs. Confirms the decode without informing any
  implementation.

## A. Cloanto/Amiga Forever ROM container, precisely

### A1. Header framing — CONFIRMED, and it corrects PLAN.md's working hypothesis

The container's leading marker is **not** the long ASCII copyright
banner ("AMIGA ROM Operating System and Libraries: Copyright
1985-1992 Commodore-Amiga, Inc. All Rights Reserved.") that PLAN.md and
the current `src/lib.rs` (`CLOANTO_HEADER`) assume. That banner text
**is real**, but it lives *inside the decoded Kickstart payload itself*
— it's a copyright string AmigaOS ships embedded in the ROM image, not
part of Cloanto's container framing. Empirically, in a decoded 3.1 A3000
ROM it starts at decoded-payload offset `0x1E` and reads (byte for
byte, confirmed against the real file): `AMIGA ROM Operating System and
Libraries Copyright © 1985-1993  Commodore-Amiga, Inc.  All Rights
Reserved.` — note this is *not byte-identical* to PLAN.md's assumed
string either (curly copyright glyph, doubled spaces, `1985-1993` not
`1985-1992`, no colon after "Libraries") because it's a different string
(the in-ROM banner varies by ROM revision/year) from a different place
(inside the payload) than the code currently assumes.

The actual container header is a fixed 11-byte ASCII magic:

```
AMIROMTYPE1
```

with **no NUL terminator, no padding, no length field** — the payload
begins immediately at byte offset 11. Verified:
- Directly via `xxd` on all seven local `.rom` files: every one starts
  with bytes `41 4d 49 52 4f 4d 54 59 50 45 31` = `"AMIROMTYPE1"`,
  identically, across OS revisions 1.3 through 3.X and both "bonus"
  modules (i.e. the magic does **not** vary between Amiga Forever
  revisions/ROM types — same 11 bytes in every sample on hand).
- Independently in AmigaROMUtil: `DetectAmigaROMEncryption()` (
  `AmigaROMUtil.c:1203-1215`) does exactly `strncmp(rom_data,
  "AMIROMTYPE1", 11) == 0`, and `CryptAmigaROM()`'s decrypt leg
  (`AmigaROMUtil.c:1306-1309`) skips exactly the first 11 bytes before
  handing the rest to the XOR step; its encrypt leg
  (`AmigaROMUtil.c:1352`) writes back exactly `"AMIROMTYPE1"` (via
  `snprintf(..., 12, "AMIROMTYPE1")`, 11 chars + NUL that is *not*
  written to the file) as the only header content.
- `romtool` (GPL, oracle only) auto-detects and decrypts these same
  files correctly with no format hints beyond `-k rom.key`, corroborating
  that leading-11-byte-magic detection is what a well-established GPL
  tool also keys off.

### A2. Footer — CONFIRMED: none

There is **no trailing plain-text footer** in this format (distinguish
from the in-payload copyright string and the m68k autovector table at
the very end of a real Kickstart image, both of which are *part of the
Kickstart ROM itself*, not container framing). The XOR'd payload runs
from byte 11 to end-of-file, full stop:

- `decoded_length = file_length - 11` reproduced the exact canonical ROM
  size for every real Kickstart sample: 262144 (256 KiB) for the 1.3
  ROM, 524288 (512 KiB) for the 1.4/2.04/3.1/3.X ROMs. No leftover bytes
  to trim, no shortfall.
  - Independently confirmed byte-for-byte: an XOR decode written from
    scratch (Python, cycling `rom.key` from index 0) against the 3.1
    A3000 ROM matched the output of `~/kicksmash32/cloanto_roms/
    decrypt-amigaforever` (a separate, GPL-2, black-box-only tool found
    alongside the ROMs) exactly, and separately matched what
    AmigaROMUtil's `CryptAmigaROM()` decrypt path computes (same
    algorithm, read directly, MIT).
  - The tail of the decoded 3.1 ROM ends `... 0018 0019 001a 001b 001c
    001d 001e 001f` — exactly the classic m68k autovector interrupt
    table Kickstart images end with, immediately after the last content
    byte of the file. There is nothing after it.
  - `romtool info -k rom.key` on the same file reports `footer  ok`
    with no separate footer-stripping step needed before feeding it the
    file — consistent with "the whole post-header span, once XOR'd, is
    already an exact, complete Kickstart image."
- AmigaROMUtil's `CryptAmigaROM()` never trims anything beyond the
  11-byte header on decrypt, nor appends anything beyond the header on
  encrypt (`AmigaROMUtil.c:1249-1364`): `result_size = rom_size - 11`,
  full stop.

**Payload extent is therefore exactly `data[11..]`** — decoding is
"strip 11 bytes, XOR the rest," with nothing left for a caller to trim.
This directly resolves the milestone-2 TODO note in `decode_cloanto`'s
doc comment ("trailing footer bytes, if any, are left for the caller to
trim") — there are none; that hedge can be deleted once this correction
lands.

### A3. `rom.key` handling — CONFIRMED

Whole key file used as a **cycling XOR pad starting at index 0 for
every decode** (not: continuing from wherever a previous operation left
off, not: any per-file offset/seed). `payload[i] XOR key[i % key.len()]`
for `i` counted from the first payload byte (index 0 of the 11-byts-in
payload, not of the whole file). Confirmed:
- Independently by writing a from-scratch decoder using this exact rule
  and matching two independent existing decoders' output byte-for-byte
  on all six full-size local Kickstart ROMs.
- In AmigaROMUtil's `DoAmigaROMCryptOperation()`
  (`AmigaROMUtil.c:1368-1383`): `key_idx` starts at 0 and increments
  `% keyfile_size` per payload byte — exactly this rule, read directly
  (MIT).
- Key length is not fixed/specified anywhere; the local sample key file
  is 1426 bytes (not a round power of two). No key bytes are reproduced
  here per the task's instruction.

### A4. Detection — CONFIRMED: leading-magic match is sufficient and reliable

Matching the fixed 11-byte ASCII prefix `AMIROMTYPE1` is what every
independent tool checked here uses as the sole detection signal
(AmigaROMUtil's `DetectAmigaROMEncryption`, the found
`decrypt-amigaforever` tool's `strcmp` on the first 11 bytes, and
implicitly `romtool`'s auto-detection, which required no other hint).
There is no stricter secondary magic/checksum gating the format — the
container's only integrity signal is that decoding with the right key
yields a valid Kickstart image (size, checksum, footer autovectors all
verifiable *after* decode, via the milestone-2 checks). Cloanto's own
docs (`cloanto.com/amiga/roms/help/type.html`) don't name the magic
string explicitly but describe the "EPROM" ROM-image type as unencrypted
("bytes are presented in sequential order with no encryption") in
contrast to the (Cloanto-only, encrypted) `.rom` container type this
research covers — consistent with there being exactly one detection
signal (the magic) rather than a format family needing disambiguation
beyond it.

## B. Hi/lo EPROM split interleave

### B1. Interleave width and hi/lo assignment — CONFIRMED: 16-bit-word interleave, not byte interleave

For a 32-bit-bus target (A1200/A3000/A4000-class, two same-size 16-bit
EPROMs such as 27C400-class parts, one per ROM socket), each 32-bit
canonical longword `b0 b1 b2 b3` is split as **two 16-bit words**, not
individual bytes:

- **hi** file gets the **first (high-order) 16-bit word** of each
  longword: bytes `b0 b1`.
- **lo** file gets the **second (low-order) 16-bit word**: bytes `b2
  b3`.

This is AmigaROMUtil's `SplitAmigaROM()` (`AmigaROMUtil.c:1611-1615`):
```c
for (i = 0; i < amiga_rom->rom_size; i += 4) {
    memcpy(&rom_high->rom_data[i / 2], &amiga_rom->rom_data[i],     2);
    memcpy(&rom_low->rom_data[i / 2],  &amiga_rom->rom_data[i + 2], 2);
}
```
and its inverse `MergeAmigaROM()` (`AmigaROMUtil.c:1670-1674`) undoes
exactly this. Verified empirically end-to-end by building the tool and
round-tripping a real 512 KiB Kickstart (3.1, A3000) through it:
`decrypt → split → merge → un-byte-swap` reproduced the original
decrypted bytes exactly (`cmp` reported no difference).

Socket naming: "hi" = the socket taking the file with the numerically
higher-order 16 bits of each word (called "Kickstart Hi" in
AmigaROMUtil's socket-label comments, `AmigaROMUtil.h:137-139`, labelled
`U34`/`A2630`-era board silkscreen in that source's comment — socket
labels are board-specific and vary by machine/expansion board). For the
A4000 specifically, community documentation (Pure Amiga's "Burn your
own Kickstart" guide and AmiBay's Kickstart 3.9 burning guide, both
web-searched, non-GPL prose) states the HI-file EPROM goes in socket
U175 (top) and the LO-file EPROM goes in socket U176 (bottom) — i.e.
"hi"/"lo" is a data-content label (which half of each word), and the
physical socket it occupies is a separate, board-specific fact.

### B2. Applies to any (word-aligned) size, not just 512 KiB — CONFIRMED

Nothing in `SplitAmigaROM`/`MergeAmigaROM` is specific to 512 KiB
images. The only preconditions checked are `rom_size % 2 == 0` (split)
and `rom_size % 4 == 0` (merge) (`AmigaROMUtil.c:1533`, `:1637`) — i.e.
any image whose size is a multiple of 4 bytes splits/merges cleanly.
512 KiB → 2×256 KiB is simply the common real-world case because that's
the size of a full Kickstart image on 32-bit-bus machines; a 256 KiB
Kickstart (e.g. the 1.3 image on hand) would split into 2×128 KiB just
as mechanically. Cloanto's own docs describe the EPROM/hi-lo split
generically ("may need to be appropriately split... depending on the
machine and EPROM burner") without tying it to a specific image size,
consistent with this.

### B3. Byte-swapping convention for the burner-ready pair — CONFIRMED

Yes — AmigaROMUtil's split step **unconditionally byte-swaps the image
to 16-bit-word-swapped ("1032") order before interleaving**, i.e. the
canonical longword `b0 b1 b2 b3` is first transformed to `b1 b0 b3 b2`
and *that* is what gets word-interleaved into hi/lo. Concretely,
`SplitAmigaROM` (`AmigaROMUtil.c:1542-1545`) always forces a byte-swap
via `SetAmigaROMByteSwap` before running the interleave loop, and the
CLI names this operation explicitly: `-p Byte swap ROM for burning to
an IC` (`main.c` usage text) as a distinct, expected step separate from
splitting. This was confirmed empirically, not just read: splitting a
real decrypted 3.1 ROM and inspecting the first bytes of `hi.rom`
showed the swapped-then-interleaved pattern exactly (canonical bytes
`11 14 4e f9 00 f8 00 d2 ...` produced `hi.rom` starting `14 11 f8 00
00 00 28 00 ...`, i.e. each output word is the byte-reversal of the
corresponding half of the original longword) — and merging `hi.rom` +
`lo.rom` back together, then applying one 16-bit un-swap pass
(`AmigaROMUtil -u`), reproduced the original decrypted bytes exactly.
So: **the split halves are conventionally byte-swapped relative to the
canonical big-endian image**, not a plain word-interleave of canonical
bytes — a decoder/encoder pair implementing this format needs both
steps (interleave *and* the per-word byte-swap) to match what real
burner tooling expects.

Naming convention: AmigaROMUtil's CLI takes arbitrary `-a`/`-b` paths
for hi/lo with no enforced extension, so it doesn't itself mandate a
naming scheme. Community tooling (a separate, unidentified "SplitROMImage"
utility referenced in a Pure Amiga burning guide, web-searched, not
read as source) is documented as producing `.hi`/`.lo` extensions from
`SplitROMImage <rom> SWAP` — consistent with `.hi`/`.lo` being the
common real-world naming convention, alongside `_high`/`_low` seen in
some ad hoc forum guides. No single naming convention is universal
enough to treat as a hard fact; `.hi`/`.lo` is the best-attested one.

## Empirical verification log (for reproducibility, no copyrighted content)

Real files used (paths only, not reproduced): the seven `.rom` files
and `rom.key` at `~/kicksmash32/cloanto_roms/`. Steps run:

1. `xxd -l 32 <file>` on all seven — confirmed identical 11-byte
   `AMIROMTYPE1` magic, non-identical subsequent (encrypted) bytes.
2. From-scratch Python XOR decode (`payload[i] ^ key[i % len(key)]`,
   payload = `data[11:]`) on all seven — output lengths exactly 256 KiB/
   512 KiB (or non-power-of-two sizes for the two "bonus" module files,
   which aren't full Kickstart images and aren't expected to be).
3. Cross-checked that Python decode byte-for-byte against a separate
   pre-existing local `decrypt-amigaforever` binary's output (GPL-2,
   used only as an output oracle, its 30-line source was read once for
   orientation but is not cited/transcribed as a source of facts here
   per the task's GPL-avoidance instruction — every fact above is
   independently re-derived and cross-checked against the MIT
   AmigaROMUtil source and empirical bytes instead).
4. Built AmigaROMUtil from source (`make`), ran
   `./AmigaROMUtil -i <rom> -k rom.key -d -o decrypted.rom` — matched
   the Python decode.
5. `./AmigaROMUtil -i decrypted.rom -s -a hi.rom -b lo.rom` then
   `./AmigaROMUtil -a hi.rom -b lo.rom -g -o merged.rom` then
   `./AmigaROMUtil -i merged.rom -u -o merged_unswap.rom` —
   `cmp merged_unswap.rom decrypted.rom` reported no differences: full
   split→merge→unswap round trip reproduces the original bytes exactly.
6. `romtool -k rom.key info <file>` (amitools 0.8.1, GPL-3, installed
   in a throwaway venv, run as a black box only) on the 3.1 A3000 ROM
   reported `header ok`, `footer ok`, `size ok`, `chk_sum ok`,
   `is_kick ok`, `base_addr 00f80000`, `boot_pc 00f800d2` — the
   `boot_pc` value matches the `4ef9 00f8 00d2` (`JMP $00F800D2`) bytes
   visible at the very start of the independently-decoded payload,
   triangulating the whole decode chain against a third, independent
   tool.

## Correction to current code

`src/lib.rs`'s `CLOANTO_HEADER` constant and `Loader::detect`'s
`data.starts_with(CLOANTO_HEADER)` check, and the doc comments claiming
"the ASCII banner is plain text and independently confirmed", are
**wrong** and need to change once this research is integrated:

- Replace the 108-byte-ish long copyright-string constant with the
  11-byte magic `b"AMIROMTYPE1"`.
- `decode_cloanto`'s payload slice (`&data[CLOANTO_HEADER.len()..]`)
  becomes correct once `CLOANTO_HEADER` is fixed to the 11-byte magic —
  the slicing logic itself was already right in shape (a fixed-length
  header, no length field to read), just pointed at the wrong constant.
- The doc comment/TODO about "trailing footer bytes, if any, are left
  for the caller to trim" should be deleted: there is no footer to
  trim; the payload runs to end-of-file exactly.
- The existing synthetic round-trip tests (`detect_recognizes_cloanto_header`
  etc.) will need their synthetic fixture's header bytes updated to
  the corrected magic, but their shape (construct a fake header +
  XOR'd payload, assert round-trip) doesn't need to change.

## Remaining unknowns

- Whether Amiga Forever ships any *other* container variant besides
  `AMIROMTYPE1` in newer (post-"Forever 9") releases — all local
  samples and both cross-checked tools agree on this one magic, but
  neither source was checked against every historical Amiga Forever
  version. Low risk: AmigaROMUtil's author states its ROM database was
  "pulled from my copy of Amiga Forever 9" (current at time of research)
  and only this one encrypted-container format is implemented.
  Confidence: PROBABLE for "this is the only variant", CONFIRMED for
  "this is *a*, currently-shipping variant".
- The exact canonical rule for "any size" hi/lo splitting on real
  hardware for sizes other than 256/512 KiB halves wasn't tested against
  a real EPROM burner or real hardware — only against AmigaROMUtil's
  general-purpose code path and Cloanto's generic prose. Mechanically
  solid (CONFIRMED for the algorithm), but no physical-burn
  confirmation for odd sizes (not expected to matter — non-KiB-power
  Kickstart sizes don't occur in practice).
- Whether *all* real-world burner/programmer software expects the
  1032-word-swapped hi/lo convention (B3), or whether some expect a
  plain (unswapped) word-interleave — only one tool (AmigaROMUtil) and
  scattered forum prose were checked. AmigaROMUtil makes swap-before-
  split the unconditional default but also exposes explicit `-p`/`-u`
  swap toggles as first-class separate operations, suggesting real
  users do sometimes want the unswapped variant depending on their
  specific burner/adapter. Treat [`ByteOrder`] as orthogonal to the
  hi/lo split (as PLAN.md's own "Byte-swapped output conventions" item
  already plans to), rather than baking one fixed swap into
  `merge_hi_lo`/`split_hi_lo` — a caller should be able to compose
  normalize (byte order) with hi/lo split/merge independently.
  Confidence: CONFIRMED for "this variant exists and is common",
  UNRESOLVED for "is it the only/dominant convention across all real
  burner software".

## Addendum (milestone-2 harness work): romtool's `-k` quirk

Observed black-box while building the differential harness: amitools
0.8.1's `romtool info -k <path>` does **not** honor the given key path
for a Cloanto-magic image — it looks for a file literally named
`rom.key` in the same directory as the ROM file. The harness works
around it by staging ROM + `rom.key` in one temp dir
(`tests/differential.rs`, `TempRomDir`). Worth remembering when the
CLI crate mirrors the `-k` flag: mirror the *documented* behaviour
(honor the path), not the quirk.
