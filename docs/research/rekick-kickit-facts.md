# Milestone-2 facts: ReKick/ReCode "DEADFEED" encoding, KickIt, and the KICK floppy

Research backing the `RomEncoding::ReKickEncoded` /
`RomEncoding::KickItWrapped` / `RomEncoding::KickFloppyWrapped`
additions to `Loader`. Unlike every other fact in this crate's research
docs, this one is **not** oracle-verified against real ROM bytes or
cross-checked against a second independent source — it rests on a single
secondary source, a blog post. Treat it as **PROBABLE**, not CONFIRMED,
until someone verifies it against a real ReKick/ReCode, KickIt, or KICK
floppy file (no such file is available in this project to test
against — the large real-Kickstart sample used elsewhere this session,
see `module-boundary-detection.md` §3.2, is all plain physical-ROM
dumps, none of these three container formats).

## Source

<http://capitoline.twocatsblack.com/index.php/digital/>,
<http://capitoline.twocatsblack.com/index.php/hash-files/>, and
<http://capitoline.twocatsblack.com/index.php/physical/> — "Capitoline",
a hobbyist Kickstart ROM editing/patching tool's own documentation site.
Plain HTTP only (no TLS listener on that host as of 2026-10-05); fetched
with `curl`, not `WebFetch` (which force-upgrades to `https://` and fails
with `ECONNREFUSED`). Read as a black-box description of an on-disk file
format, not as source code — nothing was ported from Capitoline's own
(unknown-license) implementation, only the format facts it documents.

## ReKick/ReCode ("DEADFEED") encoding

A lightly-obfuscated (the source's own words: "never intended to be
anything more than casual protection") container format for Kickstart
ROMs designed to be softloaded, distinct from both Cloanto's
`AMIROMTYPE1` format and plain byte-swapped raw dumps:

1. A **108-byte plaintext header** precedes the encrypted payload. The
   source's hex dump of this header is the standard Kickstart copyright
   banner text ("AMIGA ROM Operating System and Libraries: Copyright ©
   1985-1992 Commodore-Amiga, Inc. All Rights Reserved.", NUL-padded) —
   the same banner that (per `cloanto-hilo-facts.md`) also appears
   *inside* a decoded Kickstart payload at its own fixed offset; here it
   is reproduced as the container's own leading plaintext, not the
   payload's. Corroborated by the `hash-files.txt` page's own worked
   example, which places a named ROM's Kickstart component starting at
   container offset **108** exactly (`Kickstart=108,524288,...`).
2. The payload (conventionally 256 KiB or 512 KiB — a whole Kickstart
   image) is encrypted as a **self-keyed chained XOR over 4-byte
   blocks**: the first block is XORed against the fixed constant
   `0xDEADFEED`; each subsequent block is XORed against the *decoded
   plaintext of the previous block* (not against a derived keystream —
   the feedback is the plaintext itself). Decoding is therefore a
   strictly sequential fold: decode block *i*, then use that result as
   the key for block *i+1*.
3. The source notes a practical weakness (not a format variant): since
   most 512 KiB ROMs' first longword is the well-known `0x11144EF9`
   boot-vector signature, the first block's key is often guessable
   without even knowing the `0xDEADFEED` constant — informational only,
   doesn't change the decode algorithm above.

**Detection** (`REKICK_MAGIC`/`REKICK_HEADER_LEN` in `src/lib.rs`):
rather than match the full 108-byte header verbatim (the copyright
year range varies by ROM revision, e.g. "1985-1992" vs. later years),
the crate matches a shorter, stable prefix of that banner and requires
the remaining length (`data.len() - 108`) to be exactly
[`ROM_SIZE_256K`]/[`ROM_SIZE_512K`] — deliberately conservative, so an
unrelated file that happens to start with similar text but isn't
shaped like a whole-ROM container doesn't get misclassified.

## KickIt container

A much simpler wrapper, used for ROMs designed to be softloaded to a
non-standard RAM address (`0x200000`, `0xC00000`, `0xF00000`, etc.) on
machines without an MMU, so the image is pre-relocated rather than
reordered at load time:

- **8-byte header**: four zero bytes, then a big-endian `u32` giving
  the wrapped image's size in bytes (the source's example: `0x00080000`
  = 512 KiB). No encryption, no byte reordering implied — the payload
  immediately following is the canonical image.

**Detection** (`KICKIT_HEADER_LEN` in `src/lib.rs`): the four leading
zero bytes plus a declared size matching
[`ROM_SIZE_256K`]/[`ROM_SIZE_512K`] *and* equal to the actual remaining
data length — both conditions required, so a file that merely starts
with four zero bytes (not a real KickIt header) doesn't false-positive.

## KICK floppy container (A1000 bootstrap)

The A1000 has no physical Kickstart ROM at all — a small 8 KiB
"bootstrap" ROM instead loads the real (256 KiB) Kickstart from a
floppy into "write-once" RAM at boot. Per the `physical` page, that
floppy is **not DOS-formatted** — it's identified purely by its first
four bytes being the ASCII magic `"KICK"` (`0x4B49434B`), with the
actual 256 KiB Kickstart payload starting at a **fixed byte offset of
512** and read sequentially from there.

**Detection** (`KICK_FLOPPY_MAGIC`/`KICK_FLOPPY_HEADER_LEN` in
`src/lib.rs`): the 4-byte magic, plus a remaining length of exactly
[`ROM_SIZE_256K`] after the fixed 512-byte header — the source only
ever documents this for the A1000's one Kickstart size, so (unlike
KickIt's self-declared size) there's no 512 KiB variant to accept.

Unlike ReKick/KickIt, this one has no cross-check from a second page on
the same site (no `hash-files`-style worked example naming the exact
offset) — treat its confidence as, if anything, slightly weaker than
ReKick/KickIt's already-PROBABLE status, pending a real sample.

**Explicitly out of scope, considered and rejected this session**: the
A3000 **SuperKickstart floppy** (`"KICKSUP0"` magic), also described on
the `physical` page, bundles *two* Kickstarts (1.3 and 2.x) plus two
"Bonus" code blobs at fixed offsets — it doesn't produce a single
canonical image the way `Loader::normalize`'s `Result<Vec<u8>, _>`
shape expects, so it doesn't fit this function at all. Extracting from
it would need a dedicated multi-component type, which is a real design
decision (how many components, named how, is this `Loader`'s job or a
different one) deferred rather than rushed. Likewise the DOS-formatted
relocation floppies (Relokick/Tude, also on that page) are out of scope
entirely: those need a real AmigaDOS filesystem parser (`amiga-ffs-rs`'s
job), not anything this crate's "no file I/O, just bytes" model should
grow into.

## What's deliberately not implemented

- **No encode direction** for any of the three formats — same reasoning
  as Cloanto: this crate's `Loader` only reads real dumps a caller
  already has, it doesn't produce ReKick/KickIt/KICK-floppy-formatted
  output for burning or distribution.
- **Container variants with a different header length/offset, or a
  size other than what's documented, are not recognized** — the source
  only documents the one 108-byte/DEADFEED shape, one KickIt example,
  and one fixed KICK-floppy offset; if a real-world sample turns up
  with a different shape, extend the relevant constant/size check then,
  per this crate's "don't invent facts past what's confirmed"
  discipline.
- **SuperKickstart (`"KICKSUP0"`) and DOS-formatted relocation floppies
  (Relokick/Tude)** — see the previous section for why each is out of
  scope for this pass.
