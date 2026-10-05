# Independent module-boundary detection — Milestone 6 design notes

Status: **research + partial implementation**, started 2026-10-05. This
document is the design brief PLAN.md's Milestone 6 calls for — what's
confirmed, what's implemented, what's honestly still open. Per PLAN.md's
non-negotiable discipline for this milestone: everything below is derived
from (a) bytes this crate already parses, (b) publicly documented Amiga
linking/ROM conventions (NDK structs, the Hardware Reference Manual, hunk
format docs), and (c) Capitoline's `analysing-unknown-roms` page (already
cited in PLAN.md as an independent methodology source, read only for its
two named techniques — matchword/CRC32 cataloging and the two-ROM-diff
RELOC technique — never for any catalog *contents*). **Doobrey's
Remus/Romsplit data and Troller's SKick `.RTB`/`.PAT` files were not
opened, searched for, or referenced while writing this document or the
code it describes.**

## 1. What "module boundary" needs to mean here

A Kickstart ROM is a single linked blob: whatever hunk/symbol boundaries
existed at link time are gone by the time it's burned into ROM. The two
things `romtool split`'s catalog actually records, per PLAN.md's already-
confirmed `.RTB`/`.PAT` format research, are:

1. **Module extents** — contiguous byte ranges, each corresponding to one
   originally-separate link unit (a library, device, or resource, plus
   whatever shared/glue code sits between them).
2. **Relocations** — byte offsets inside the ROM holding absolute
   addresses that were resolved once at ROM-build time and would need
   re-resolving if that code ran from a different base (the whole point
   of Remus/Romsplit/SKick: re-targeting modules to run from RAM instead
   of ROM).

This document (and the code landing with it) addresses both, with very
different confidence levels: *starts* are a solved, structural fact;
*ends* are fundamentally a best-effort bound, not a fact; RELOCs have a
concrete, implementable detection technique with known failure modes.

## 2. Module start offsets — already free, confirmed, nothing new needed

Milestone 3's `ResidentScan` already yields every module whose author
used the standard AmigaOS auto-init convention (`RTC_MATCHWORD` +
self-referencing `rt_MatchTag`) — that's every library, device, and
resource `exec.library`'s own `InitResident` scan would find at boot,
which in practice is the great majority of what Remus/Romsplit's catalog
calls a "module." `Resident::offset` (already on the type, see `src/
lib.rs`) is the start offset; no new API needed, confirmed by re-reading
milestone 3's own doc comments while writing this document.

**What this does not cover**, honestly:

- Code with no `Resident` structure at all — e.g. static helper routines
  called only via internal jumps, not registered with Exec. These leave
  no structural self-announcement anywhere in the ROM; finding their
  boundaries needs either code-flow analysis (disassembly, explicitly a
  non-goal per PLAN.md) or cross-referencing published SDK object code
  (fingerprinting, not attempted here).
- The very first bytes of the ROM (the header/boot code before the first
  `Resident`) and any inter-module padding/glue, both of which belong to
  no resident at all.

So "module start detection" is **done**, not attempted here as new work —
this section exists to record that `ResidentScan` already is the answer,
and to scope what it does *not* claim to find.

## 3. Module end offsets — a bound, not a fact

`rt_EndSkip` is deliberately untrusted by `ResidentScan` (milestone 3) —
it's data the module author wrote, not something the scanner can verify
against anything self-referential the way `rt_MatchTag` works. It can't
be used here either, for the same reason: trusting it would mean trusting
unverified ROM content, exactly the class of thing this crate has
avoided since milestone 1.

**What's actually knowable, cheaply and honestly:**

Given the sorted list of `Resident` start offsets `s_0 < s_1 < ... < s_n`
found by a scan, module `i`'s real extent **must** satisfy
`s_i <= end_i <= s_{i+1}` (or `<= rom_len` for the last module) — it
cannot run into the next resident's own `rt_MatchWord`, because that
word has to be intact in ROM for `exec_rev`'s own boot-time scan to find
it. This gives a **true upper bound** on every module's end, for free,
with zero heuristics and zero risk of being wrong (it can only be loose,
never incorrect) — implemented below as `module_boundaries`.

This upper bound is *not* the real end in general: real Kickstart ROMs
routinely have non-resident filler between one module's actual last
instruction and the next module's `RTC_MATCHWORD` (alignment padding,
shared jump tables, code belonging to no resident at all). Romsplit-style
catalogs resolve this tighter boundary using data this crate does not
have and will not acquire.

**Heuristics considered and explicitly rejected for this pass**, so a
future session doesn't re-litigate them without the reasoning:

- *"End = next module's start minus alignment padding."* Requires
  knowing the real padding convention (word? longword? zero-filled only,
  or filled with a repeating pattern?) with enough confidence to trust
  automatically — not established from public sources at the rigor this
  crate requires elsewhere (every other fact here is oracle-verified or
  NDK-cited). Trailing zero/pad detection is easy to *implement* but easy
  to get wrong silently (a module that legitimately ends in zero bytes,
  e.g. a BSS-like static buffer baked into ROM, is indistinguishable from
  padding without more information) — exactly the kind of unverifiable
  heuristic PLAN.md warns this milestone will produce, and one this
  session declines to ship as if it were a fact.
- *"Code vs. data entropy/heuristic classification."* Standard technique
  in general binary analysis, but has no Amiga-specific self-check the
  way `rt_MatchTag` does — it would be guessing dressed up as detection.
  Worth real research effort in a future session (with synthetic 68k
  code corpora as ground truth, not real ROMs), not attempted here.
- *"Disassemble forward from `rt_Init` until an illegal/return
  instruction."* Explicitly out of scope — PLAN.md's non-goals rule out
  m68k disassembly/execution in this crate entirely.

**Conclusion for this pass**: ship the honest upper bound
(`module_boundaries`), document it as a bound rather than an answer, and
leave tighter-end heuristics as explicitly open future work.

## 4. RELOC detection via the two-ROM-diff technique

### 4.1 The technique, as a general primitive

Per the methodology already recorded in PLAN.md's licensing section
(from Capitoline's `analysing-unknown-roms` page, read only for the
technique, not any catalog data): if the *same* code is present in two
ROM images, loaded at two different absolute base addresses `A` and `B`,
then for every absolute-address constant the code contains, its stored
value in the `A`-based image plus `(B - A)` equals its stored value in
the `B`-based image. Any 4-byte-aligned-by-instruction-word offset where
this holds is a relocation candidate.

This is not actually ROM-specific, nor even Amiga-specific — it's a
generic "find words that shifted by a known delta between two buffers"
diff, so the implementation below (`find_relocations`) takes two
arbitrary same-length byte slices and a `u32` delta; nothing about it
reads `KickRom` or `Resident` state. This matches PLAN.md's framing of it
as a generic primitive and keeps it allocation-cheap and testable with
synthetic fixtures that have nothing to do with real ROM bytes.

### 4.2 API shape

```rust
pub struct RelocCandidate {
    pub offset: usize,
}

pub enum RelocDiffError {
    LengthMismatch { a_len: usize, b_len: usize },
}

pub fn find_relocations(
    a: &[u8],
    b: &[u8],
    delta: u32,
) -> Result<Vec<RelocCandidate>, RelocDiffError>;
```

- **Inputs**: `a`/`b` are the *same* code (e.g. the same library,
  excised from two ROM dumps that load it at different bases — the
  caller is responsible for lining them up and supplying equal-length
  slices; this crate does not attempt to find "the same module in two
  ROMs" by itself, that's a separate correlation problem, likely via
  `machine_hints`/`identify` plus manual module selection). `delta` is
  `B - A` (wrapping, since Amiga addresses are 32-bit and base
  arithmetic is expected to wrap the same way `ResidentScan`'s
  `base_addr` math already does).
- **Scanning granularity**: every 2-byte-aligned offset, matching
  `ResidentScan`'s own convention — m68k instructions and their
  longword operands are only ever word-aligned, never odd-aligned, so
  odd offsets are skipped exactly as the resident scanner already
  skips them.
- **Output**: one `RelocCandidate` per offset where
  `u32::from_be_bytes(a[offset..offset+4]).wrapping_add(delta) ==
  u32::from_be_bytes(b[offset..offset+4])`. Deliberately the *raw*
  candidate list — no clustering, no "merge adjacent candidates into one
  structure" step, same "ship the mechanism, not a catalog" discipline
  as `apply_patches`.
- **Errors**: `RelocDiffError::LengthMismatch` if `a.len() != b.len()` —
  the technique is meaningless for differently-sized inputs (this isn't
  a general-purpose diff tool, it only makes sense comparing the same
  code).

### 4.3 Failure modes, stated honestly

**False positives** (reported as a candidate, but not actually a
relocation):

- *Coincidental matches.* A pair of unrelated words that happen to
  differ by exactly `delta` by chance. This grows likelier the smaller
  `delta` is relative to the data's own entropy — a `delta` that's a
  small, "round" value (e.g. `0x10000`, a bare high-word bump) is far
  more collision-prone across incidental data words than a large,
  arbitrary one. No mitigation beyond caller judgement is proposed here;
  flagging this clearly in the function's own doc comment is the
  mitigation.
- *`delta == 0`.* Degenerate case: every identical word in `a`/`b`
  "matches" (`x + 0 == x`), which floods the result with the entire
  unchanged portion of the buffers. Not rejected as an error (it's not
  nonsensical input, just a useless comparison), but called out
  explicitly in the doc comment so a caller doesn't mistake "every word
  matched" for "every word is a relocation."
- *Adjacent/overlapping aliasing.* If a run of bytes happens to look
  like a relocatable address at more than one word-aligned offset
  (e.g. inside a jump table of several consecutive absolute addresses),
  each table entry legitimately produces its own candidate — this is
  correct behavior, not a false positive, but a caller expecting "one
  candidate per logical relocation site" needs to know a dense run of
  candidates can mean "one table of N entries," not "N unrelated bugs."

**False negatives** (an actual relocation that goes undetected):

- *Non-absolute relocations.* PC-relative addressing needs no
  relocation at all (by construction — this is a correct *absence*, not
  a miss).
- *Scaled/BCPL-style relocations.* PLAN.md's own `.RTB` format research
  already recorded that early `dos.library` BCPL code stores some
  relocations divided by 4 (BCPL's own addressing convention) — these
  won't be caught by a direct `delta` comparison; a caller aware of this
  would need a second pass with `delta / 4`. Not implemented as a crate
  feature yet (no confirmed, general rule for *which* code needs this
  pass — the `.RTB` second-section flag mechanism that signals it is
  exactly the catalog-format detail PLAN.md already treats as
  format-shape-only, not something to reproduce without a confirmed,
  independent specification of *when* it applies beyond "early
  dos.library").
- *Relocations whose value happens to be identical at both bases.* Rare
  but possible if the low bits of the address coincidentally repeat —
  not expected to matter in practice given real base deltas are
  typically whole 64 KiB+ jumps, but worth naming as a theoretical gap.

### 4.4 What this does and does not solve for Milestone 6 overall

`find_relocations` gives a real, implementable, testable building block
for the RELOC half of Milestone 6's goal — but using it for real still
requires two real ROM images of the *same* Kickstart build loaded at two
different bases (e.g. two different machines' same-version ROMs mapped
at different `base_addr` values, or a soft-kick tool's relocated copy
versus the ROM original) and a human (or future heuristic) to line up
"the same module" in both before calling it. Neither of those inputs
exists in this crate's test suite (per PLAN.md, real ROMs never ship
here) — tests below use wholly synthetic byte buffers with planted,
known deltas, proving the *algorithm*, not that it works on any specific
real Kickstart build. That real-world validation, if ever done, follows
the `AMIGA_ROM_DIR` discipline: local-only, never committed, never used
to "correct" the algorithm by copying an answer from restricted data.

## 5. What remains genuinely open

- Tighter module end-offset detection (padding/alignment heuristics or
  code/data classification) — explicitly deferred, see §3.
- Any heuristic for finding non-resident-anchored modules at all (code
  with no `Resident` structure) — not attempted; likely needs either
  cross-referencing public SDK object files by signature, or accepting
  this is simply unsolvable to this crate's evidentiary standard.
- A CRC32/known-module hash-file scheme (Capitoline's other named
  technique, alongside the two-ROM-diff one) — not designed or
  implemented this session; would need its own seed data (built the
  same honest way as milestone 3's `KnownRom` table: independently
  computed from ROMs this project's own author legally owns, never
  copied from a restricted hash database) and is a separate, sizeable
  piece of work.
- The BCPL-scaled relocation second pass (§4.3) — no confirmed, general
  rule yet for exactly which regions need it.
- Turning `module_boundaries` + `find_relocations` into anything
  resembling a `romtool split`-equivalent *catalog builder* — both are
  shipped here as primitives only, per the same "mechanism, not data"
  discipline as `apply_patches`/`combine`. Assembling them into a
  catalog-generation pipeline is future work, gated on having the
  end-offset problem actually solved (§3), which it currently is not.
