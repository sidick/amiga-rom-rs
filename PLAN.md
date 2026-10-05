# Implementation plan

The goal is a library that covers everything amitools' `romtool`
(https://amitools.readthedocs.io/en/latest/tools/romtool.html) can do to
a Kickstart ROM image — inspect, normalize, scan, split, build, patch —
implemented independently, so that no consumer ever has to reach around
this crate to a GPL tool. Staged by risk (facts → inspect → scan →
build/patch), matching the `amiga-ffs-rs`/`amiga-rdb-rs` pattern. Boxes
get ticked as they land; anything discovered missing gets *added here
first* so the plan stays the map.

**This repository is the library only.** A CLI consumer (`amiga-romtool`,
binary name `amigaromtool` to avoid clobbering amitools' `romtool` on
`PATH`) is a separate repository that depends on this crate for all
parsing/validation/building logic and only adds file I/O, arg parsing,
and output formatting. Where a `romtool` subcommand is *mostly* I/O and
formatting (e.g. `dump`, `diff`), the plan says so and keeps the library
side minimal rather than inventing API for the CLI's convenience.

`no_std` + `alloc`, zero dependencies, MIT OR Apache-2.0, MSRV 1.63 —
all matching the sibling crates. No file I/O: callers pass `&[u8]` /
`&mut [u8]`.

## Why independent, not a port

`amitools` is GPL-3 (same reasoning as `amiga-ffs-rs`/`amiga-rdb-rs`
existing at all — no permissively-licensed equivalent existed). So
`amiga-rom` is implemented against the *public* Kickstart ROM
header/footer format (Amiga Hardware Reference Manual, NDK includes for
struct layouts, well-documented community write-ups), not transliterated
from `KickRomAccess.py`/`RomImage.py`. Functionally equivalent output,
independent implementation.

**Source discipline, the same rule as `amiga-rdb-rs`:** GPL tools
(amitools) are run as *differential oracles* — create inputs, observe
outputs, match behaviour — never read as a source to port from.
Permissively-licensed code (kicksmash32, BSD-2-Clause; AmigaROMUtil,
MIT) may be read directly as a cross-check. Struct layouts and byte
offsets are facts, not expression, and come from the NDK includes and
hardware documentation.

## Split-data catalog (Remus/Romsplit) — deferred, license confirmed restrictive

The module-boundary catalog `romtool split`/`list`/`query`/`build` depend
on comes from Doobrey's Remus/Romsplit tools. amitools' own repo ships
the data files with only an attribution README — no license grant there
(separate from amitools' own GPL-3 on its code).

**Checked directly against the actual archives (Remus 1.81, ROMsplit
1.30, downloaded from doobreynet.co.uk/beta/).** `Remus.guide`'s Legal
node states the data files are Copyright(c) 2004-2021 Doobrey and that
"the Licensee acknowledges not to redistribute the Software (or any part
of) without express permission from the Licensor." `ROMsplit.guide`
separately states "Copying/redistribution in whole or part by any means
is not permitted without permisson." Confirmed, not just absent —
explicit all-rights-reserved, no bundling.

**Also checked SKick 346** (Pavel Troller/SinSoft — a related but
distinct tool: whole-ROM relocation for soft-kicking, used by WHDLoad's
kickemu, not per-module splitting). Same shape of restriction:
`SKick.doc` states the archive "may be freely redistributed, but only in
totally unchanged state," i.e. no extracting/reusing the `.RTB`/`.PAT`
data files individually. Useful anyway as independent confirmation that
building this kind of table by hand is tractable — its `.RTB` files open
with a 4-byte KickSum header (matching what `amiga-rom` computes for
free) followed by a dense, delta-encoded relocation-offset stream, not
full 32-bit addresses. Format observed, not reused.

**Further independent elaboration of that same delta encoding**, found
post-0.4.0 at <http://capitoline.twocatsblack.com/index.php/skick-rtb/>
(a third party's own description of the on-disk `.RTB`/`.PAT` shape for
interoperability with `skick`, including that page's own public-domain
example C validator — not Troller's restricted docs, read only as a
black-box format description, same discipline as the SKick 346 check
above): each reloc position is stored as a *delta* from the previous
one — a single byte if `<256`, else a `0x00` sentinel byte followed by
a word-aligned big-endian 2-byte delta (forcing the stream to stay
word-aligned, inserting a padding `0x00` when needed) — terminated by
four zero bytes, with an optional second section (flagged by
`0xFFFFFFFF`) for the early `dos.library`'s BCPL-style relocs (stored
×4 smaller, BCPL's own addressing convention), itself terminated by
four zero bytes. `.PAT` files are simpler: a checksum header followed
by `(offset, 4 replacement bytes)` pairs, zero-terminated — i.e.
exactly this crate's own `PatchOp`/`apply_patches` shape (milestone 5),
independently corroborating that design rather than suggesting a
change to it. Still format observed, not reused — milestone 6 is where
an independent *generator* for this kind of data, if ever attempted,
would live, not here.

**Methodology note for milestone 6**, from the same site's
`analysing-unknown-roms` page: two techniques for identifying module
boundaries and RELOCs without restricted data — (1) scanning for
`RTC_MATCHWORD` and cross-referencing a self-built database of known
modules by CRC32 (their own "hash file" scheme), and (2) finding RELOCs
by diffing two ROMs that contain the *same* library loaded at two
*different* addresses — any byte position that differs, at the same
relative offset within the library, and by exactly the address delta
between the two ROMs' load points, must be a RELOC. Both are
independently derivable from first principles (no restricted-data
access implied); worth revisiting when milestone 6 is actually
attempted.

Given both: **don't bundle any of this.** When split/build (milestone 4)
is tackled, the catalog loads from a **pluggable format the user
supplies**, not shipped in the crate. Emailing Doobrey and/or
Troller/Fabre for express permission is a live option — cheap to ask,
contact details are right in their docs — but not a blocker for
anything earlier.

## The other bundling constraint: ROM images themselves

Kickstart ROMs are Commodore/Amiga copyright material (now Cloanto/Amiga
Corporation's) and **cannot ship in this repo, not even as test
fixtures** — the same reason Cloanto's `rom.key` never ships. Test
strategy follows from this and shapes the whole suite:

- **Synthetic fixtures**: tests construct minimal images in code — a
  valid header, footer, size field and sealed checksum around zero
  filler — exercising every check without a byte of Commodore code.
  This is the rdb-crate approach (its fixtures are built by `seal()`
  helpers, not shipped images) and it works here for everything except
  "does a real 33.180 ROM report 33.180".
- **Env-gated real-ROM tests**: `AMIGA_ROM_DIR=/path/to/roms cargo test
  real_rom` runs assertions against whatever legally-owned dumps the
  developer points it at, identified by their checksums, skipping
  silently when unset (same shape as `AMIGA_RDB_DIFFERENTIAL`). CI does
  not have ROMs; these are local-only.
- **The differential oracle needs no ROMs either**: amitools' `romtool`
  is happy to run `info` on a *synthetic* image, so "our RomInfo ==
  romtool's info output, field for field" runs in CI against images
  this crate builds — pinned amitools version, env-gated
  (`AMIGA_ROM_DIFFERENTIAL=1`), matching the rdb crate's harness.

## Already landed (0.1 scaffolding)

- [x] Crate skeleton: `no_std` + `alloc`, `std` feature (Error impls
      only), zero deps, MSRV 1.63, dual license, PLAN/CLAUDE/README
- [x] API surface fixed as stubs: `Loader::detect`/`normalize`,
      `ByteOrder` (4 modes), `RomEncoding`, `LoaderError`,
      `merge_hi_lo`/`split_hi_lo`, `KickRom` with every check/value
      method, `RomInfo` aggregate — unimplemented legs are `todo!()`
      with the missing fact named in the doc comment, so nothing
      reports a false "ok"
- [x] `KickRom::check_size` (256/512 KiB)
- [x] Cloanto banner detection + XOR-decode (`decode_cloanto`), with
      synthetic round-trip tests
- [x] `checksum_ones_complement` — the end-around-carry longword sum,
      the one checksum primitive everything else must route through

## Milestone 1 — the facts pass

Nothing ships out of this milestone but confirmed byte-level facts,
recorded *here* with their source cited, and the test fixtures that
encode them. Every later milestone builds on these; guessing any of
them is how a checker reports "ok" on garbage. Each item below names
the fact to establish and the acceptable sources — the plan
deliberately does **not** state the offsets as if already known.

- [x] **ROM header layout.** CONFIRMED, oracle-verified + parent-session
      spot-checked on three real ROMs (1.3, 3.1, 3.2.2). Full report:
      `docs/research/header-footer-facts.md` §1. First 0x18 bytes:
      0x00 u16 marker word (`0x1111` 256 KiB-era / `0x1114` 512 KiB —
      acceptance is *size-dependent*: `0x1111` rejected on a 512 KiB
      image; this also settles that the leading word is a marker, not
      an SSP fragment as the byte-order report's PROBABLE reading
      guessed); 0x02 u16 = `0x4EF9` (`JMP abs.L`, required exactly);
      0x04 u32 = the JMP target = **`boot_pc`** (there is no separate
      boot_pc field); 0x08 u32 = `0x0000FFFF` constant (purpose
      UNRESOLVED, presence confirmed on 5 ROMs); 0x0C u16+u16 =
      **`rom_rev`**; 0x10 u16+u16 = **`exec_rev`**; 0x14 u32 =
      `0xFFFFFFFF` separator. **`base_addr` is computed, never
      stored: `boot_pc & 0xFFFF0000`** — proven by synthetic ROMs at
      three different base highs and both sizes, and by a real
      1.4-beta dump reporting the oddball `0x00FF0000`.
- [x] **ROM footer layout.** CONFIRMED
      (`docs/research/header-footer-facts.md` §2). Last 24 bytes:
      `len-24` u32 stored checksum; `len-20` u32 size field (== actual
      image length); `len-16..` eight u16 words nominally
      `0x0018..0x001F` (the m68k exception-vector *indices* 24–31 as
      literal data). Tolerance isolated by oracle: the first word
      (`len-16`) is *not* checked (real 1.3 ROMs carry `0xFFC0` there
      and still pass), the remaining seven are. **Surprise with an
      implementation consequence:** romtool's printed `size_field`
      line never goes NOK under any mutation — yet `is_kick` fails
      when the field doesn't match the real length. `is_kick` therefore
      enforces a requirement no printed check line exposes; see the
      milestone-2 `is_kick_rom` item.
- [x] **Stored-checksum location + exact KickSum convention.**
      CONFIRMED (`docs/research/header-footer-facts.md` §3): stored
      big-endian u32 at `len-24`; whole-image end-around-carry sum
      (checksum field included) == `0xFFFFFFFF`, verified on five
      real ROMs (two independent 1.3 dumps, 2.04, 3.1, AROS) and
      re-verified by the parent session on 3.2.2. The "negated-sum
      variant" hypothesized here earlier is **algebraically the same
      formula**, not a competing convention — verified numerically on
      all five. `checksum_ones_complement` is exactly right and needs
      no variant branch.
- [x] **"Kickety split" signature.** CONFIRMED mechanism
      (`docs/research/header-footer-facts.md` §4): the check looks for
      the marker word + `0x4EF9` at the image **midpoint**
      (`size / 2`), with two wrinkles — it is *stricter* than the
      offset-0 header check (only `0x1111` accepted at the midpoint,
      not `0x1114`), and the JMP target word there is not checked at
      all. **Not part of `is_kick`** (confirmed both ways). Which
      real ROMs have it is *not* era-determined (3.1 does; 2.04,
      3.0-ish and 3.2.2 don't) — PROBABLE territory, informational
      only, not blocking.
- [x] **Magic reset opcode.** CONFIRMED
      (`docs/research/header-footer-facts.md` §5): `0x4E70` (m68k
      `RESET`) at **fixed absolute offset 0xD0**, independent of
      `boot_pc` (oracle: moving `boot_pc` away doesn't flip it) —
      four real ROMs including AROS carry it there. **Not part of
      `is_kick`** (confirmed by forcing it NOK on an otherwise-valid
      image).
- [x] **Where `exec_rev` comes from.** RESOLVED: **fixed header
      offset 0x10** — not resident-derived. Proven by the strongest
      oracle test in the set (`docs/research/header-footer-facts.md`
      §6): three synthetics varying only offset 0x10 tracked 1:1,
      and a planted fake `Resident` (valid `0x4AFC` matchword,
      self-pointing `rt_MatchTag`, bogus version 99) had zero effect
      on the reported value. Per this item's own branching rule,
      `exec_rev` lands in **milestone 2** with the other direct
      offset reads; no `ResidentScan` dependency.
- [x] **Boot-vector signature table for byte-order detection.**
      Transcribed from kicksmash32's `detect_byte_order()`
      (`sw/hostsmash.c:2008-2016` at commit `a343550`, BSD-2) — full
      table with per-family commentary in
      `docs/research/byte-order-facts.md`. Seven rows; the four
      *distinct* first longwords are `11144ef9` (Kickstart 2.04+, also
      ROM Switcher), `11114ef9` (1.3, also Logica-Dialoga and AROS),
      `612e4447` (DiagROM 2.x Beta), `11144447` (DiagROM 2.x); the
      second longword of each pair is that family's boot-PC value
      (`00f800d2` for 2.04+, `00fc00d2` for 1.3, etc.).

      **Decision: match the first longword only, then shape-check the
      second — deliberately neither of kicksmash32's two behaviours.**
      Its git history shows the pair-shaped table never had its second
      column read in any revision (introduced already-unused in the
      commit "Improved auto byte order swapping to detect more ROM
      types"), and the data explains why that's right, not a bug: LW0
      (ROM magic + `4EF9` `JMP xxx.L` opcode) is era-stable across
      every 2.04–3.2 build, while LW1 is the *build-specific* jump
      target — exact-matching LW1 would reject any ROM version not in
      the table. So: exact-match LW0 against the distinct values under
      all four permutations, then require the un-permuted LW1 to look
      like a plausible ROM-space address (`0x00F8xxxx`/`0x00FCxxxx`)
      rather than any exact value — stricter than kicksmash32 against
      false positives, no looser against unknown ROM builds. LW1
      values kept in the table as provenance.
- [x] **Cloanto container framing, precisely.** CONFIRMED — and it
      corrected this plan's working hypothesis. Full report:
      `docs/research/cloanto-hilo-facts.md`, triangulated three ways
      (byte inspection of seven real Amiga Forever ROMs + `rom.key`,
      AmigaROMUtil source read (MIT, `Kreeblah/AmigaROMUtil` — not
      Hyperkid123 as earlier drafts guessed), amitools run as a
      black-box oracle). The container header is **not** the long
      copyright banner — that text lives *inside the decoded
      Kickstart payload* (at payload offset 0x1E in the 3.1 A3000
      ROM, wording varying by ROM revision). The real framing: an
      11-byte ASCII magic **`AMIROMTYPE1`** (no NUL, no padding, no
      length field), payload = `data[11..]` to end-of-file exactly
      (**no footer, nothing to trim** — decoded length is exactly
      256/512 KiB), XOR'd as `payload[i] ^ key[i % keylen]` with the
      key cycling from index 0. Leading-magic match is the sole
      detection signal every independent tool uses; no stricter
      secondary check exists. `src/lib.rs` corrected accordingly
      (`CLOANTO_MAGIC`). Open (PROBABLE): whether any post-Forever-9
      release ships a different container variant.
- [x] **Hi/lo EPROM interleave width.** CONFIRMED — **16-bit-word
      interleave, not byte-split**: for each canonical longword
      `b0 b1 b2 b3`, the hi file takes `b0 b1` and the lo file takes
      `b2 b3` (AmigaROMUtil `SplitAmigaROM`/`MergeAmigaROM`,
      `AmigaROMUtil.c:1611-1615`/`:1670-1674`; verified by a full
      decrypt→split→merge→un-swap round trip on a real 3.1 ROM
      reproducing the original bytes exactly). Applies to any
      size % 4 == 0, not just 512 KiB. One wrinkle for the API:
      burner tooling conventionally applies a `1032` per-word
      byte-swap *before* interleaving (AmigaROMUtil does it
      unconditionally in split), but it also exposes swap as a
      separate first-class operation — **decision: `merge_hi_lo`/
      `split_hi_lo` do the pure canonical word-interleave only, and
      callers compose with the `ByteOrder` machinery for
      burner-ready output**, keeping the two axes orthogonal
      (whether the swapped convention is universal across burners is
      UNRESOLVED, so don't bake it in). `.hi`/`.lo` is the
      best-attested naming convention (CLI concern, noted for the
      consumer crate). Details: `docs/research/cloanto-hilo-facts.md`.
- [x] **Byte-swapped output conventions.** CONFIRMED from
      kicksmash32's mode constants, CLI docs and errata
      (`docs/research/byte-order-facts.md` §3): `0123` is the
      distributed/reference file format; `3210` is what
      A3000/A4000-class 32-bit ROM sockets want; `1032` is the
      16-bit-socket format (A500/A600/A2000, "likely also A1200" per
      kicksmash32's own hedge) **and** the A3000T, which is a
      documented board-wiring erratum despite its 32-bit bus
      (`rev7_3kt/errata.txt`); `2301` has **no confirmed real
      hardware target** — it exists as a detectable permutation
      only. (`dd conv=swab` ⇄ `1032` is a community equivalence, not
      in kicksmash32.) API consequence, as anticipated: `ByteOrder`
      names permutations ("what a dump looks like"), and any
      "what should I burn for machine X" mapping is a consumer-crate
      table, not this enum's semantics.
- [x] **Fixture builder.** Landed: `#[cfg(test)] mod fixtures` in
      `src/lib.rs` — `RomFixtureParams` (size, rom_rev, exec_rev,
      base_addr, with per-size defaults) and
      `synthetic_rom(params) -> Vec<u8>`, transcribing the header/
      footer/magic-reset facts and sealing via
      `checksum_ones_complement` (store the complement of the
      zeroed-field sum; `S + !S` folds to `0xFFFFFFFF` with no carry
      edge case, `debug_assert`ed after sealing). **Exit criterion
      met and independently re-verified by the reviewing session**:
      amitools 0.8.1 `romtool info` reports `is_kick ok` (and every
      gating check ok, `kickety_split NOK` as expected) on both a
      256 KiB and a 512 KiB fixture, with reported
      base_addr/boot_pc/rom_rev/exec_rev matching the built
      parameters exactly — the facts are proven right with zero
      copyrighted bytes in the repo. Milestone 1 complete.

## Milestone 2 — `info` complete (inspect + normalize)

`romtool info` parity: every `KickRom` stub becomes real, and
`Loader` normalizes everything a user is likely to actually have.
Ordered so each item's tests exist before or with it.

- [x] **Checksum verify + seal.** `verify_check_sum` wired to
      `checksum_ones_complement` and the confirmed stored-checksum
      offset; plus the write-side inverse now rather than in
      milestone 5 — `seal_checksum(rom: &mut [u8])` computing and
      storing the value that makes the image sum valid. The pair is
      trivial once the facts exist, every fixture needs the sealer,
      and it is this crate's `checksum_ok`/`seal_checksum` analogue:
      one primitive pair, no hand-rolled sums elsewhere.
- [x] **Header/footer/size-field checks + value reads**:
      `check_header`, `check_footer`, `check_size_field`,
      `read_check_sum`, `base_addr`, `boot_pc`, `rom_rev`,
      `exec_rev` — direct transcription of the milestone-1 facts
      (all offsets confirmed; `exec_rev` proven fixed-offset, so it
      lands here, not with scan). Every read is bounds-checked:
      `KickRom` accepts *any* `&[u8]` (including empty and
      truncated), checks return `false` rather than panicking, value
      reads return `Option`/documented sentinel — **decide the exact
      shape here**: `romtool` prints values even for broken images,
      so value methods likely become `Option<u32>` with `RomInfo`
      mirroring that, and the current bare-`u32` stubs change
      signature. This is the moment to fix it, before consumers
      exist. Two wild-sample caveats from the 52-image local sweep
      (research doc addendum): pre-1.2 ROMs carry an unpopulated
      `0xFFFF.0xFFFF` `rom_rev` (don't invent meaning for it), and
      romtool's size-dependent marker rule makes `check_header`
      report NOK on the genuine 1.4-beta ROM (`0x1111` at 512 KiB)
      — match romtool for parity, document the known false-negative.
- [x] **`check_kickety_split`, `check_magic_reset`, `is_kick_rom`**
      per the confirmed facts. The conjunction is now settled by
      oracle (milestone 1 §4/§5): `is_kick` = size ∧ header ∧ footer
      ∧ (size field == actual length) ∧ checksum — **neither
      kickety-split nor magic-reset is part of it** (the scaffolding
      stub wrongly included magic-reset; fixed when the facts landed).
      One differential-harness consequence: our `check_size_field`
      reports the truth (field == length), but romtool's printed
      `size_field` line is cosmetic and never goes NOK — so the
      oracle comparison must treat that one field as
      "ours-is-stricter" rather than expecting equality on corrupt
      images; `is_kick` itself still agrees, which is the check that
      matters.
- [x] **`check_doubled`** — implemented: `KickRom::check_doubled`,
      `RomInfo::doubled_ok`, wired into `info()`. Informational only,
      not part of `is_kick_rom`. 5 new tests (both-halves-identical,
      native-512K-not-doubled, wrong sizes, one-byte-difference,
      `info()` wiring). No differential-harness change needed —
      `doubled_ok` has no `romtool` line to compare against, and the
      harness already compares field-by-field rather than
      exhaustively, so it simply never references the new field. A
      gap found post-0.3.0: real Kickstart 1.3
      dumps commonly show up padded to 512 KiB by literally
      concatenating the 256 KiB image with itself (not the same thing
      as `kickety_split`, which only checks 4 bytes — the marker word
      + JMP opcode — at the midpoint; a doubled image trips
      `kickety_split` too, since the duplicate header is real, but a
      512 KiB image can satisfy `kickety_split` without being doubled
      — they're separate facts). **Confirmed locally**: this repo's
      own `KICK13.ROM` fixture
      (`~/src/external/Copperline/test-assets/KICK13.ROM`, 512 KiB) has
      its first and second 256 KiB halves byte-for-byte identical —
      verified directly, not assumed. Add `KickRom::check_doubled(&self)
      -> bool` (`data.len() == ROM_SIZE_512K && data[..256Ki] ==
      data[256Ki..]`) and a `doubled_ok` field on `RomInfo`, naming
      matched to this crate's existing `_ok` convention
      (`kickety_split_ok`, `magic_reset_ok`) rather than any external
      reference's field name. Informational only, like
      `kickety_split_ok`/`magic_reset_ok` — not part of `is_kick_rom`'s
      conjunction, same reasoning: whether a ROM happens to be padded
      by duplication says nothing about its own validity.
- [x] **`RomInfo` + differential oracle.** `info()` aggregate;
      env-gated `AMIGA_ROM_DIFFERENTIAL=1` test comparing field-for-
      field against pinned amitools `romtool info` over the synthetic
      fixtures (valid, wrong-size, corrupted-checksum, no-footer …),
      wired into CI exactly like the rdb crate's rdbtool harness.
- [x] **Byte-order detect + reorder.** `Loader::detect` over the
      signature table under all four permutations;
      `Loader::normalize` reordering to canonical. Property tests:
      reorder is self-inverse for 1032/2301/3210, round-trips for
      all four, detect(normalize(x)) == Normal, and every
      permutation of a valid fixture detects correctly.
- [x] **Cloanto decode, finished.** Exact framing from milestone 1
      replaces the current banner-prefix approximation (payload
      extent, footer trim — the current decoder's "caller trims the
      footer" note is a stub-era wart that goes away here). Key
      errors distinguished: `KeyRequired` vs an empty key
      (`InvalidKey`, replacing the current `debug_assert`). Synthetic
      encode→decode round-trip test; env-gated real-key test.
- [x] **`merge_hi_lo`/`split_hi_lo`** per the confirmed interleave;
      split→merge round-trip property test; `LoaderError` variants
      for size mismatches settled (`MismatchedHiLoLength` exists;
      likely add odd-size / not-a-ROM-size).
- [x] **ReKick/ReCode, KickIt, and KICK floppy container formats** —
      new item, added post-0.4.0 after a reader pointed at
      <http://capitoline.twocatsblack.com/>, a hobbyist Kickstart
      editing tool's documentation site (plain HTTP only — fetch with
      `curl`, not `WebFetch`, which force-upgrades to `https://` and
      gets `ECONNREFUSED`). Three more raw-dump containers, all decoded
      key-free so they slot into `Loader::detect`/`normalize` the same
      way the byte-order permutations do (unlike Cloanto, which needs a
      caller-supplied key). Full facts, with the explicit caveat that
      this rests on a single secondary source (not oracle-verified, no
      real sample file available to this project):
      `docs/research/rekick-kickit-facts.md`.
      - **`RomEncoding::ReKickEncoded`** ("DEADFEED" format): a
        108-byte plaintext header (the same copyright-banner text
        confirmed at a different offset in milestone 1's Cloanto
        research) precedes a self-keyed chained-XOR payload — block 0
        XORed against the fixed constant `0xDEADFEED`, every later
        block XORed against the *previous block's decoded plaintext*.
        `Loader::detect` matches a stable prefix of the banner (not
        the full 108 bytes, since the copyright year range varies by
        ROM revision) and requires the remaining length to be exactly
        256 KiB or 512 KiB before classifying — `decode_rekick` then
        unconditionally succeeds (no key, no error variant needed).
      - **`RomEncoding::KickItWrapped`**: a trivial 8-byte header (four
        zero bytes, then a big-endian size) wrapping an
        already-canonical softloaded image (used for non-MMU machines
        loading a Kickstart to a non-standard RAM address). Detected
        only when the declared size is exactly 256 KiB/512 KiB and
        matches the actual remaining length; `normalize` just strips
        the 8 bytes.
      - **`RomEncoding::KickFloppyWrapped`** — added in a follow-up
        session after the user pointed at the same site's `physical`
        page: the A1000's non-DOS "KICK" bootstrap floppy (its own tiny
        8 KiB ROM loads a 256 KiB Kickstart from floppy into RAM at
        boot, since the A1000 has no Kickstart ROM socket at all) is
        identified by a 4-byte `"KICK"` magic at offset 0, with the
        Kickstart payload at a fixed offset of 512 bytes. Detected only
        when the remaining length is exactly 256 KiB (the A1000's only
        Kickstart size — unlike KickIt, there's no size field to
        cross-check, so this is the sole guard against misclassifying
        an unrelated file); `normalize` strips the 512-byte header.
        Weaker-sourced than ReKick/KickIt even by this item's own
        PROBABLE standard — no second-page cross-check exists for it
        the way `hash-files` corroborated ReKick's header length.
        **Explicitly scoped out, considered and rejected in the same
        session**: the A3000 SuperKickstart floppy (`"KICKSUP0"`,
        bundles two Kickstarts + two Bonus blobs, doesn't fit a single
        `Vec<u8>` result) and DOS-formatted relocation floppies
        (Relokick/Tude, need a real filesystem parser — `amiga-ffs-rs`'s
        job, not this crate's) — both documented in the research doc's
        "KICK floppy container" section as deliberate non-goals for
        this pass, not oversights.
      - **Deliberately not done** (all three formats): no encode
        direction (same reasoning as Cloanto — this crate reads real
        dumps, it doesn't produce distributable output in any of these
        formats), and no attempt to generalize beyond the one
        documented header length/shape/offset of each — a different
        real-world variant is a reason to extend this item, not to
        guess ahead of evidence.
      - 9 new unit tests total across the three formats
        (detect/reject/round-trip each); covered by the existing fuzz
        target automatically, since it already calls
        `Loader::detect`/`normalize` on arbitrary bytes — no
        fuzz-target code change needed for the new branches.
- [x] **Error shape audit.** `LoaderError::NotYetImplemented` is
      deleted (it exists only to keep stubs honest); remaining
      variants reviewed against "every failure a caller can act on
      is distinguishable"; `Display` for all, `std::error::Error`
      under the feature — already the convention, re-checked once
      the real variants exist.
- [x] **Fuzzing starts here, not later.** `cargo-fuzz` target:
      arbitrary bytes → `Loader::detect`, `Loader::normalize` (with
      and without a key), `KickRom::info` — must never panic, never
      overflow, never allocate absurdly. The rdb crate found two
      real hostile-input crashes this way; assume this crate has
      some too. 60-second smoke in CI, corpus seeded with the
      synthetic fixtures. (Until the last `todo!()` in a fuzzed path
      is gone, the target simply can't be written — which is itself
      the argument for finishing milestone 2 before growing API.)
- [x] **Publish 0.2.0** once `info` parity holds: the crate is
      already useful (emulators and dump tools want exactly
      normalize+identify), and publishing early is the sibling
      crates' pattern.

## Milestone 3 — scan (residents)

`romtool scan` parity: walk the ROM's Resident (RomTag) structures.
This is also what `list`-like identification and any future
split-by-module work stands on. (`exec_rev` turned out *not* to live
here — milestone 1 proved it's a fixed header read, so it ships with
milestone 2.)

- [x] **`Resident` layout from the NDK.** CONFIRMED — transcribed
      directly from `exec/resident.h` (NDK 3.2 R4, Hyperion/Commodore,
      permissively available for exact quotation — no oracle needed,
      this is a struct-layout fact, not an empirical one) and
      `exec/nodes.h`. m68k structs pack tight at natural (word/byte)
      alignment, no compiler padding — every multi-byte field below
      lands on an even offset with zero gaps, so `sizeof(Resident) ==
      0x1A` (26 bytes) exactly:

      | Offset | Width | Field | Notes |
      |---|---|---|---|
      | 0x00 | u16 | `rt_MatchWord` | must equal `RTC_MATCHWORD = 0x4AFC` (the 68000 `ILLEGAL` opcode — deliberately a trap if ever executed) |
      | 0x02 | u32 (APTR) | `rt_MatchTag` | self-pointer: must equal this struct's own *absolute ROM address* (offset 0x00 of this same structure) |
      | 0x06 | u32 (APTR) | `rt_EndSkip` | absolute address to resume scanning after this module |
      | 0x0A | u8 | `rt_Flags` | `RTF_AUTOINIT` (bit 7, `0x80`): `rt_Init` points to an auto-init data structure, not code, directly relevant to this crate since dereferencing it differently changes what `rt_Init` means; `RTF_AFTERDOS` (bit 2), `RTF_SINGLETASK` (bit 1), `RTF_COLDSTART` (bit 0) — not needed for a scanner, recorded for completeness |
      | 0x0B | u8 | `rt_Version` | release version number (single byte — coarser than the header's `rom_rev`/`exec_rev` pairs) |
      | 0x0C | u8 | `rt_Type` | `NT_LIBRARY=9`, `NT_DEVICE=3`, `NT_RESOURCE=8`, `NT_PROCESS=13`, plus the full `NT_*` set from `nodes.h` (0=UNKNOWN..19=DEATHMESSAGE, 254=USER, 255=EXTENDED) |
      | 0x0D | i8 | `rt_Pri` | signed initialization priority |
      | 0x0E | u32 (char*) | `rt_Name` | absolute pointer to a NUL-terminated name string in ROM |
      | 0x12 | u32 (char*) | `rt_IdString` | absolute pointer to a NUL-terminated ID string in ROM |
      | 0x16 | u32 (APTR) | `rt_Init` | meaning gated by `RTF_AUTOINIT`: plain init-code pointer when clear, auto-init table pointer when set |

      **Pointer translation, the load-bearing consequence for
      `ResidentScan`**: every pointer field (`rt_MatchTag`,
      `rt_EndSkip`, `rt_Name`, `rt_IdString`, `rt_Init`) is an
      *absolute Amiga address*, not a file offset — exactly the
      `base_addr` (milestone 2) translation problem the milestone-3
      item below already anticipates. `rt_MatchTag == this_struct_addr`
      is therefore the validity check: compute the candidate's own
      absolute address as `base_addr + candidate_offset`, and require
      the stored `rt_MatchTag` value to equal it exactly — a strong
      self-consistency check with no separate "known good" table
      needed, unlike the header signature problem in milestone 1.
- [x] **`ResidentScan`**: iterate matchwords over the image,
      validate each candidate's `rt_MatchTag` points back at itself
      (ROM-address-space aware: pointers are absolute addresses in
      the ROM's mapped range, so `base_addr` from milestone 2 is
      what turns a pointer into an image offset — get this
      translation right once, in one function), yield a borrowed
      `Resident<'a>` per hit with name/idstring resolved as
      `&[u8]`-with-`Display` (ROM strings are not guaranteed UTF-8;
      don't pretend they are). Allocation-free iterator, consistent
      with `KickRom`.
- [x] **Hostile-input discipline**: a matchword at the last word of
      the image, self-pointers outside the image, strings running
      off the end, a `rt_EndSkip` pointing backwards — all yield
      "not a resident" or a truncated-but-typed result, never a
      panic. Covered by 9 synthetic unit tests (`scan_tests`) and
      added to `fuzz/fuzz_targets/parse.rs`
      (`rom.scan().collect::<Vec<_>>()`, proving both no-panic and
      termination on adversarial input) — 13M further fuzz executions
      with the new coverage, zero crashes.
- [ ] ~~**`exec_rev` resolved**~~ — moved to milestone 2 (milestone 1
      proved it's a fixed read at header offset 0x10, not
      resident-derived); only `RomInfo` finalization remains here if
      scan adds fields.
- [x] **Oracle**: sanity-checked black-box against real `romtool scan`
      (no source read) on 3.1 A3000 — **exact match, 42/42 residents**,
      same offsets/names/versions/end_skip values. Parent session
      independently re-ran against two more real ROMs (2.04 A3000: 42
      residents; 3.0 A1200: 43), all with plausible names
      (exec.library, graphics.library, expansion.library, ...) and no
      panics. A committed, env-gated `tests/real_roms.rs`-style
      assertion (rather than this ad hoc check) is a natural follow-up
      but not required to consider this item done — the committed unit
      tests already cover the algorithm exhaustively with synthetic
      fixtures (`scan_tests`, 9 tests: valid hit, wrong self-pointer,
      truncated matchword, multiple hits, string resolution in/out of
      bounds, EndSkip-not-trusted, base_addr-unknown).
- [x] **`dump`/`diff` stay CLI-side.** `dump` is hex formatting of
      bytes the library already hands over; `diff` is a byte/field
      comparison of two normalized images plus formatting. The
      library's contribution is `Loader::normalize` + `RomInfo` +
      `ResidentScan`, which all exist by now; no new API unless the
      CLI proves something missing (in which case it gets added here
      first).
- [x] **Machine identification** — new item, raised post-0.3.0 (not
      in `romtool` at all; this crate's own addition, grilled
      2026-09-15). Two complementary pieces, both decided:

      **1. `KickRom::machine_hints` — resident-based heuristic, built
      entirely on the existing `ResidentScan` primitive, no new
      low-level parsing.** Confirmed empirically against five real 3.1
      ROMs (A600/A1200/A3000/A4000/A4000T, this session) that
      Commodore embeds real machine-identifying signal in resident
      names:
      - A **literal machine-name resident**: the A3000 ROM carries a
        resident named exactly `"A3000 bonus"`; A4000 and A4000T carry
        `"A4000 bonus"` (confirmed present even back in the 2.04 A3000
        ROM, not just 3.1). Strip the trailing `" bonus"` for the
        machine name.
      - `card.resource`/`carddisk.device` (PCMCIA) present only on
        A1200 and A600 — absent on the A3000-class machines.
      - `NCR scsi.device` present only on A4000T, alongside the
        `scsi.device` every machine has — its distinct SCSI
        controller.

      **Explicitly a heuristic, not a fact** — unlike everything else
      in this crate, it's incomplete by construction: plenty of real
      ROMs (2.04-era non-A3000 dumps not yet sampled, CD32, CDTV,
      AROS) may carry none of these markers, and `machine_hints` on
      such an image correctly returns all-`None`/`false`, not a wrong
      guess. Shape: a small struct (`named_machine: Option<&[u8]>`,
      `has_pcmcia: bool`, `has_ncr_scsi: bool`) returned by a method
      that iterates `self.scan()` once. No restricted data involved —
      these are plaintext module names Commodore put in the ROM,
      visible to anyone who scans it.

      **2. `identify`/`KnownRom` — checksum-keyed lookup, mechanism +
      a small seed table.** Same shape as milestone 4's already-grilled
      `ModuleSpec` precedent (plain data, no trait — `identify(check_sum:
      u32, table: &[KnownRom]) -> Option<KnownRom>`), because the
      checksum-only case (`machine_hints` needs the actual ROM bytes;
      this doesn't) is a genuinely different use case worth its own
      mechanism, not because polymorphism is needed. **Not the
      milestone-4 licensing situation** — checksum→version/machine
      metadata is publicly documented (Cloanto's own "ROM Types
      Summary" page, already cited in this project's Cloanto
      research; Wikipedia's Kickstart version history), unlike
      Doobrey's restricted split-boundary data, so a small seed table
      ships in the crate rather than staying caller-supplied-only.
      `devices: &[&str]` on each entry is the ROM's resident name list
      — genuinely useful for the checksum-only case (no bytes to scan
      yet), redundant with `ResidentScan` when the bytes *are* in
      hand.

      **Seed table — 7 entries, each independently verified this
      session** (stored `check_sum` read directly, `rom_rev`/`exec_rev`
      cross-checked against `info()`, `devices` from an actual
      `scan()` run — not copied from any external database):

      | check_sum | machine | rom_rev | exec_rev |
      |---|---|---|---|
      | `0x150B7DB3` | A3000 | 34.5 | 34.2 | (Kickstart 1.3)
      | `0x54876DAB` | A3000 | 37.175 | 37.132 | (2.04)
      | `0x87BA7A3E` | A1200 | 40.68 | 40.10 | (3.1)
      | `0x8F4C0C67` | A3000 | 40.68 | 40.10 | (3.1)
      | `0x45C3145E` | A4000 | 40.68 | 40.10 | (3.1)
      | `0x47BCEC13` | A4000T | 40.70 | 40.10 | (3.1)
      | `0x9FDEEEF6` | A600 | 40.63 | 40.10 | (3.1)

      "Small, seeded, extendable" per the grilled decision — not an
      attempt at exhaustive coverage (that would be a maintenance
      commitment this crate hasn't signed up for); a caller with more
      entries passes their own `&[KnownRom]` to the same `identify`
      function.

      **Implemented and re-verified against all 7 real ROM files this
      session touched** (not just the synthetic fixtures the unit
      tests use): every `identify(read_check_sum(), KNOWN_ROMS)`
      lookup matched the expected machine exactly, and
      `machine_hints` behaved exactly as documented on the A4000T
      case — `named_machine: Some("A4000")` (the `bonus` resident
      doesn't distinguish the tower variant), `has_ncr_scsi: true`
      carrying the actual differentiator, not a fabricated
      `"A4000T"`. 12 tests; added to the fuzz target.

      **A real false positive found immediately after, on AROS**
      (`aros-20181209.rom`, real local file): `machine_hints` reported
      `has_pcmcia: true` — wrong. AROS's ROM ships a `card.resource`
      resident generically (for broad hardware compatibility), not
      because this build targets real A600/A1200 PCMCIA hardware.
      Investigated and fixed rather than left as a known limitation:

      - **`aros.library`** is a reliable, AROS-specific resident name
        (unlike `card.resource`, essentially no genuine Commodore/
        Hyperion Kickstart would carry it) — added `is_aros: bool`.
      - Several residents' `id_string`s (`exec.library`,
        `expansion.library`, `timer.device`, `battclock.resource`,
        `kernel.resource`, `processor.resource`) carry the literal
        token `"amiga-m68k"` — AROS's own documented `<platform>-<cpu>`
        port-naming convention (confirmed against
        https://aros.sourceforge.io/introduction/ports.html, quoted:
        "AROS/amiga-m68k is the native port for m68k Amigas, or
        emulators like WinUAE... most complete port of AROS"). Added
        `target_platform: Option<&[u8]>`, a borrowed substring match
        against that one confirmed literal token — deliberately not a
        general `<platform>-<cpu>` parser (only one token is confirmed
        against a real sample; other AROS ports use different tokens
        this crate doesn't specifically recognize yet, extend when a
        real sample justifies it, same discipline as everywhere else
        in this crate).
      - **Fix**: `has_pcmcia` is forced `false` when `is_aros` is
        true — documented as a deliberate carve-out, not silently
        dropped. The `uaegfx.hidd`/`ata_gayle.hidd` residents also
        present on this ROM corroborate the reading: this build
        targets m68k Amiga-class systems broadly (real Gayle-equipped
        hardware *or* UAE-family emulators), not one Commodore model,
        which is exactly why it carries generic hardware-support
        residents instead of one machine's subset.

## Milestone 4 — split (modules), scoped down

The catalog-dependent half of `romtool`, gated on the licensing
reality up top: the crate defines a minimal *interface*, users supply
the *data*. **Scoped to `split` only** for this pass (grilled
2026-09-15) — `build` needs hunk parsing/writing and relocation
application, which is real, separable work; doing `split` alone first
proves the module-data shape against something real before that
complexity is taken on.

Design decisions from that session, each with its reasoning:

- **No `ModuleCatalog` trait.** A trait implies polymorphism, and
  nothing today needs it: this crate does zero file I/O (established
  since milestone 1), so a catalog *file format* is the CLI crate's
  concern, not this one's — the "crate ships a documented file
  format" idea from the original draft is dropped. `split` just takes
  plain data: `split(rom: &[u8], modules: &[ModuleSpec]) ->
  Result<Vec<Module>, SplitError>`, where `ModuleSpec` is `{name,
  offset, length}` and nothing else (no relocation data, no "kind"
  tag, no cross-reference to milestone 3's `ResidentScan` — scanning
  and catalog-driven splitting stay unrelated concepts). Introduce a
  trait later only if a second real shape of catalog data shows up to
  abstract over.
- **No catalog-identity matching in this crate.** `split` bounds-checks
  each module range against the ROM it's actually given (a hard
  safety property, matching every other check in this crate), but has
  no opinion on whether the `ModuleSpec` list "belongs" to that ROM —
  matching a catalog to a ROM by KickSum is a lookup step that happens
  entirely outside this crate, before `split` is ever called.
- **No overlap detection between module ranges.** Two `ModuleSpec`
  entries claiming the same bytes is a semantic complaint about the
  catalog's own consistency, not a memory-safety concern (`&rom[a..b]`
  and `&rom[c..d]` overlapping is safe to construct) — out of scope
  here, matching the "no premature abstraction" convention.
- **Output is borrowed, not owned.** `split` never transforms bytes,
  only cuts them, so it returns slices into the input ROM
  (`&'a [u8]` per module), consistent with `KickRom`'s and
  `ResidentScan`'s existing allocation-free convention. A caller
  wanting an owned `Vec<u8>` per module calls `.to_vec()` themselves.
- [x] **Implement per the above.** `ModuleSpec<'a>{name, offset,
      length}`, `Module<'a>{name, data}`, `SplitError::ModuleOutOfBounds
      {index, offset, length, rom_len}` (single variant — one failure
      mode, per the no-premature-abstraction convention), and
      `split<'a>(rom: &'a [u8], modules: &[ModuleSpec<'a>]) ->
      Result<Vec<Module<'a>>, SplitError>`. `checked_add` guards the
      offset+length bounds check against `usize` overflow; fail-fast
      (first out-of-bounds spec aborts the whole call, no partial
      `Vec`); zero-length modules and overlapping ranges both succeed
      (overlap detection is explicitly out of scope). Added to the fuzz
      target with offsets/lengths derived from the input bytes so the
      bounds-check path (including the overflow guard) is actually
      exercised. 58 unit tests (was 51); verified against the real
      1.63.0 toolchain, not just stable, after the MSRV break earlier
      this session.
- [ ] **`list`/`query`/`build` deferred** — not part of this milestone;
      revisit once `split` is proven and the hunk-parsing/relocation
      scope decision (implement inline vs depend on a crate) is
      reached. `hunkfile` (crates.io, 0BSD) was surveyed
      2026-09-15 and found **not viable as a dependency**: std-only
      (no no_std path), and it only exposes raw relocation *records*
      — not application logic — so the actual value this crate would
      need isn't there anyway. No other Amiga hunk-format crate exists
      on crates.io. Leaning toward a minimal inline implementation
      when `build` is reached, not a dependency.

## Milestone 5 — patch / combine / copy

Facts below confirmed 2026-09-15 against amitools' docs
(https://amitools.readthedocs.io/en/latest/tools/romtool.html, quoted
directly) and, where the docs were silent, empirically against
`romtool` as a black-box oracle (synthetic 512 KiB fixtures built with
this crate's own `seal_checksum`, never real ROM bytes) — the original
draft of this section had two facts wrong, caught before they reached
an implementation brief; see the corrections below.

- [x] **`copy --fix-checksum`** — **needs zero new library code.**
      Docs: "Copy a rom to a new file... `-c`/`--fix-checksum` after
      the copy fix the checksum of the written image." That's exactly
      `Loader::normalize` (if the input isn't already canonical) +
      `seal_checksum`, both already shipped since milestone 2. The
      *byte-order/Cloanto/hi-lo encode-direction* idea in this item's
      original draft was this plan's own scope inflation, not
      anything `romtool copy` actually does — dropped. Nothing to
      implement here; `copy` is purely the CLI crate wiring two
      existing primitives together.
- [x] **`combine`** — implemented exactly per the design below:
      `combine(first: &[u8], second: &[u8]) -> Result<Vec<u8>,
      CombineError>`, `CombineError::InvalidInput { which: InputSide,
      size }`. The doc comment states the swap fact prominently (front
      and center, with an explicit "don't do the naive fix" warning) —
      reviewed line-by-line by the parent session specifically for
      this, since getting it backwards would be the worst possible bug
      here. `combine_is_not_commutative` test proves it. — docs:
      "Concatenate a 512 KiB Kickstart and a
      512 KiB Ext ROM image to create a 1 MiB ROM suitable for soft
      kickers or maprom tools." **Corrects the original draft**, which
      wrongly assumed this was the milestone-1 kickety-split concept
      (256 KiB halves, self-referential midpoint header) — unrelated;
      `combine` is a completely different, larger-scale operation with
      no connection to kickety-split.

      **Confirmed empirically, and genuinely surprising — verify
      against this before implementing, don't assume CLI arg order
      matches output order:** `romtool combine kick.rom ext.rom -o
      out.rom` writes **`ext.rom`'s bytes first, `kick.rom`'s bytes
      second** — the *second* positional argument comes first in the
      output, reversed from what the argument names suggest. Proven
      by swapping the call (`combine(ext, kick)` byte-for-byte equals
      `kick_bytes ++ ext_bytes`, the "naively expected" order) — not a
      one-off fluke. Also confirmed: both inputs must independently
      pass full `is_kick` validity (an invalid/all-zero 512 KiB buffer
      is rejected, "Not a Kick ROM image!"); the two halves' bytes are
      **not otherwise transformed** (no reseal, no header/footer
      touch); the combined 1 MiB output is **not** itself resealed or
      expected to pass `KickRom`'s own checks as one coherent image
      (`check_size` correctly rejects it — it's a raw two-bank blob
      for hardware tools, not a validated single ROM).

      **API decision so this crate doesn't propagate romtool's
      confusing argument-order footgun**: name the library function's
      parameters by *position in the output*, not by the
      kick/ext labels that turned out to be misleading — e.g.
      `combine(first: &[u8], second: &[u8]) -> Result<Vec<u8>,
      CombineError>` returning literally `first ++ second` after
      validating each is 512 KiB and `is_kick`. The CLI crate is
      responsible for mapping `romtool combine kick.rom ext.rom`'s
      actual (reversed) byte order onto this unambiguous pair, with a
      loud comment there explaining why — the confusion stays
      contained to one documented call site instead of leaking into
      this crate's naming.
- [x] **Patch framework** — implemented: `PatchOp<'a>{offset, expected,
      replacement}`, `apply_patches(rom: &mut [u8], patches: &[PatchOp])
      -> Result<(), PatchError>` (`OffsetOutOfBounds`/
      `ExpectedMismatch`/`LengthMismatch`, each identifying the failing
      patch's index). Two-pass verify-then-apply, proven by a dedicated
      test (patch 3 of 5 failing leaves `rom` byte-for-byte untouched,
      not partially patched). No `1mb_rom` data shipped, per the
      decision below. — docs confirm only **one** named built-in
      patch exists: `1mb_rom`, "Patch Kickstart to support ext ROM
      with 512 KiB" (pairs with `combine` above — it's what makes a
      Kickstart recognize the resulting 1 MiB layout). **Corrects the
      original draft**, which misremembered this as "1.x scsi.device
      disable" — wrong, drop that reference. The crate still ships
      only the *mechanism* (find/verify/replace with expected-bytes
      safety, reseal via `seal_checksum` afterward) — not `1mb_rom`
      itself: deriving its exact patch bytes/offsets independently
      (without reading amitools' GPL source) is real reverse-engineering
      work in its own right, not a quick re-derivation as the original
      draft assumed. Treat shipping `1mb_rom`'s actual data as a
      separate, later decision — same shape as the milestone-4
      catalog-data question, not bundled into this pass.

## Milestone 6 — independent module-boundary detection (last, deliberately)

**Decided 2026-09-15: pursue this, but after everything else** — it's
a real research problem, not an implementation task, and shouldn't
block anything simpler. The goal: derive Remus/Romsplit-equivalent
module boundary data (and, later, relocation info) *without* Doobrey's
or Troller's restricted files as an input — an independent replacement
for the milestone-4 catalog data, built the same way this whole crate
has been: facts confirmed from primary sources, GPL/restricted tools
used only as black-box behavioral oracles, never read or copied from.

**Why this is hard, confirmed from amitools' own documentation, not
assumed:** `romtool`'s docs state outright — "splitting a ROM is a
difficult process as the borders of the modules are not clearly
marked in the ROM and furthermore the code positions that require
relocation are not marked at all... splitting is done with the help
of a split data catalog." This is a different class of problem than
milestones 1–3, which all worked because the format announces itself
somewhere (fixed header offsets; a matchword the CPU traps on if
executed). A production-linked Kickstart ROM strips exactly the
information — hunk separators, symbol boundaries — that would make a
module's start/end self-evident. This is closer to "decompile which
bytes came from which source file": likely fuzzy heuristics (code
fingerprinting, cross-referencing published SDK object files, entropy
analysis), not a deterministic decoder, and there's no guarantee it's
fully solvable without debug info that was never shipped.

**What's already available for free, no restricted data needed:**
milestone 3's `ResidentScan` already yields genuine module *start*
offsets — every `Resident` hit's own offset is a real boundary, since
the matchword self-announces. That only covers library/device/resource
init points (not arbitrary internal modules) and gives no *end*
boundary (`rt_EndSkip` is deliberately never trusted, per milestone 3).
Whatever this milestone builds should start from that free structural
anchor before reaching for heuristics.

**The legal discipline, non-negotiable if this is attempted:**
- Design and implement using only the ROM bytes and publicly
  documented Amiga linking/compilation conventions. **Never open
  Doobrey's or Troller's files while designing or writing this code**
  — no "read their format to learn the trick," no consulting their
  data mid-implementation. That crosses from independent derivation
  into the access-plus-reproduce pattern PLAN.md's licensing section
  already flagged as risky (their EULA is "no redistribution... no
  license grant," not GPL — there's no broad use-and-study right to
  lean on the way there is with amitools).
- If their published data is used *afterward* purely as a correctness
  check, treat it exactly like the `AMIGA_ROM_DIR` real-ROM harness:
  **private and local-only, never committed to the repo, never
  published as a diff or comparison report, and never allowed to
  "correct" the independent algorithm** — a mismatch is a bug to debug
  against the ROM itself, not license to copy their answer. This
  mirrors the amitools-as-oracle discipline already proven throughout
  this project, tightened because the license terms here are stricter.
- Get real legal advice before publishing anything derived this way —
  this crate's own research has consistently erred toward caution on
  Doobrey/Troller material (see the licensing section up top), and
  that caution should carry through here.

**Progress, 2026-10-05 — research + partial implementation, not
complete.** Full design brief:
`docs/research/module-boundary-detection.md`. Per that document's own
header, nothing in it or in the code below came from opening or
searching for Doobrey's or Troller's restricted files.

- [x] **Module start offsets confirmed as already solved.** Re-read
      milestone 3's own doc comments specifically for this: `Resident
      ::offset` (already on the type) *is* the start offset, free,
      no new API needed. Honestly scoped what it does *not* cover
      (code with no `Resident` structure at all) — see the design
      doc §2.
- [x] **`module_boundaries`** — a new, safe (non-heuristic) primitive:
      given a sorted list of `Resident` hits and the image length,
      returns one `ModuleBoundary{start, end_upper_bound}` per resident,
      where `end_upper_bound` is the next resident's start (or
      `rom_len` for the last one). This is a **true upper bound, not a
      claimed real end** — documented as such everywhere it appears,
      because the module's actual last byte can be, and usually is,
      earlier (alignment padding, shared glue code). 4 unit tests
      (`boundary_tests`), including an out-of-order-input case proving
      it never panics even outside its documented precondition. Added
      to the fuzz target.
- [x] **Tighter end-offset detection — partially addressed.** The three
      original candidate heuristics (padding/alignment stripping,
      code/data entropy classification, disassemble-until-RTS) were
      considered and rejected — see design doc §3 for the reasoning
      per heuristic; none met this crate's own bar (oracle-verified or
      NDK-cited) for being shipped as more than a guess. A fourth,
      different technique shipped instead (design doc §3.1):
      `ModuleBoundary` gained `end_skip_hint: Option<usize>`, and
      `module_boundaries` a new `base_addr: Option<u32>` parameter to
      compute it. `rt_EndSkip` is still never *trusted* as a fact or
      used to drive control flow (milestone 3's rule stands) — instead
      it's translated to a file offset (same `wrapping_sub` convention
      as `ResidentScan`'s pointer fields) and reported as a hint *only*
      when it lands strictly inside `(start, end_upper_bound]`, i.e.
      only when it's consistent with the already-proven-safe bound,
      never when it would contradict it. A corrupt/hostile `rt_EndSkip`
      can at worst produce a plausible-but-wrong in-range hint — it can
      never make `end_skip_hint` exceed the hard ceiling the way
      trusting it for control flow could. Documented explicitly as
      lower-confidence than `end_upper_bound` (consistent-with is not
      the same as confirmed-correct). **Still doesn't solve the general
      case**: real ROMs whose `rt_EndSkip` conventionally equals the
      next module's start (the common, RKRM-documented case) get
      `end_skip_hint: None`, same as before — this only surfaces
      information for modules whose author set a tighter `rt_EndSkip`.
      **Real-ROM validated in a follow-up session** against ~70 real
      Kickstart dumps the developer legally owns (`~/Documents/
      Amiberry/Roms` + `.../Kickstarts`, versions 1.0-3.2.x across
      A500-A4000T/CD32, plus AROS) via a throwaway local program (not
      committed — no test here depends on these files) reporting
      aggregate counts only: **51.7%/56.0%** of residents (two
      directories, ~2600 residents total) got a `Some` hint — a real,
      frequently-firing signal, not a theoretical curiosity. One
      pattern recorded for future reference: Hyperion's 3.2.x-era
      builds showed a far lower rate (~1/42-43 per ROM) than every
      Commodore-era build sampled (~18-30/20-45) — a build-convention
      difference, not a bug. `find_relocations` against a real same-
      build/different-base pair stays unvalidated: this collection is
      entirely physical dumps at each machine's standard base, no
      softloaded/relocated copy of the same build to diff against —
      needs a different kind of sample. Full writeup:
      `docs/research/module-boundary-detection.md` §3.2. 8 new unit tests
      (`boundary_tests`, up from 4): hint present when strictly inside
      the bound, absent with no `base_addr`, absent when equal to
      `end_upper_bound` (the conventional case), absent for three
      distinct out-of-range/hostile values (before start, past the
      bound, wildly out-of-range), and a wrapping-translation case that
      proves no panic. Fuzz target updated to pass `rom.base_addr()`
      through, exercising `end_skip_hint`'s translation on adversarial
      input automatically.
- [x] **`find_relocations`** — generic two-buffer RELOC-diff primitive
      implementing the technique already recorded above (same code at
      two load addresses, diff for words that shifted by exactly the
      base delta). Deliberately **not** ROM-specific — takes two
      arbitrary equal-length `&[u8]` and a `u32` delta, returns
      `Vec<RelocCandidate>`; `RelocDiffError::LengthMismatch` on
      mismatched lengths. Scans every 2-byte-aligned offset, matching
      `ResidentScan`'s own word-alignment convention. 7 unit tests
      (`reloc_tests`) with synthetic planted-relocation fixtures
      (never real ROM bytes) covering: a single planted reloc, multiple
      relocs, zero matches on identical buffers with nonzero delta, the
      documented `delta == 0` degenerate case, odd-offset exclusion,
      length mismatch, and empty buffers. Added to the fuzz target.
      Failure modes (coincidental false positives, BCPL-scaled false
      negatives, table-aliasing) documented in design doc §4.3 rather
      than silently assumed away.
- [ ] **Using `find_relocations` on real modules — not attempted.**
      Needs two real same-build Kickstart ROMs loaded at different
      bases and a way to line up "the same module" in both; neither
      exists in this crate's test suite by design (no real ROM bytes
      ship here). If ever done, follows the `AMIGA_ROM_DIR` discipline:
      local-only, never committed, never used to "correct" the
      algorithm.
- [ ] **CRC32/known-module hash-file identification** (Capitoline's
      other named technique) — not designed or implemented this
      session. See design doc §5.
- [ ] **BCPL-scaled (`delta / 4`) second pass for early `dos.library`
      relocs** — not implemented; no confirmed, general rule yet for
      exactly which regions need it (see design doc §4.3).
- [ ] **Catalog-assembly layer** (turning `module_boundaries` +
      `find_relocations` into anything resembling a `romtool
      split`-equivalent generator) — not started, and gated on the
      still-open end-offset problem above. The two primitives shipped
      this session are deliberately just that: primitives, same
      "mechanism, not data" discipline as `apply_patches`/`combine`.

This milestone remains open. What shipped this session is real,
tested, and honestly scoped — not a claim that module-boundary
detection is solved.

## Cross-cutting

- [x] **Panic policy**: after milestone 2, no public entry point may
      panic on any input (`todo!()` stubs are the documented,
      temporary exception and each names its milestone). Enforced by
      the fuzz targets plus explicit hostile-input unit tests per
      check.
- [x] **CI** (GitHub Actions), cloned from the rdb crate's shape:
      stable test, `--no-default-features` build+test, clippy
      `-D warnings`, `fmt --check`, `RUSTDOCFLAGS="-D warnings"
      doc`, MSRV 1.63 job, amitools differential job (pins the
      version), 60-second fuzz smoke once the fuzz target exists.
- [ ] **`#![deny(missing_docs)]`** once milestone 2's API-shape
      audit settles signatures (before publish).
- [x] **Real-ROM harness**: the `AMIGA_ROM_DIR` env-gated test
      module (skips silently when unset), asserting known facts
      about known ROMs by KickSum. Local-only, never CI.
- [x] **crates.io**: published `amiga-rom` 0.2.0 (2026-09-15, tag
      `v0.2.0`); 0.x thereafter until the milestone-4 interfaces prove
      out.

## Backlog — additional ideas (research session 2026-10-05)

Surfaced while reading maidavale.org's ROM-hacking roundup and then
Capitoline's own site in detail (http://capitoline.twocatsblack.com/),
specifically the `1mb-roms`, `superkick`, `skick-rtb`, `patcher`,
`hash-files`, `analysing-unknown-roms`, `structure`, `digital` and
`physical` pages — same site already cited above for the `.RTB`/`.PAT`
format description and the milestone-6 identification methodology.
Recorded here, not acted on yet except where noted.

- **Milestone 6 is already in progress** — see the dedicated section
  above; this session dispatched a research worker against it
  (2026-10-05), under the same legal discipline already written there
  (ROM bytes + public conventions only, never Doobrey's/Troller's
  restricted files, real-ROM comparisons private/local-only).
- **skick/RTB *encoder*.** The plan above only treats the `.RTB` delta
  format as something to *understand* (for milestone-6 context and as
  independent corroboration of the `PatchOp` shape). Actually
  generating `.RTB` files (softload Kickstart support, also usable by
  WHDLoad) is new scope, and depends on milestone 6's RELOC data
  existing first — a pure byte-level encoder, so it belongs in this
  crate, not the CLI. Not started.
- **SuperKickstart / A1000 `KICK` boot-floppy container formats.**
  Two more non-ADF, non-filesystem floppy containers directly relevant
  to a Kickstart-ROM crate: the A1000's 8k-bootstrap `KICK`-prefixed
  floppy (raw 256k ROM starting at byte 512) and the A3000-targeted
  `KICKSUP0`-prefixed SuperKickstart floppy (fixed-offset Kickstart +
  "Bonus" code pair, two revisions). Candidates for new `RomEncoding`
  variants/detection, same shape as the already-implemented
  Cloanto/ReKick/KickIt containers. Not started; no decision yet on
  whether floppy-sector-level containers belong in this crate (bytes
  in, bytes out — arguably in scope) or are better left to a
  disk-image-aware consumer.
- **SCANTABLE-derived 1MB/2MB patch data.** Milestone 5 shipped the
  `apply_patches` mechanism but deliberately deferred shipping the
  `1mb_rom` named patch's actual bytes/offsets as "real
  reverse-engineering work in its own right." Capitoline's `1mb-roms`
  page documents the SCANTABLE patching technique (locate the
  SCANTABLE via a byte search anchored on exec.library's own
  `RT_ENDSKIP` field, append a replacement table sized for 1MB/2MB,
  repoint the `LEA`-relative reference) for KS1.3, KS2.x, KS3.1, and
  Hyperion 3.1.4+ ROMs — independently derivable the same way the
  byte-order/header facts above were (ROM's own structure + public
  68k/Amiga conventions, no restricted catalog needed). A candidate
  source for finally deriving `1mb_rom` (and a `2mb_rom` sibling)
  properly. Not started.
- **PAT file format — no action needed.** Capitoline's `.PAT` format
  (checksum header + zero-terminated `(offset, 4 replacement bytes)`
  records) was already confirmed, in this same plan, to be exactly
  this crate's existing `PatchOp`/`apply_patches` shape. Noted here
  only to close the loop — nothing new to do.

## Non-goals, so they don't creep in

- **No file I/O, ever** — the CLI owns files; this crate owns bytes.
- **No bundled ROM images, `rom.key`s, or Remus/SKick data** — the
  licensing sections above are the record of why.
- **No m68k emulation/disassembly** — `scan` reads structures, it
  does not execute or trace. A disassembling consumer brings its own
  disassembler.
- **No ROM downloading/acquisition help** in docs or tools.
- **Not a hunk linker** — milestone 4 needs hunk relocation *for ROM
  building only*; general hunk tooling belongs in its own crate.
