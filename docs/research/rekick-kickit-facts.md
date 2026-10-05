# Milestone-2 facts: ReKick/ReCode "DEADFEED" encoding, and the KickIt container

Research backing the `RomEncoding::ReKickEncoded` / `RomEncoding::KickItWrapped`
additions to `Loader`. Unlike every other fact in this crate's research
docs, this one is **not** oracle-verified against real ROM bytes or
cross-checked against a second independent source — it rests on a single
secondary source, a blog post. Treat it as **PROBABLE**, not CONFIRMED,
until someone verifies it against a real ReKick/ReCode or KickIt file (no
such file is available in this project to test against).

## Source

<http://capitoline.twocatsblack.com/index.php/digital/> and
<http://capitoline.twocatsblack.com/index.php/hash-files/> — "Capitoline",
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

## What's deliberately not implemented

- **No encode direction** for either format — same reasoning as
  Cloanto: this crate's `Loader` only reads real dumps a caller already
  has, it doesn't produce ReKick/KickIt-formatted output for burning or
  distribution.
- **Container variants with a different header length, or a size other
  than 256 KiB/512 KiB, are not recognized** — the source only
  documents the one 108-byte/DEADFEED shape and one KickIt example; if
  a real-world sample turns up with a different shape, extend
  `REKICK_HEADER_LEN`/the size check then, per this crate's "don't
  invent facts past what's confirmed" discipline.
