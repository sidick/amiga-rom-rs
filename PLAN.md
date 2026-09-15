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
- [ ] **Publish 0.2.0** once `info` parity holds: the crate is
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
- [ ] **Implement per the above** once picked back up.
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

- [ ] **`copy --fix-checksum`** needs only milestone 2's
      `seal_checksum` — expose the ergonomic "fix this image in
      place" wrapper and the byte-order/Cloanto/hi-lo *encode*
      directions (`Loader`'s inverse: canonical → burner-ready), per
      the milestone-1 conventions item.
- [ ] **`combine`**: join a 256 KiB kick + 256 KiB ext ROM into
      512 KiB (the kickety-split layout from milestone 1, now
      written rather than just detected), re-sealed.
- [ ] **Patch framework**: named patches applied to known ROMs
      (`romtool patch`'s model: identify ROM by checksum, apply
      byte-level edits, re-seal). The crate ships the *mechanism*
      (find/verify/replace with expected-bytes safety, like a binary
      patch format); which patches ship as data is a licensing
      question deferred to when reached — amitools' one built-in
      (1.x scsi.device disable) may be small enough to re-derive
      independently.

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
- [ ] **crates.io**: reserve/publish `amiga-rom` 0.2.0 at end of
      milestone 2 (see item there); 0.x thereafter until the
      milestone-4 interfaces prove out.

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
