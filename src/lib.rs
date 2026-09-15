//! Amiga Kickstart ROM images: normalize raw dumps, then inspect/validate
//! a canonical image.
//!
//! This crate is pure format logic — no file I/O. Everything operates on
//! borrowed `&[u8]` / `&mut [u8]`; a caller (a CLI, an emulator, a test)
//! owns reading the bytes in from wherever they live. A ROM image is one
//! flat blob rather than a block device, so unlike `amiga-ffs`/`amiga-rdb`
//! there's no `BlockSource`-style seam here — just slices.
//!
//! Two layers:
//!
//! - [`Loader`] turns whatever bytes a user actually has — byte-swapped,
//!   [Cloanto/Amiga Forever-encoded](RomEncoding::CloantoEncoded), or a
//!   split hi/lo EPROM pair ([`merge_hi_lo`]/[`split_hi_lo`]) — into a
//!   canonical raw image. These need `alloc`, since they own the output
//!   buffer.
//! - [`KickRom`] inspects/validates a canonical image: allocation-free,
//!   borrowing the caller's `&[u8]`.
//!
//! See `PLAN.md` in this repository for the full design rationale,
//! including why this crate is an independent implementation (not a port
//! of amitools' GPL-3 `romtool`) and why the Remus/Romsplit
//! module-boundary catalog used by `split`/`build` is deliberately not
//! bundled.
//!
//! # Status
//!
//! Milestone 1 (the facts pass) is complete: every byte-level fact —
//! header/footer offsets, the kickety-split signature, the magic-reset
//! opcode, the stored-checksum location and KickSum convention, the
//! byte-order signature table, Cloanto framing, hi/lo interleave — is
//! confirmed and recorded in `docs/research/` and `PLAN.md`, proven by
//! a synthetic fixture (`fixtures::synthetic_rom`, test-only) that
//! amitools' `romtool` accepts as `is_kick: ok`. Milestone 2's core is
//! now real on both sides: every [`KickRom`] check/value method
//! transcribes the confirmed facts (bounds-checked, panic-free on any
//! input; value methods return `Option`), [`seal_checksum`] is the
//! public checksum-writing counterpart, and the loader is complete —
//! byte-order detection/reordering over the confirmed signature table,
//! Cloanto decode, and the hi/lo EPROM word-interleave. No `todo!()`
//! remains; the differential-oracle harness and fuzzing are the
//! outstanding milestone-2 items. Milestone 3's `scan` step is also
//! implemented: [`KickRom::scan`] returns a [`ResidentScan`], an
//! allocation-free iterator over `Resident` (RomTag) structures found
//! in the image, matching `romtool scan` parity — see that type's doc
//! comment for the exact scanning/hostile-input rules. Milestone 4 is
//! scoped down to just [`split`]: bounds-checked, borrowed-output
//! extraction of caller-supplied [`ModuleSpec`] ranges, with no
//! `ModuleCatalog` trait, no overlap detection, and no catalog-identity
//! matching — see that function's doc comment.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

/// A canonical Kickstart ROM image is either 256 KiB or 512 KiB.
pub const ROM_SIZE_256K: usize = 256 * 1024;
/// A canonical Kickstart ROM image is either 256 KiB or 512 KiB.
pub const ROM_SIZE_512K: usize = 512 * 1024;

/// The Cloanto/Amiga Forever ROM container's 11-byte ASCII magic — no
/// NUL, no padding, no length field; the XOR'd Kickstart payload begins
/// immediately after it and runs to end-of-file (there is no container
/// footer). Confirmed against real Amiga Forever ROMs and
/// AmigaROMUtil (MIT) — see `docs/research/cloanto-hilo-facts.md`.
const CLOANTO_MAGIC: &[u8] = b"AMIROMTYPE1";

/// Byte order of a raw ROM dump relative to the canonical big-endian
/// layout. Real dumps show up in all four of these, depending on the
/// source machine's ROM socket/bus width — see `PLAN.md`'s loader
/// section (cross-checked against cdhooper/kicksmash32, BSD-2-Clause).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// `0123` — canonical, no reordering needed.
    Normal,
    /// `1032` — adjacent-byte swap within each 16-bit word.
    Order1032,
    /// `2301` — swap the upper/lower 16-bit halves of each 32-bit long.
    Order2301,
    /// `3210` — full byte-reversal within each 32-bit long.
    Order3210,
}

/// How a raw ROM file's bytes are encoded, as classified by
/// [`Loader::detect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomEncoding {
    /// A plain dump in the given [`ByteOrder`].
    Raw(ByteOrder),
    /// Cloanto/Amiga Forever's XOR-encoded container. [`Loader::normalize`]
    /// needs a `rom.key` to decode this.
    CloantoEncoded,
}

/// Errors from [`Loader`] and the hi/lo split/merge functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderError {
    /// [`RomEncoding::CloantoEncoded`] was detected or forced, but no key
    /// was supplied to [`Loader::normalize`].
    KeyRequired,
    /// A key was supplied to decode a [`RomEncoding::CloantoEncoded`]
    /// image, but it was empty (cycling XOR against an empty key is
    /// undefined — there's no byte to cycle).
    InvalidKey,
    /// [`Loader::detect`] could not classify the data as any known ROM
    /// format (no boot-vector signature matched under any byte order,
    /// and the Cloanto magic wasn't present).
    UnknownFormat,
    /// A byte-reordering operation ([`Loader::normalize`]'s raw leg, or
    /// [`split_hi_lo`]) was given data whose length isn't a multiple of
    /// 4 bytes, so it can't be split into whole 4-byte groups.
    UnalignedLength,
    /// [`merge_hi_lo`] was given two images of different lengths.
    MismatchedHiLoLength,
    /// [`merge_hi_lo`] was given hi/lo images of odd length, so they
    /// can't be split into whole 16-bit words.
    OddHiLoLength,
}

impl fmt::Display for LoaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoaderError::KeyRequired => {
                write!(f, "Cloanto-encoded ROM: a rom.key is required to decode it")
            }
            LoaderError::InvalidKey => {
                write!(f, "Cloanto rom.key must not be empty")
            }
            LoaderError::UnknownFormat => {
                write!(
                    f,
                    "data does not match any known ROM format (byte-order signature or Cloanto magic)"
                )
            }
            LoaderError::UnalignedLength => {
                write!(f, "ROM image length must be a multiple of 4 bytes")
            }
            LoaderError::MismatchedHiLoLength => {
                write!(f, "hi and lo EPROM images must be the same length")
            }
            LoaderError::OddHiLoLength => {
                write!(f, "hi and lo EPROM images must have even length")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LoaderError {}

/// Boot-vector signature table for byte-order detection: the four
/// *distinct* first longwords (big-endian, canonical order) transcribed
/// from kicksmash32's `detect_byte_order()` table
/// (`sw/hostsmash.c:2008-2016` at commit `a343550`, BSD-2-Clause). Two
/// kicksmash32 rows share `11144ef9` (Kickstart 2.04+ and the ROM
/// Switcher) and two share `11114ef9` (Kickstart 1.3, Logica-Dialoga,
/// and AROS); only the distinct values are kept here since detection
/// matches on this longword alone (see [`Loader::detect`]'s doc comment
/// for why the second longword is shape-checked, not exact-matched).
const SIG_LW0: [([u8; 4], &str); 4] = [
    ([0x11, 0x14, 0x4E, 0xF9], "Kickstart 2.04+ / ROM Switcher"),
    (
        [0x11, 0x11, 0x4E, 0xF9],
        "Kickstart 1.3 / Logica-Dialoga / AROS",
    ),
    ([0x61, 0x2E, 0x44, 0x47], "DiagROM 2.x Beta"),
    ([0x11, 0x14, 0x44, 0x47], "DiagROM 2.x"),
];

/// The four detectable/settable byte-order permutations, in a fixed
/// order used by [`Loader::detect`]'s search.
const PERMUTATIONS: [ByteOrder; 4] = [
    ByteOrder::Normal,
    ByteOrder::Order1032,
    ByteOrder::Order2301,
    ByteOrder::Order3210,
];

/// Applies `order` to one 4-byte group (`word[0]` = the lowest file
/// offset), per the exact byte-index semantics confirmed in
/// `docs/research/byte-order-facts.md` §2.
///
/// Every one of these four permutations is its own inverse — applying
/// the same `order` twice returns the original bytes — so this single
/// function serves both directions (canonical→raw and raw→canonical)
/// with no separate inverse table needed.
const fn permute_word(word: [u8; 4], order: ByteOrder) -> [u8; 4] {
    match order {
        ByteOrder::Normal => word,
        ByteOrder::Order1032 => [word[1], word[0], word[3], word[2]],
        ByteOrder::Order2301 => [word[2], word[3], word[0], word[1]],
        ByteOrder::Order3210 => [word[3], word[2], word[1], word[0]],
    }
}

/// Normalizes a raw ROM file's bytes into a canonical image.
pub struct Loader;

impl Loader {
    /// Inspects leading bytes against known boot-vector signatures under
    /// all four [`ByteOrder`] permutations (plus the Cloanto
    /// `AMIROMTYPE1` container magic), and classifies. Does not
    /// decode/reorder — see [`Loader::normalize`] for that.
    ///
    /// Returns `None` when `data` doesn't match anything recognized —
    /// too short (fewer than 8 bytes, unless it matches the Cloanto
    /// magic, which only needs its own 11 bytes), or its first longword
    /// doesn't match the boot-vector signature table under any
    /// permutation.
    ///
    /// Matching proceeds: for each table entry and each permutation,
    /// compare the image's raw first big-endian `u32` against the
    /// table value permuted that way. On a match, the *second* longword
    /// (similarly un-permuted back to canonical order) must additionally
    /// look like a plausible ROM-space address — high byte `0x00`,
    /// second byte in `0xF0..=0xFF` (covering the confirmed real bases
    /// `F8`/`FC`/`F0`/`FF`). This is deliberately neither of
    /// kicksmash32's two behaviours (exact-matching the pair, or
    /// ignoring the second longword entirely): an exact-value check on
    /// the second longword would wrongly reject unknown ROM builds
    /// whose boot PC isn't one of the sampled values, while skipping it
    /// entirely would accept any four bytes that happen to share a
    /// first longword with a real ROM family. See `PLAN.md`'s
    /// "Boot-vector signature table" milestone-1 item for the full
    /// rationale.
    pub fn detect(data: &[u8]) -> Option<RomEncoding> {
        if data.starts_with(CLOANTO_MAGIC) {
            return Some(RomEncoding::CloantoEncoded);
        }
        if data.len() < 8 {
            return None;
        }
        let lw0 = [data[0], data[1], data[2], data[3]];
        let lw1 = [data[4], data[5], data[6], data[7]];
        for &(sig, _family) in SIG_LW0.iter() {
            for &order in PERMUTATIONS.iter() {
                if lw0 != permute_word(sig, order) {
                    continue;
                }
                let canon_lw1 = permute_word(lw1, order);
                if canon_lw1[0] == 0x00 && (0xF0..=0xFF).contains(&canon_lw1[1]) {
                    return Some(RomEncoding::Raw(order));
                }
            }
        }
        None
    }

    /// Normalizes `data` into a canonical raw image. `key` is required
    /// iff [`RomEncoding::CloantoEncoded`] is detected.
    ///
    /// Errors: [`LoaderError::UnknownFormat`] if [`Loader::detect`]
    /// can't classify `data` at all; [`LoaderError::KeyRequired`] /
    /// [`LoaderError::InvalidKey`] for a missing/empty Cloanto key;
    /// [`LoaderError::UnalignedLength`] if a non-[`ByteOrder::Normal`]
    /// raw image's length isn't a multiple of 4 bytes (reordering is
    /// only defined on whole 4-byte groups — a trailing partial group
    /// can't occur for a genuinely swapped dump of a real ROM, since
    /// canonical sizes are themselves multiples of 4, but a caller can
    /// still hand in arbitrary bytes).
    pub fn normalize(data: &[u8], key: Option<&[u8]>) -> Result<Vec<u8>, LoaderError> {
        match Self::detect(data) {
            Some(RomEncoding::CloantoEncoded) => {
                let key = key.ok_or(LoaderError::KeyRequired)?;
                decode_cloanto(data, key)
            }
            Some(RomEncoding::Raw(ByteOrder::Normal)) => Ok(data.to_vec()),
            Some(RomEncoding::Raw(order)) => {
                if data.len() % 4 != 0 {
                    return Err(LoaderError::UnalignedLength);
                }
                let mut out = Vec::with_capacity(data.len());
                for chunk in data.chunks_exact(4) {
                    let word = [chunk[0], chunk[1], chunk[2], chunk[3]];
                    out.extend_from_slice(&permute_word(word, order));
                }
                Ok(out)
            }
            None => Err(LoaderError::UnknownFormat),
        }
    }
}

/// Decodes a Cloanto/Amiga Forever-encoded ROM: strips the 11-byte
/// `AMIROMTYPE1` magic and XORs the rest against `key`, cycling from
/// key index 0. The payload runs to end-of-file exactly — there is no
/// container footer, so the result is the complete canonical image with
/// nothing to trim (confirmed against real Amiga Forever ROMs; see
/// `docs/research/cloanto-hilo-facts.md`).
fn decode_cloanto(data: &[u8], key: &[u8]) -> Result<Vec<u8>, LoaderError> {
    if key.is_empty() {
        return Err(LoaderError::InvalidKey);
    }
    let payload = &data[CLOANTO_MAGIC.len()..];
    let mut out = Vec::with_capacity(payload.len());
    for (i, &b) in payload.iter().enumerate() {
        out.push(b ^ key[i % key.len()]);
    }
    Ok(out)
}

/// Merges two same-size hi/lo EPROM dumps into one canonical image: for
/// each output 32-bit longword, the hi file supplies the first 16-bit
/// word (bytes `b0 b1`) and the lo file supplies the second (`b2 b3`) —
/// pure canonical word-interleave, confirmed against AmigaROMUtil (MIT)
/// in `docs/research/cloanto-hilo-facts.md` §B1. No byte-swapping is
/// baked in here: the burner convention of swapping to `1032` order
/// before interleaving composes separately via [`Loader::normalize`]/
/// [`ByteOrder`], keeping the two axes orthogonal (rationale in
/// `PLAN.md`'s "Hi/lo EPROM interleave width" milestone-1 item).
///
/// Errors if `hi`/`lo` differ in length, or if their (common) length is
/// odd — each is a sequence of whole 16-bit words.
pub fn merge_hi_lo(hi: &[u8], lo: &[u8]) -> Result<Vec<u8>, LoaderError> {
    if hi.len() != lo.len() {
        return Err(LoaderError::MismatchedHiLoLength);
    }
    if hi.len() % 2 != 0 {
        return Err(LoaderError::OddHiLoLength);
    }
    let mut out = Vec::with_capacity(hi.len() + lo.len());
    for i in (0..hi.len()).step_by(2) {
        out.push(hi[i]);
        out.push(hi[i + 1]);
        out.push(lo[i]);
        out.push(lo[i + 1]);
    }
    Ok(out)
}

/// Inverse of [`merge_hi_lo`]: splits a canonical image into hi/lo EPROM
/// dumps, per the same 16-bit-word interleave (hi = `b0 b1` of each
/// longword, lo = `b2 b3`).
///
/// Errors with [`LoaderError::UnalignedLength`] if `rom.len()` isn't a
/// multiple of 4 — the interleave operates on whole 32-bit longwords.
pub fn split_hi_lo(rom: &[u8]) -> Result<(Vec<u8>, Vec<u8>), LoaderError> {
    if rom.len() % 4 != 0 {
        return Err(LoaderError::UnalignedLength);
    }
    let mut hi = Vec::with_capacity(rom.len() / 2);
    let mut lo = Vec::with_capacity(rom.len() / 2);
    for chunk in rom.chunks_exact(4) {
        hi.push(chunk[0]);
        hi.push(chunk[1]);
        lo.push(chunk[2]);
        lo.push(chunk[3]);
    }
    Ok((hi, lo))
}

/// Sums every big-endian 32-bit longword in `data` with ones'-complement
/// (end-around-carry) addition — the standard Amiga ROM checksum
/// primitive. A correct Kickstart ROM's total, including its own stored
/// checksum word, folds to `0xFFFFFFFF`.
///
/// This is generic arithmetic, independent of where in a ROM image the
/// checksum word itself lives — [`KickRom::verify_check_sum`] wires it up
/// to the actual stored-checksum offset once that's confirmed (see
/// `PLAN.md`'s milestone-1 facts pass). Route all checksum math through
/// this function rather than hand-rolling it elsewhere.
///
/// `data.len()` must be a multiple of 4; longwords are read big-endian.
pub fn checksum_ones_complement(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    for chunk in data.chunks_exact(4) {
        let word = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let (partial, carried) = sum.overflowing_add(word);
        sum = if carried { partial + 1 } else { partial };
    }
    sum
}

/// A canonical Kickstart ROM image, borrowed for inspection.
///
/// Allocation-free: every method reads directly from the borrowed
/// `data`. Construct via [`KickRom::new`] after normalizing raw bytes
/// through [`Loader`].
pub struct KickRom<'a> {
    data: &'a [u8],
}

impl<'a> KickRom<'a> {
    /// Wraps a canonical (already-normalized) ROM image for inspection.
    pub fn new(data: &'a [u8]) -> Self {
        KickRom { data }
    }

    /// `true` iff the image is exactly 256 KiB or 512 KiB.
    pub fn check_size(&self) -> bool {
        matches!(self.data.len(), ROM_SIZE_256K | ROM_SIZE_512K)
    }

    /// `true` iff a valid Kickstart ROM header is found at the start of
    /// the image (`docs/research/header-footer-facts.md` §1).
    ///
    /// The marker word at offset 0x00 must be either `0x1114` (accepted
    /// at any size) or `0x1111` — but `0x1111` is accepted only when the
    /// image is exactly [`ROM_SIZE_256K`], matching `romtool`'s
    /// size-dependent rule (oracle-confirmed: `0x1111` at 512 KiB is
    /// rejected). The word at offset 0x02 must be exactly `0x4EF9`
    /// (`JMP abs.L`). **Known false negative** (§1 addendum): this
    /// rejects the genuine Kickstart 1.4-beta ROM, which carries
    /// `0x1111` at 512 KiB — matched here deliberately for parity with
    /// `romtool`, not a bug in this crate.
    ///
    /// Returns `false` (never panics) on any image shorter than 0x18
    /// bytes.
    pub fn check_header(&self) -> bool {
        if self.data.len() < 0x18 {
            return false;
        }
        let marker = u16::from_be_bytes([self.data[0x00], self.data[0x01]]);
        let opcode = u16::from_be_bytes([self.data[0x02], self.data[0x03]]);
        let marker_ok = marker == 0x1114 || (marker == 0x1111 && self.data.len() == ROM_SIZE_256K);
        marker_ok && opcode == 0x4EF9
    }

    /// `true` iff a valid Kickstart ROM footer is found at the end of
    /// the image (`docs/research/header-footer-facts.md` §2).
    ///
    /// Validates the seven trailing u16 words at `len-14..len-1`
    /// against `0x0019..0x001F`. The word at `len-16` is deliberately
    /// **not** checked — real 1.3 ROMs carry `0xFFC0` there and still
    /// pass, per the oracle isolation in §2.
    ///
    /// Returns `false` (never panics) on any image shorter than 24
    /// bytes.
    pub fn check_footer(&self) -> bool {
        let len = self.data.len();
        if len < 24 {
            return false;
        }
        const EXPECTED: [u16; 7] = [0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F];
        for (i, &want) in EXPECTED.iter().enumerate() {
            let off = len - 14 + i * 2;
            let word = u16::from_be_bytes([self.data[off], self.data[off + 1]]);
            if word != want {
                return false;
            }
        }
        true
    }

    /// `true` iff the footer's size field (u32 at `len-20`) equals the
    /// image's actual length (`docs/research/header-footer-facts.md`
    /// §2).
    ///
    /// This implements the *true* semantics of the field: `romtool`'s
    /// printed `size_field` line is cosmetic and never goes NOK under
    /// any mutation (oracle-confirmed), even though `is_kick` silently
    /// depends on this equality. This method is honest — it reports the
    /// real comparison, matching what `is_kick_rom` needs.
    ///
    /// Returns `false` (never panics) on any image shorter than 24
    /// bytes.
    pub fn check_size_field(&self) -> bool {
        let len = self.data.len();
        if len < 24 {
            return false;
        }
        let off = len - 20;
        let field = u32::from_be_bytes([
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ]);
        field as usize == len
    }

    /// `true` iff the image's length is a nonzero multiple of 4 and at
    /// least 24 bytes, and [`checksum_ones_complement`] over the whole
    /// image folds to `0xFFFFFFFF` (`docs/research/header-footer-facts.md`
    /// §3).
    ///
    /// Returns `false` (never panics) on any image that fails the
    /// length preconditions.
    pub fn verify_check_sum(&self) -> bool {
        let len = self.data.len();
        if len == 0 || len < 24 || len % 4 != 0 {
            return false;
        }
        checksum_ones_complement(self.data) == 0xFFFF_FFFF
    }

    /// `true` iff the "kickety split" signature — a second copy of the
    /// marker word + `JMP` opcode at the image's exact midpoint
    /// (`docs/research/header-footer-facts.md` §4) — is present.
    ///
    /// Stricter than [`KickRom::check_header`]: only marker `0x1111` is
    /// accepted at the midpoint (not `0x1114`), matching the oracle
    /// result. The address word following the opcode is not checked.
    /// Requires an even length; returns `false` (never panics) on any
    /// image too short to hold the two words at `len/2`.
    pub fn check_kickety_split(&self) -> bool {
        let len = self.data.len();
        if len == 0 || len % 2 != 0 {
            return false;
        }
        let mid = len / 2;
        if mid + 4 > len {
            return false;
        }
        let marker = u16::from_be_bytes([self.data[mid], self.data[mid + 1]]);
        let opcode = u16::from_be_bytes([self.data[mid + 2], self.data[mid + 3]]);
        marker == 0x1111 && opcode == 0x4EF9
    }

    /// `true` iff the m68k `RESET` opcode (`0x4E70`) is present at the
    /// fixed absolute offset `0xD0`
    /// (`docs/research/header-footer-facts.md` §5), independent of
    /// `boot_pc`/`base_addr`.
    ///
    /// Returns `false` (never panics) on any image shorter than 0xD2
    /// bytes.
    pub fn check_magic_reset(&self) -> bool {
        const RESET_OFFSET: usize = 0xD0;
        if self.data.len() < RESET_OFFSET + 2 {
            return false;
        }
        u16::from_be_bytes([self.data[RESET_OFFSET], self.data[RESET_OFFSET + 1]]) == 0x4E70
    }

    /// `true` iff the image passes every check that gates "is a
    /// Kickstart ROM": size, header, footer, size field, checksum.
    /// Kickety-split and magic-reset are informational and
    /// deliberately *not* part of this conjunction — confirmed by
    /// oracle (`docs/research/header-footer-facts.md` §4/§5).
    pub fn is_kick_rom(&self) -> bool {
        self.check_size()
            && self.check_header()
            && self.check_footer()
            && self.check_size_field()
            && self.verify_check_sum()
    }

    /// Reads the stored checksum value (u32 at `len-24`) from the
    /// footer (`docs/research/header-footer-facts.md` §3).
    ///
    /// Returns `None` on any image shorter than 24 bytes.
    pub fn read_check_sum(&self) -> Option<u32> {
        let len = self.data.len();
        if len < 24 {
            return None;
        }
        let off = len - 24;
        Some(u32::from_be_bytes([
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ]))
    }

    /// The ROM's base address in the Amiga's memory map, derived (never
    /// looked up) as `boot_pc & 0xFFFF0000`
    /// (`docs/research/header-footer-facts.md` §1).
    ///
    /// Returns `None` on any image shorter than 0x18 bytes.
    pub fn base_addr(&self) -> Option<u32> {
        self.boot_pc().map(|pc| pc & 0xFFFF_0000)
    }

    /// The boot program counter — the u32 `JMP` target at header offset
    /// 0x04, which *is* the whole first instruction
    /// (`docs/research/header-footer-facts.md` §1).
    ///
    /// Returns `None` on any image shorter than 0x18 bytes.
    pub fn boot_pc(&self) -> Option<u32> {
        if self.data.len() < 0x18 {
            return None;
        }
        Some(u32::from_be_bytes([
            self.data[0x04],
            self.data[0x05],
            self.data[0x06],
            self.data[0x07],
        ]))
    }

    /// The ROM's (major, minor) revision, read from header offset 0x0C
    /// (`docs/research/header-footer-facts.md` §1). Note the §1
    /// addendum: pre-1.2 ROMs carry an unpopulated `(0xFFFF, 0xFFFF)`
    /// here — this method returns that value as-is, without inventing
    /// meaning for it.
    ///
    /// Returns `None` on any image shorter than 0x18 bytes.
    pub fn rom_rev(&self) -> Option<(u16, u16)> {
        if self.data.len() < 0x18 {
            return None;
        }
        Some((
            u16::from_be_bytes([self.data[0x0C], self.data[0x0D]]),
            u16::from_be_bytes([self.data[0x0E], self.data[0x0F]]),
        ))
    }

    /// The embedded Exec's (major, minor) revision, read from the fixed
    /// header offset 0x10 — not resident-derived (proven by oracle,
    /// `docs/research/header-footer-facts.md` §6).
    ///
    /// Returns `None` on any image shorter than 0x18 bytes.
    pub fn exec_rev(&self) -> Option<(u16, u16)> {
        if self.data.len() < 0x18 {
            return None;
        }
        Some((
            u16::from_be_bytes([self.data[0x10], self.data[0x11]]),
            u16::from_be_bytes([self.data[0x12], self.data[0x13]]),
        ))
    }

    /// Scans the image for `Resident` (RomTag) structures, `romtool
    /// scan`-style. See [`ResidentScan`]'s doc comment for the full
    /// algorithm and hostile-input rules.
    ///
    /// The self-pointer validity check (`rt_MatchTag == base_addr +
    /// offset`) needs a known [`KickRom::base_addr`] to mean anything; if
    /// that's `None` (image shorter than 0x18 bytes), the returned
    /// iterator yields nothing rather than skip the check — a matchword
    /// found without a way to validate it is not a confirmed hit.
    pub fn scan(&self) -> ResidentScan<'a> {
        ResidentScan {
            data: self.data,
            base_addr: self.base_addr(),
            pos: 0,
        }
    }

    /// Aggregates every check/value into one [`RomInfo`], matching
    /// `romtool info`'s field set.
    pub fn info(&self) -> RomInfo {
        RomInfo {
            size_ok: self.check_size(),
            header_ok: self.check_header(),
            footer_ok: self.check_footer(),
            size_field_ok: self.check_size_field(),
            chk_sum_ok: self.verify_check_sum(),
            kickety_split_ok: self.check_kickety_split(),
            magic_reset_ok: self.check_magic_reset(),
            is_kick: self.is_kick_rom(),
            check_sum: self.read_check_sum(),
            base_addr: self.base_addr(),
            boot_pc: self.boot_pc(),
            rom_rev: self.rom_rev(),
            exec_rev: self.exec_rev(),
        }
    }
}

/// Errors from [`seal_checksum`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealError {
    /// The image is shorter than the 24-byte footer that holds the
    /// checksum word.
    TooShort,
    /// The image length is not a multiple of 4, so it cannot be summed
    /// as consecutive big-endian longwords.
    NotLongwordAligned,
}

impl fmt::Display for SealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SealError::TooShort => {
                write!(f, "image is shorter than the 24-byte checksum footer")
            }
            SealError::NotLongwordAligned => {
                write!(f, "image length is not a multiple of 4")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for SealError {}

/// Seals `rom`'s stored checksum: zeroes the u32 at `len-24`, computes
/// [`checksum_ones_complement`] over the whole image, and stores the
/// bitwise complement of that sum back at `len-24` — the inverse of
/// [`KickRom::verify_check_sum`]
/// (`docs/research/header-footer-facts.md` §3). Always routes through
/// [`checksum_ones_complement`], never a hand-rolled sum.
///
/// # Errors
///
/// [`SealError::TooShort`] if `rom.len() < 24`;
/// [`SealError::NotLongwordAligned`] if `rom.len() % 4 != 0`.
pub fn seal_checksum(rom: &mut [u8]) -> Result<(), SealError> {
    let len = rom.len();
    if len < 24 {
        return Err(SealError::TooShort);
    }
    if len % 4 != 0 {
        return Err(SealError::NotLongwordAligned);
    }
    let checksum_off = len - 24;
    rom[checksum_off..checksum_off + 4].copy_from_slice(&0u32.to_be_bytes());
    let sum = checksum_ones_complement(rom);
    rom[checksum_off..checksum_off + 4].copy_from_slice(&(!sum).to_be_bytes());
    Ok(())
}

/// The aggregate result of every [`KickRom`] check, matching `romtool
/// info`'s field set. Value fields are `None` exactly when the image is
/// too short to contain that field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RomInfo {
    pub size_ok: bool,
    pub header_ok: bool,
    pub footer_ok: bool,
    pub size_field_ok: bool,
    pub chk_sum_ok: bool,
    pub kickety_split_ok: bool,
    pub magic_reset_ok: bool,
    pub is_kick: bool,
    pub check_sum: Option<u32>,
    pub base_addr: Option<u32>,
    pub boot_pc: Option<u32>,
    pub rom_rev: Option<(u16, u16)>,
    pub exec_rev: Option<(u16, u16)>,
}

/// The 68000 "ILLEGAL" instruction word every `Resident` structure
/// begins with (`exec/resident.h`'s `RTC_MATCHWORD`, NDK-confirmed).
const RTC_MATCHWORD: u16 = 0x4AFC;

/// Byte length of a `struct Resident` on m68k: ten fields, no padding
/// (a `UWORD` then nine `ULONG`/`APTR`/byte-sized fields, all naturally
/// aligned on m68k's 2-byte minimum alignment) — `2 + 4 + 4 + 1 + 1 + 1
/// + 1 + 4 + 4 + 4 == 26`. NDK-confirmed against `exec/resident.h`.
const RESIDENT_STRUCT_LEN: usize = 26;

/// Longest run searched for a NUL terminator when resolving a
/// `rt_Name`/`rt_IdString` pointer, so a corrupt/missing NUL can't walk
/// the scan to the end of a huge image. Real Amiga strings here are a
/// handful of bytes (module and library names); 256 is generous.
const RESIDENT_STRING_SEARCH_CAP: usize = 256;

/// A borrowed view over one `Resident` (RomTag) structure found by
/// [`KickRom::scan`], laid out per `exec/resident.h` (NDK-confirmed;
/// field order and byte offsets from the struct's start):
///
/// | offset | field           | type                |
/// |-------:|-----------------|----------------------|
/// | 0x00   | `rt_MatchWord`  | `UWORD` (`match_word`) |
/// | 0x02   | `rt_MatchTag`   | `APTR` self-pointer (validated, not stored) |
/// | 0x06   | `rt_EndSkip`    | `APTR` (`end_skip`) |
/// | 0x0A   | `rt_Flags`      | `UBYTE` (`flags`) |
/// | 0x0B   | `rt_Version`    | `UBYTE` (`version`) |
/// | 0x0C   | `rt_Type`       | `UBYTE` (`node_type`) |
/// | 0x0D   | `rt_Pri`        | `BYTE` (`priority`) |
/// | 0x0E   | `rt_Name`       | `char *` (`name`, resolved) |
/// | 0x12   | `rt_IdString`   | `char *` (`id_string`, resolved) |
/// | 0x16   | `rt_Init`       | `APTR` (`init_addr`) |
///
/// `name`/`id_string` are raw `&[u8]`, NUL-terminated in ROM but *not*
/// guaranteed valid UTF-8 — this crate never claims `&str` for bytes it
/// hasn't validated. Callers wanting a display string should use
/// `String::from_utf8_lossy(resident.name)` (or `.id_string`). Either
/// field is an empty slice when its pointer couldn't be resolved to an
/// in-bounds, NUL-terminated run within
/// [`KickRom::scan`]'s search cap — never a panic, never a guess.
///
/// `offset` is where this structure's `rt_MatchWord` was found in the
/// image (not an NDK field — added for caller convenience, e.g. to
/// report a hit's location). `end_skip` is the raw `rt_EndSkip` value
/// as stored in the ROM: [`ResidentScan`] never dereferences or trusts
/// it for scan control flow (see that type's doc comment), so a
/// corrupt or backwards value here is reported as-is and is the
/// caller's problem if they choose to use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resident<'a> {
    pub match_word: u16,
    pub flags: u8,
    pub version: u8,
    pub node_type: u8,
    pub priority: i8,
    pub name: &'a [u8],
    pub id_string: &'a [u8],
    pub init_addr: u32,
    pub offset: usize,
    pub end_skip: u32,
}

/// Resolves a `rt_Name`/`rt_IdString`-style pointer (an absolute
/// ROM-space address) to a borrowed, NUL-terminated (NUL excluded)
/// slice of `data`.
///
/// Returns an empty slice — never panics, never scans past
/// `RESIDENT_STRING_SEARCH_CAP` (256) bytes — when: `ptr` translates (via
/// `ptr - base_addr`) to an offset at or past `data.len()` (this also
/// naturally rejects `ptr < base_addr`, since the wrapping subtraction
/// underflows to a huge offset); or no NUL byte is found within the
/// capped search window.
fn resolve_resident_str(data: &[u8], base_addr: u32, ptr: u32) -> &[u8] {
    let start = ptr.wrapping_sub(base_addr) as usize;
    if start >= data.len() {
        return &[];
    }
    let cap = RESIDENT_STRING_SEARCH_CAP.min(data.len() - start);
    let window = &data[start..start + cap];
    match window.iter().position(|&b| b == 0) {
        Some(nul_at) => &window[..nul_at],
        None => &[],
    }
}

/// Allocation-free iterator over [`Resident`] structures found in a
/// [`KickRom`] image, returned by [`KickRom::scan`] (`romtool scan`
/// parity).
///
/// # Algorithm
///
/// Every 2-byte-aligned offset in the image is inspected for
/// `RTC_MATCHWORD` — m68k instructions (and so `rt_MatchWord`, the
/// 68000 `ILLEGAL` opcode) only ever land on even offsets, so odd
/// offsets are never checked. At each matchword, if the remaining 24
/// bytes of a `struct Resident` (`RESIDENT_STRUCT_LEN`, 26 bytes total)
/// don't fit in the image, the candidate is skipped. Otherwise
/// `rt_MatchTag` (the `u32` self-pointer at offset+2) must equal
/// `base_addr + offset` *exactly* — this is the **only** validity gate;
/// a matchword with any other `rt_MatchTag` is not a resident and
/// scanning simply continues from the next word. `rt_Name`/`rt_IdString`
/// pointers on a valid hit are resolved via `resolve_resident_str`
/// (bounds-checked, capped NUL search, never a panic).
///
/// After a match (valid or not), scanning always continues from the
/// *next word* (`offset + 2`) — **never** by jumping via `rt_EndSkip`.
/// A hostile or corrupt `rt_EndSkip` (pointing backwards, out of
/// bounds, or anywhere at all) is therefore incapable of affecting scan
/// control flow; it is surfaced on [`Resident::end_skip`] as raw data
/// for the caller, never dereferenced by this scanner. This is a
/// deliberate deviation from following the ROM's own module-chaining
/// convention, in favor of hostile-input robustness.
///
/// # Termination
///
/// `pos` strictly increases by 2 every iteration and the loop stops
/// once `pos + 2 > data.len()`, so this iterator always terminates,
/// including on a zero-length or 1-byte image (immediately empty) and
/// on a matchword at the very last 2 bytes of the image (detected, but
/// skipped for lacking room for the rest of the struct).
///
/// # `base_addr`
///
/// The self-pointer check is meaningless without a known
/// [`KickRom::base_addr`]; when that's `None`, this iterator is
/// permanently empty (see [`KickRom::scan`]'s doc comment).
pub struct ResidentScan<'a> {
    data: &'a [u8],
    base_addr: Option<u32>,
    pos: usize,
}

impl<'a> Iterator for ResidentScan<'a> {
    type Item = Resident<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let base_addr = self.base_addr?;
        while self.pos + 2 <= self.data.len() {
            let offset = self.pos;
            // Always advance to the next word, regardless of whether
            // this candidate turns out valid — never re-visits, never
            // jumps via untrusted struct contents.
            self.pos += 2;

            let word = u16::from_be_bytes([self.data[offset], self.data[offset + 1]]);
            if word != RTC_MATCHWORD {
                continue;
            }
            let struct_end = match offset.checked_add(RESIDENT_STRUCT_LEN) {
                Some(end) if end <= self.data.len() => end,
                _ => continue,
            };
            let match_tag = u32::from_be_bytes([
                self.data[offset + 2],
                self.data[offset + 3],
                self.data[offset + 4],
                self.data[offset + 5],
            ]);
            let expected = base_addr.wrapping_add(offset as u32);
            if match_tag != expected {
                continue;
            }

            let end_skip = u32::from_be_bytes([
                self.data[offset + 6],
                self.data[offset + 7],
                self.data[offset + 8],
                self.data[offset + 9],
            ]);
            let flags = self.data[offset + 10];
            let version = self.data[offset + 11];
            let node_type = self.data[offset + 12];
            let priority = self.data[offset + 13] as i8;
            let name_ptr = u32::from_be_bytes([
                self.data[offset + 14],
                self.data[offset + 15],
                self.data[offset + 16],
                self.data[offset + 17],
            ]);
            let id_ptr = u32::from_be_bytes([
                self.data[offset + 18],
                self.data[offset + 19],
                self.data[offset + 20],
                self.data[offset + 21],
            ]);
            let init_addr = u32::from_be_bytes([
                self.data[offset + 22],
                self.data[offset + 23],
                self.data[offset + 24],
                self.data[offset + 25],
            ]);
            debug_assert_eq!(struct_end, offset + RESIDENT_STRUCT_LEN);

            let name = resolve_resident_str(self.data, base_addr, name_ptr);
            let id_string = resolve_resident_str(self.data, base_addr, id_ptr);

            return Some(Resident {
                match_word: word,
                flags,
                version,
                node_type,
                priority,
                name,
                id_string,
                init_addr,
                offset,
                end_skip,
            });
        }
        None
    }
}

/// One caller-supplied module range to extract via [`split`]: a name and
/// a byte range (`offset..offset+length`) into a ROM image.
///
/// This is plain, caller-constructed input — never produced by this
/// crate. Per `PLAN.md`'s "Milestone 4" design, there is deliberately no
/// `ModuleCatalog` trait here: this crate does zero file I/O, so a
/// catalog *file format* (how a list of these gets read from disk) is
/// the CLI crate's concern, not this one's. A `ModuleSpec` carries only
/// `{name, offset, length}` — no relocation data, no "kind" tag, no
/// cross-reference to [`ResidentScan`]; scanning and catalog-driven
/// splitting are unrelated concepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleSpec<'a> {
    /// The module's name, as given by the catalog this came from.
    pub name: &'a str,
    /// Byte offset of the module's start within the ROM image.
    pub offset: usize,
    /// Length of the module in bytes.
    pub length: usize,
}

/// One module extracted by [`split`]: a name and a borrowed slice of the
/// source ROM.
///
/// `data` borrows directly from the `rom` passed to [`split`] — per
/// `PLAN.md`'s Milestone 4 decision, `split` never transforms bytes, only
/// cuts them, matching [`KickRom`]'s and [`ResidentScan`]'s existing
/// allocation-free convention. A caller wanting an owned `Vec<u8>` calls
/// `.to_vec()` on `data` themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Module<'a> {
    /// The module's name, borrowed from the [`ModuleSpec`] it came from.
    pub name: &'a str,
    /// The module's bytes: `&rom[offset..offset + length]`.
    pub data: &'a [u8],
}

/// Errors from [`split`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    /// A [`ModuleSpec`]'s `offset..offset+length` range does not fit
    /// within the ROM: either the addition overflows `usize` (a hostile
    /// offset/length pair), or the resulting end exceeds `rom.len()`.
    /// `index` is this module's position in the `modules` slice passed
    /// to [`split`] (not its name — kept cheap, no `alloc::string::String`
    /// needed to report this).
    ModuleOutOfBounds {
        /// Index of the offending [`ModuleSpec`] within the `modules`
        /// slice.
        index: usize,
        /// The requested offset.
        offset: usize,
        /// The requested length.
        length: usize,
        /// The ROM's actual length.
        rom_len: usize,
    },
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplitError::ModuleOutOfBounds {
                index,
                offset,
                length,
                rom_len,
            } => {
                write!(
                    f,
                    "module {} at offset {} length {} does not fit within a {}-byte ROM",
                    index, offset, length, rom_len
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for SplitError {}

/// Extracts `modules` from `rom` as borrowed slices, per `PLAN.md`'s
/// "Milestone 4" design.
///
/// For each [`ModuleSpec`] in `modules`, in order, this bounds-checks
/// `offset..offset+length` against `rom` (a hard safety property, like
/// every other check in this crate) and either produces a [`Module`]
/// borrowing that range, or fails. Bounds-checking uses
/// [`usize::checked_add`] for `offset + length`, so a hostile/absurd
/// pair (e.g. `offset: usize::MAX, length: 1`) reports
/// [`SplitError::ModuleOutOfBounds`] rather than panicking on overflow.
/// A `length` of `0` is valid and yields an empty slice, not an error.
///
/// This is fail-fast, matching the return type
/// `Result<Vec<Module>, SplitError>`: the first out-of-bounds module
/// aborts the whole call with `Err` and no `Vec` is returned at all —
/// there is no partial-success mode where modules before the failure are
/// still handed back. An empty `modules` slice yields `Ok(vec![])`.
///
/// Per `PLAN.md`, this deliberately does **not** detect overlapping
/// module ranges (two specs claiming the same bytes is a complaint about
/// the catalog's own consistency, not a memory-safety concern — out of
/// scope), and has no opinion on whether `modules` "belongs" to `rom`
/// (matching a catalog to a ROM, e.g. by KickSum, is a lookup step for
/// the caller, entirely outside this crate).
///
/// # Errors
///
/// [`SplitError::ModuleOutOfBounds`] identifying the first (by index)
/// [`ModuleSpec`] whose range does not fit within `rom`.
pub fn split<'a>(rom: &'a [u8], modules: &[ModuleSpec<'a>]) -> Result<Vec<Module<'a>>, SplitError> {
    let mut out = Vec::with_capacity(modules.len());
    for (index, spec) in modules.iter().enumerate() {
        let end = match spec.offset.checked_add(spec.length) {
            Some(end) if end <= rom.len() => end,
            _ => {
                return Err(SplitError::ModuleOutOfBounds {
                    index,
                    offset: spec.offset,
                    length: spec.length,
                    rom_len: rom.len(),
                })
            }
        };
        out.push(Module {
            name: spec.name,
            data: &rom[spec.offset..end],
        });
    }
    Ok(out)
}

/// Test-only synthetic Kickstart image fixtures — never ships real ROM
/// bytes (see `PLAN.md`'s "ROM images themselves" section). Builds a
/// minimal valid canonical image encoding exactly the milestone-1 facts
/// (`docs/research/header-footer-facts.md` §1 header, §2 footer, §3
/// checksum, §5 magic reset), sealed via [`checksum_ones_complement`] —
/// never hand-rolled arithmetic, per `PLAN.md`'s rule.
#[cfg(test)]
mod fixtures {
    use super::*;
    use alloc::vec;

    /// Marker word at header offset 0x00, size-dependent per §1's oracle
    /// result (`0x1111` accepted only for 256 KiB, `0x1114` for 512 KiB).
    const MARKER_256K: u16 = 0x1111;
    const MARKER_512K: u16 = 0x1114;

    /// `JMP <abs.L>` opcode required exactly at header offset 0x02.
    const JMP_OPCODE: u16 = 0x4EF9;

    /// Constant of unresolved purpose at header offset 0x08, present in
    /// every real ROM sampled (§1).
    const HEADER_CONST_08: u32 = 0x0000_FFFF;

    /// Separator constant at header offset 0x14 (§1).
    const HEADER_SEPARATOR: u32 = 0xFFFF_FFFF;

    /// `m68k RESET` opcode, required at fixed absolute offset 0xD0 (§5).
    const RESET_OPCODE: u16 = 0x4E70;
    const RESET_OFFSET: usize = 0xD0;

    /// The seven footer "vector index" words that the footer check
    /// actually validates (`len-14..len-1`); the eighth (`len-16`) is
    /// tolerated to be anything (§2) but conventionally `0x0018` too.
    const FOOTER_VECTOR_WORDS: [u16; 8] = [
        0x0018, 0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F,
    ];

    /// Default base address for a 512 KiB image (§1: the conventional
    /// 2.0+ era value).
    pub const DEFAULT_BASE_512K: u32 = 0x00F8_0000;
    /// Default base address for a 256 KiB image (§1: the conventional
    /// 1.x era value).
    pub const DEFAULT_BASE_256K: u32 = 0x00FC_0000;

    /// Parameters for [`synthetic_rom`].
    #[derive(Debug, Clone, Copy)]
    pub struct RomFixtureParams {
        /// Total image size — must be [`ROM_SIZE_256K`] or
        /// [`ROM_SIZE_512K`], enforced by [`synthetic_rom`].
        pub size: usize,
        /// (major, minor) stored at header offset 0x0C.
        pub rom_rev: (u16, u16),
        /// (major, minor) stored at header offset 0x10.
        pub exec_rev: (u16, u16),
        /// Top-16-bit base address; `boot_pc` is derived as
        /// `base_addr + 0xD2` so it lands right after the magic-reset
        /// opcode at fixed offset 0xD0, matching the real ROMs sampled
        /// in §1/§5. Only the top 16 bits are meaningful (`boot_pc =
        /// base_addr | 0xD2`, i.e. any low 16 bits given here are
        /// ignored — callers should pass a page-aligned address as
        /// every real sample does).
        pub base_addr: u32,
    }

    impl RomFixtureParams {
        /// Defaults for a 512 KiB image: base `0x00F80000`, rev 0.0.
        pub fn new_512k() -> Self {
            RomFixtureParams {
                size: ROM_SIZE_512K,
                rom_rev: (0, 0),
                exec_rev: (0, 0),
                base_addr: DEFAULT_BASE_512K,
            }
        }

        /// Defaults for a 256 KiB image: base `0x00FC0000`, rev 0.0.
        pub fn new_256k() -> Self {
            RomFixtureParams {
                size: ROM_SIZE_256K,
                rom_rev: (0, 0),
                exec_rev: (0, 0),
                base_addr: DEFAULT_BASE_256K,
            }
        }
    }

    fn put_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    /// Builds a minimal valid canonical Kickstart ROM image per
    /// `params`: header (§1) with magic reset (§5), zero filler, footer
    /// (§2) with a sealed checksum (§3) such that
    /// `checksum_ones_complement(&image) == 0xFFFFFFFF`.
    ///
    /// # Panics
    ///
    /// Panics if `params.size` is neither [`ROM_SIZE_256K`] nor
    /// [`ROM_SIZE_512K`].
    pub fn synthetic_rom(params: RomFixtureParams) -> Vec<u8> {
        assert!(
            matches!(params.size, ROM_SIZE_256K | ROM_SIZE_512K),
            "fixture size must be ROM_SIZE_256K or ROM_SIZE_512K"
        );
        let mut data = vec![0u8; params.size];

        // --- Header (§1), first 0x18 bytes ---
        let marker = if params.size == ROM_SIZE_512K {
            MARKER_512K
        } else {
            MARKER_256K
        };
        let base_addr = params.base_addr & 0xFFFF_0000;
        let boot_pc = base_addr + 0xD2;
        put_u16(&mut data, 0x00, marker);
        put_u16(&mut data, 0x02, JMP_OPCODE);
        put_u32(&mut data, 0x04, boot_pc);
        put_u32(&mut data, 0x08, HEADER_CONST_08);
        put_u16(&mut data, 0x0C, params.rom_rev.0);
        put_u16(&mut data, 0x0E, params.rom_rev.1);
        put_u16(&mut data, 0x10, params.exec_rev.0);
        put_u16(&mut data, 0x12, params.exec_rev.1);
        put_u32(&mut data, 0x14, HEADER_SEPARATOR);

        // --- Magic reset (§5): fixed absolute offset 0xD0 ---
        put_u16(&mut data, RESET_OFFSET, RESET_OPCODE);

        // --- Footer (§2), last 24 bytes ---
        let len = data.len();
        let checksum_off = len - 24;
        let size_field_off = len - 20;
        let vectors_off = len - 16;
        put_u32(&mut data, checksum_off, 0); // sealed below
        put_u32(&mut data, size_field_off, len as u32);
        for (i, &word) in FOOTER_VECTOR_WORDS.iter().enumerate() {
            put_u16(&mut data, vectors_off + i * 2, word);
        }

        // --- Checksum seal (§3), via the public sealer — no hand-rolled
        // arithmetic here, per PLAN.md's rule.
        seal_checksum(&mut data).expect("fixture image is always >= 24 bytes and 4-aligned");

        debug_assert_eq!(checksum_ones_complement(&data), 0xFFFF_FFFF);
        data
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{synthetic_rom, RomFixtureParams, DEFAULT_BASE_512K};
    use super::*;
    use alloc::vec;

    #[test]
    fn check_size_accepts_256k_and_512k() {
        let small = vec![0u8; ROM_SIZE_256K];
        let large = vec![0u8; ROM_SIZE_512K];
        let odd = vec![0u8; ROM_SIZE_256K + 1];
        assert!(KickRom::new(&small).check_size());
        assert!(KickRom::new(&large).check_size());
        assert!(!KickRom::new(&odd).check_size());
    }

    #[test]
    fn detect_recognizes_cloanto_magic() {
        let mut data = CLOANTO_MAGIC.to_vec();
        data.extend_from_slice(&[0u8; 16]);
        assert_eq!(Loader::detect(&data), Some(RomEncoding::CloantoEncoded));
    }

    /// The Cloanto magic takes priority regardless of what follows it —
    /// even bytes that would otherwise look like junk.
    #[test]
    fn detect_recognizes_cloanto_magic_regardless_of_payload() {
        let mut data = CLOANTO_MAGIC.to_vec();
        data.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(Loader::detect(&data), Some(RomEncoding::CloantoEncoded));
    }

    /// A canonical-order boot vector (Kickstart 2.04+ family: LW0
    /// `11144ef9`, LW1 a plausible `$00F8xxxx` boot PC).
    const NORMAL_HEADER: [u8; 8] = [0x11, 0x14, 0x4E, 0xF9, 0x00, 0xF8, 0x00, 0xD2];

    #[test]
    fn detect_recognizes_normal_order_header() {
        assert_eq!(
            Loader::detect(&NORMAL_HEADER),
            Some(RomEncoding::Raw(ByteOrder::Normal))
        );
    }

    #[test]
    fn detect_recognizes_every_permutation_of_a_valid_header() {
        for &order in PERMUTATIONS.iter() {
            let mut permuted = Vec::new();
            for chunk in NORMAL_HEADER.chunks_exact(4) {
                let word = [chunk[0], chunk[1], chunk[2], chunk[3]];
                permuted.extend_from_slice(&permute_word(word, order));
            }
            assert_eq!(
                Loader::detect(&permuted),
                Some(RomEncoding::Raw(order)),
                "order {order:?} round-trip failed"
            );
        }
    }

    #[test]
    fn detect_recognizes_all_four_signature_table_values() {
        // Second longwords chosen as plausible ROM-space addresses for
        // each family per docs/research/byte-order-facts.md.
        let headers: [[u8; 8]; 4] = [
            [0x11, 0x14, 0x4E, 0xF9, 0x00, 0xF8, 0x00, 0xD2], // Kickstart 2.04+
            [0x11, 0x11, 0x4E, 0xF9, 0x00, 0xFC, 0x00, 0xD2], // Kickstart 1.3
            [0x61, 0x2E, 0x44, 0x47, 0x00, 0xF8, 0x01, 0x90], // DiagROM 2.x Beta
            [0x11, 0x14, 0x44, 0x47, 0x00, 0xF8, 0x00, 0xD6], // DiagROM 2.x
        ];
        for header in headers.iter() {
            assert_eq!(
                Loader::detect(header),
                Some(RomEncoding::Raw(ByteOrder::Normal)),
                "header {header:02x?} not recognized"
            );
        }
    }

    #[test]
    fn detect_rejects_junk() {
        let junk = [0xAAu8; 16];
        assert_eq!(Loader::detect(&junk), None);
    }

    #[test]
    fn detect_rejects_valid_lw0_with_implausible_lw1() {
        let mut data = NORMAL_HEADER;
        // Replace LW1 with something that isn't a plausible ROM-space
        // address (high byte non-zero).
        data[4..8].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(Loader::detect(&data), None);
    }

    #[test]
    fn detect_rejects_short_input() {
        assert_eq!(Loader::detect(&[]), None);
        assert_eq!(Loader::detect(&[0x11, 0x14, 0x4E]), None);
        assert_eq!(Loader::detect(&NORMAL_HEADER[..7]), None);
    }

    #[test]
    fn permute_word_permutations_are_self_inverse() {
        let word = [0x12, 0x34, 0x56, 0x78];
        for &order in PERMUTATIONS.iter() {
            let once = permute_word(word, order);
            let twice = permute_word(once, order);
            assert_eq!(twice, word, "order {order:?} is not self-inverse");
        }
    }

    /// Synthetic 16-byte image (4 longwords) built from the canonical
    /// `NORMAL_HEADER` plus filler, used to exercise normalize's raw
    /// reordering leg without touching the `fixtures` module (owned by
    /// a parallel worker).
    fn synthetic_canonical_image() -> Vec<u8> {
        let mut data = NORMAL_HEADER.to_vec();
        data.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
        data
    }

    #[test]
    fn normalize_round_trips_every_permutation_of_a_synthetic_image() {
        let canonical = synthetic_canonical_image();
        for &order in PERMUTATIONS.iter() {
            let mut permuted = Vec::new();
            for chunk in canonical.chunks_exact(4) {
                let word = [chunk[0], chunk[1], chunk[2], chunk[3]];
                permuted.extend_from_slice(&permute_word(word, order));
            }
            let normalized = Loader::normalize(&permuted, None).unwrap();
            assert_eq!(
                normalized, canonical,
                "order {order:?} did not normalize back to canonical"
            );
        }
    }

    #[test]
    fn normalize_returns_unknown_format_for_junk() {
        let junk = [0xAAu8; 16];
        assert_eq!(
            Loader::normalize(&junk, None),
            Err(LoaderError::UnknownFormat)
        );
    }

    #[test]
    fn normalize_rejects_unaligned_swapped_length() {
        // A valid, swapped 8-byte header (Order1032) followed by 3
        // extra bytes: not a multiple of 4, so reordering can't proceed.
        let mut swapped = Vec::new();
        for chunk in NORMAL_HEADER.chunks_exact(4) {
            let word = [chunk[0], chunk[1], chunk[2], chunk[3]];
            swapped.extend_from_slice(&permute_word(word, ByteOrder::Order1032));
        }
        swapped.extend_from_slice(&[0x00, 0x00, 0x00]);
        assert_eq!(
            Loader::normalize(&swapped, None),
            Err(LoaderError::UnalignedLength)
        );
    }

    #[test]
    fn normalize_requires_nonempty_key_for_cloanto() {
        let mut data = CLOANTO_MAGIC.to_vec();
        data.extend_from_slice(&[0u8; 16]);
        assert_eq!(
            Loader::normalize(&data, Some(&[])),
            Err(LoaderError::InvalidKey)
        );
    }

    #[test]
    fn split_hi_lo_and_merge_hi_lo_round_trip() {
        let canonical = synthetic_canonical_image();
        let (hi, lo) = split_hi_lo(&canonical).unwrap();
        let merged = merge_hi_lo(&hi, &lo).unwrap();
        assert_eq!(merged, canonical);
    }

    #[test]
    fn split_hi_lo_rejects_non_multiple_of_four() {
        let data = vec![0u8; 6];
        assert_eq!(split_hi_lo(&data), Err(LoaderError::UnalignedLength));
    }

    #[test]
    fn merge_hi_lo_rejects_odd_length() {
        let hi = vec![0u8; 3];
        let lo = vec![0u8; 3];
        assert_eq!(merge_hi_lo(&hi, &lo), Err(LoaderError::OddHiLoLength));
    }

    /// Hand-written 8-byte example (two canonical longwords) verifying
    /// the interleave is positional: `b0 b1` of each longword goes to
    /// hi, `b2 b3` goes to lo — not a byte-split, not word-swapped.
    #[test]
    fn split_hi_lo_interleaves_positionally() {
        let rom: [u8; 8] = [0x11, 0x14, 0x4E, 0xF9, 0x00, 0xF8, 0x00, 0xD2];
        let (hi, lo) = split_hi_lo(&rom).unwrap();
        assert_eq!(hi, vec![0x11, 0x14, 0x00, 0xF8]);
        assert_eq!(lo, vec![0x4E, 0xF9, 0x00, 0xD2]);
    }

    #[test]
    fn normalize_decodes_cloanto_xor_payload() {
        let key = b"key";
        let plain: &[u8] = b"0123456789";
        let mut data = CLOANTO_MAGIC.to_vec();
        for (i, &b) in plain.iter().enumerate() {
            data.push(b ^ key[i % key.len()]);
        }
        let decoded = Loader::normalize(&data, Some(key)).unwrap();
        assert_eq!(decoded, plain);
    }

    #[test]
    fn normalize_requires_key_for_cloanto() {
        let mut data = CLOANTO_MAGIC.to_vec();
        data.extend_from_slice(&[0u8; 16]);
        assert_eq!(
            Loader::normalize(&data, None),
            Err(LoaderError::KeyRequired)
        );
    }

    #[test]
    fn checksum_ones_complement_folds_all_ones_to_all_ones() {
        // Two longwords that are each other's ones'-complement: their
        // ones'-complement sum is the all-ones pattern, matching a
        // "correct ROM" total.
        let mut data = Vec::new();
        data.extend_from_slice(&0x1234_5678u32.to_be_bytes());
        data.extend_from_slice(&(!0x1234_5678u32).to_be_bytes());
        assert_eq!(checksum_ones_complement(&data), 0xFFFF_FFFF);
    }

    #[test]
    fn checksum_ones_complement_handles_end_around_carry() {
        let mut data = Vec::new();
        data.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        data.extend_from_slice(&0x0000_0001u32.to_be_bytes());
        // 0xFFFFFFFF + 1 overflows a plain u32 add; end-around carry
        // folds it back to 1.
        assert_eq!(checksum_ones_complement(&data), 1);
    }

    #[test]
    fn merge_hi_lo_rejects_mismatched_lengths() {
        let hi = vec![0u8; 4];
        let lo = vec![0u8; 8];
        assert_eq!(
            merge_hi_lo(&hi, &lo),
            Err(LoaderError::MismatchedHiLoLength)
        );
    }

    #[test]
    fn synthetic_rom_seals_checksum_and_check_size_for_both_sizes() {
        let img_256 = synthetic_rom(RomFixtureParams::new_256k());
        let img_512 = synthetic_rom(RomFixtureParams::new_512k());

        assert_eq!(img_256.len(), ROM_SIZE_256K);
        assert_eq!(img_512.len(), ROM_SIZE_512K);

        assert_eq!(checksum_ones_complement(&img_256), 0xFFFF_FFFF);
        assert_eq!(checksum_ones_complement(&img_512), 0xFFFF_FFFF);

        assert!(KickRom::new(&img_256).check_size());
        assert!(KickRom::new(&img_512).check_size());
    }

    #[test]
    fn synthetic_rom_header_bytes_match_documented_offsets() {
        let img = synthetic_rom(RomFixtureParams::new_512k());

        // 0x00 marker word: 0x1114 for 512 KiB.
        assert_eq!(u16::from_be_bytes([img[0x00], img[0x01]]), 0x1114);
        // 0x02 JMP opcode.
        assert_eq!(u16::from_be_bytes([img[0x02], img[0x03]]), 0x4EF9);
        // 0x04 boot_pc == base_addr + 0xD2.
        let boot_pc = u32::from_be_bytes([img[0x04], img[0x05], img[0x06], img[0x07]]);
        assert_eq!(boot_pc, DEFAULT_BASE_512K + 0xD2);
        // 0x08 unresolved constant.
        let const_08 = u32::from_be_bytes([img[0x08], img[0x09], img[0x0A], img[0x0B]]);
        assert_eq!(const_08, 0x0000_FFFF);
        // 0x14 separator constant.
        let sep = u32::from_be_bytes([img[0x14], img[0x15], img[0x16], img[0x17]]);
        assert_eq!(sep, 0xFFFF_FFFF);
        // 0xD0 magic-reset opcode.
        assert_eq!(u16::from_be_bytes([img[0xD0], img[0xD1]]), 0x4E70);

        // Footer: size field at len-20 equals actual length; the
        // trailing seven vector-index words len-14..len-1 are exact.
        let len = img.len();
        let size_field =
            u32::from_be_bytes([img[len - 20], img[len - 19], img[len - 18], img[len - 17]]);
        assert_eq!(size_field, len as u32);
        let expected_tail: [u16; 7] = [0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F];
        for (i, &want) in expected_tail.iter().enumerate() {
            let off = len - 14 + i * 2;
            assert_eq!(u16::from_be_bytes([img[off], img[off + 1]]), want);
        }
    }

    #[test]
    fn synthetic_rom_rom_rev_and_exec_rev_land_at_documented_offsets() {
        let mut params = RomFixtureParams::new_512k();
        params.rom_rev = (40, 68);
        params.exec_rev = (37, 175);
        let img = synthetic_rom(params);

        assert_eq!(u16::from_be_bytes([img[0x0C], img[0x0D]]), 40);
        assert_eq!(u16::from_be_bytes([img[0x0E], img[0x0F]]), 68);
        assert_eq!(u16::from_be_bytes([img[0x10], img[0x11]]), 37);
        assert_eq!(u16::from_be_bytes([img[0x12], img[0x13]]), 175);
    }

    // --- Milestone 2: every check true on synthetic fixtures ---------

    fn all_checks_pass(rom: &KickRom) {
        assert!(rom.check_size(), "check_size");
        assert!(rom.check_header(), "check_header");
        assert!(rom.check_footer(), "check_footer");
        assert!(rom.check_size_field(), "check_size_field");
        assert!(rom.verify_check_sum(), "verify_check_sum");
        assert!(rom.check_magic_reset(), "check_magic_reset");
        assert!(rom.is_kick_rom(), "is_kick_rom");
    }

    #[test]
    fn synthetic_rom_passes_every_check_at_both_sizes() {
        let img_256 = synthetic_rom(RomFixtureParams::new_256k());
        let img_512 = synthetic_rom(RomFixtureParams::new_512k());
        all_checks_pass(&KickRom::new(&img_256));
        all_checks_pass(&KickRom::new(&img_512));
    }

    #[test]
    fn value_reads_match_fixture_params_at_both_sizes() {
        let mut params = RomFixtureParams::new_512k();
        params.rom_rev = (40, 68);
        params.exec_rev = (37, 175);
        let img = synthetic_rom(params);
        let rom = KickRom::new(&img);

        assert_eq!(
            rom.read_check_sum(),
            Some(u32::from_be_bytes([
                img[img.len() - 24],
                img[img.len() - 23],
                img[img.len() - 22],
                img[img.len() - 21],
            ]))
        );
        assert_eq!(rom.boot_pc(), Some(DEFAULT_BASE_512K + 0xD2));
        assert_eq!(rom.base_addr(), Some(DEFAULT_BASE_512K));
        assert_eq!(rom.rom_rev(), Some((40, 68)));
        assert_eq!(rom.exec_rev(), Some((37, 175)));
    }

    // --- Hostile inputs: no panics, checks false, values None --------

    fn assert_hostile_input_is_safe(data: &[u8]) {
        let rom = KickRom::new(data);
        assert!(!rom.check_header());
        assert!(!rom.check_footer());
        assert!(!rom.check_size_field());
        assert!(!rom.verify_check_sum());
        assert!(!rom.check_kickety_split());
        assert!(!rom.check_magic_reset());
        assert!(!rom.is_kick_rom());
        let _ = rom.read_check_sum();
        let _ = rom.base_addr();
        let _ = rom.boot_pc();
        let _ = rom.rom_rev();
        let _ = rom.exec_rev();
        let _ = rom.info();
    }

    #[test]
    fn hostile_inputs_never_panic_and_report_false_or_none() {
        assert_hostile_input_is_safe(&[]);
        assert_hostile_input_is_safe(&[0u8]);
        assert_hostile_input_is_safe(&[0u8; 23]);
        assert_hostile_input_is_safe(&[0u8; 0x17]); // just under header size
        assert_hostile_input_is_safe(&[0u8; 31]); // odd relative to footer, still short
        assert_hostile_input_is_safe(&[0u8; 0x19]); // odd length
        assert_hostile_input_is_safe(&vec![0u8; ROM_SIZE_256K - 1]); // truncated mid-footer/odd
        assert_hostile_input_is_safe(&[0u8; 0x18]); // exactly header size, no footer
    }

    #[test]
    fn value_reads_are_none_below_header_length_and_some_at_it() {
        let short = vec![0u8; 0x17];
        let rom = KickRom::new(&short);
        assert_eq!(rom.boot_pc(), None);
        assert_eq!(rom.base_addr(), None);
        assert_eq!(rom.rom_rev(), None);
        assert_eq!(rom.exec_rev(), None);

        let exact = vec![0u8; 0x18];
        let rom = KickRom::new(&exact);
        assert_eq!(rom.boot_pc(), Some(0));
        assert_eq!(rom.base_addr(), Some(0));
        assert_eq!(rom.rom_rev(), Some((0, 0)));
        assert_eq!(rom.exec_rev(), Some((0, 0)));
    }

    #[test]
    fn read_check_sum_is_none_below_24_bytes_and_some_at_it() {
        let short = vec![0u8; 23];
        assert_eq!(KickRom::new(&short).read_check_sum(), None);
        let exact = vec![0u8; 24];
        assert_eq!(KickRom::new(&exact).read_check_sum(), Some(0));
    }

    // --- Corrupting exactly one field flips exactly its check ---------

    #[test]
    fn corrupting_header_marker_flips_only_header_check() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        img[0x00] = 0xAB;
        img[0x01] = 0xCD;
        seal_checksum(&mut img).unwrap(); // re-seal so only header is broken
        let rom = KickRom::new(&img);
        assert!(!rom.check_header());
        assert!(rom.check_footer());
        assert!(rom.check_size_field());
        assert!(rom.verify_check_sum());
        assert!(!rom.is_kick_rom());
    }

    #[test]
    fn corrupting_footer_word_flips_only_footer_check() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        let len = img.len();
        // Last word (len-2..len), one of the seven checked words.
        img[len - 2] = 0xFF;
        img[len - 1] = 0xFF;
        seal_checksum(&mut img).unwrap();
        let rom = KickRom::new(&img);
        assert!(rom.check_header());
        assert!(!rom.check_footer());
        assert!(rom.check_size_field());
        assert!(rom.verify_check_sum());
        assert!(!rom.is_kick_rom());
    }

    #[test]
    fn corrupting_len_minus_16_word_does_not_flip_footer_check() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        let len = img.len();
        img[len - 16] = 0xFF;
        img[len - 15] = 0xC0;
        seal_checksum(&mut img).unwrap();
        let rom = KickRom::new(&img);
        assert!(rom.check_footer());
        assert!(rom.is_kick_rom());
    }

    #[test]
    fn corrupting_size_field_flips_only_size_field_check() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        let len = img.len();
        let off = len - 20;
        img[off..off + 4].copy_from_slice(&0x1234_5678u32.to_be_bytes());
        seal_checksum(&mut img).unwrap();
        let rom = KickRom::new(&img);
        assert!(rom.check_header());
        assert!(rom.check_footer());
        assert!(!rom.check_size_field());
        assert!(rom.verify_check_sum());
        assert!(!rom.is_kick_rom());
    }

    #[test]
    fn corrupting_any_content_byte_flips_checksum() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        // Flip a byte in the zero filler between header and footer,
        // without re-sealing: only the checksum should go bad.
        img[0x100] ^= 0xFF;
        let rom = KickRom::new(&img);
        assert!(rom.check_header());
        assert!(rom.check_footer());
        assert!(rom.check_size_field());
        assert!(!rom.verify_check_sum());
        assert!(!rom.is_kick_rom());
    }

    #[test]
    fn header_marker_0x1111_fails_at_512_kib() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        img[0x00] = 0x11;
        img[0x01] = 0x11;
        seal_checksum(&mut img).unwrap();
        assert!(!KickRom::new(&img).check_header());
    }

    // --- Kickety split ------------------------------------------------

    #[test]
    fn kickety_split_detected_with_0x1111_at_midpoint() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        let mid = img.len() / 2;
        img[mid] = 0x11;
        img[mid + 1] = 0x11;
        img[mid + 2] = 0x4E;
        img[mid + 3] = 0xF9;
        assert!(KickRom::new(&img).check_kickety_split());
    }

    #[test]
    fn kickety_split_not_detected_with_0x1114_at_midpoint() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        let mid = img.len() / 2;
        img[mid] = 0x11;
        img[mid + 1] = 0x14;
        img[mid + 2] = 0x4E;
        img[mid + 3] = 0xF9;
        assert!(!KickRom::new(&img).check_kickety_split());
    }

    // --- Magic reset ----------------------------------------------------

    #[test]
    fn magic_reset_flips_when_0xd0_is_cleared() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        assert!(KickRom::new(&img).check_magic_reset());
        img[0xD0] = 0;
        img[0xD1] = 0;
        seal_checksum(&mut img).unwrap();
        assert!(!KickRom::new(&img).check_magic_reset());
        // Not part of is_kick.
        assert!(KickRom::new(&img).is_kick_rom());
    }

    // --- seal_checksum --------------------------------------------------

    #[test]
    fn seal_checksum_restores_verify_check_sum_after_mutation() {
        let mut img = synthetic_rom(RomFixtureParams::new_512k());
        img[0x200] ^= 0xFF;
        assert!(!KickRom::new(&img).verify_check_sum());
        seal_checksum(&mut img).unwrap();
        assert!(KickRom::new(&img).verify_check_sum());
    }

    #[test]
    fn seal_checksum_rejects_too_short_and_unaligned() {
        let mut too_short = vec![0u8; 23];
        assert_eq!(seal_checksum(&mut too_short), Err(SealError::TooShort));

        let mut unaligned = vec![0u8; 25];
        assert_eq!(
            seal_checksum(&mut unaligned),
            Err(SealError::NotLongwordAligned)
        );
    }
}

#[cfg(test)]
mod scan_tests {
    use super::fixtures::{synthetic_rom, RomFixtureParams, DEFAULT_BASE_512K};
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Writes a 26-byte `struct Resident` into `img` at file offset
    /// `off`, with `rt_MatchTag` computed as the correct self-pointer
    /// (`base_addr + off`) so the result validates by default. Tests
    /// that want an invalid self-pointer overwrite it afterwards.
    #[allow(clippy::too_many_arguments)]
    fn write_resident(
        img: &mut [u8],
        off: usize,
        base_addr: u32,
        flags: u8,
        version: u8,
        node_type: u8,
        priority: i8,
        name_ptr: u32,
        id_ptr: u32,
        init_addr: u32,
        end_skip: u32,
    ) {
        let match_tag = base_addr.wrapping_add(off as u32);
        img[off..off + 2].copy_from_slice(&RTC_MATCHWORD.to_be_bytes());
        img[off + 2..off + 6].copy_from_slice(&match_tag.to_be_bytes());
        img[off + 6..off + 10].copy_from_slice(&end_skip.to_be_bytes());
        img[off + 10] = flags;
        img[off + 11] = version;
        img[off + 12] = node_type;
        img[off + 13] = priority as u8;
        img[off + 14..off + 18].copy_from_slice(&name_ptr.to_be_bytes());
        img[off + 18..off + 22].copy_from_slice(&id_ptr.to_be_bytes());
        img[off + 22..off + 26].copy_from_slice(&init_addr.to_be_bytes());
    }

    /// Writes a NUL-terminated byte string into `img` at file offset
    /// `off`.
    fn write_cstr(img: &mut [u8], off: usize, s: &[u8]) {
        img[off..off + s.len()].copy_from_slice(s);
        img[off + s.len()] = 0;
    }

    /// A base image large enough to hold residents and their strings
    /// well clear of the header/footer, with a known `base_addr`.
    fn base_image() -> Vec<u8> {
        synthetic_rom(RomFixtureParams::new_512k())
    }

    #[test]
    fn finds_one_valid_resident_with_correct_fields() {
        let mut img = base_image();
        let base = DEFAULT_BASE_512K;
        let name_off = 0x500;
        let id_off = 0x520;
        write_cstr(&mut img, name_off, b"exec.library");
        write_cstr(&mut img, id_off, b"exec 45.10");
        let resident_off = 0x300;
        write_resident(
            &mut img,
            resident_off,
            base,
            0x80, // RTF_AUTOINIT
            45,
            3, // NT_LIBRARY
            126,
            base + name_off as u32,
            base + id_off as u32,
            base + 0x600,
            0xDEAD_BEEF, // garbage end_skip, must be reported but not followed
        );

        let rom = KickRom::new(&img);
        let hits: Vec<Resident> = rom.scan().collect();
        assert_eq!(hits.len(), 1, "expected exactly one resident hit");
        let hit = &hits[0];
        assert_eq!(hit.match_word, RTC_MATCHWORD);
        assert_eq!(hit.offset, resident_off);
        assert_eq!(hit.flags, 0x80);
        assert_eq!(hit.version, 45);
        assert_eq!(hit.node_type, 3);
        assert_eq!(hit.priority, 126);
        assert_eq!(hit.name, b"exec.library");
        assert_eq!(hit.id_string, b"exec 45.10");
        assert_eq!(hit.init_addr, base + 0x600);
        assert_eq!(hit.end_skip, 0xDEAD_BEEF);
    }

    #[test]
    fn wrong_self_pointer_yields_nothing() {
        let mut img = base_image();
        let base = DEFAULT_BASE_512K;
        let off = 0x300;
        write_resident(&mut img, off, base, 0, 0, 0, 0, 0, 0, 0, 0);
        // Corrupt the self-pointer after the fact.
        img[off + 2..off + 6].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());

        let rom = KickRom::new(&img);
        assert_eq!(rom.scan().count(), 0);
    }

    #[test]
    fn truncated_matchword_at_end_of_image_yields_nothing_no_panic() {
        let base = DEFAULT_BASE_512K;
        // Image ends exactly at the matchword, with no room for the
        // rest of the struct.
        let mut img = vec![0u8; 0x18 + 2];
        // Valid header so base_addr() is Some.
        {
            let full = synthetic_rom(RomFixtureParams {
                size: ROM_SIZE_256K,
                rom_rev: (0, 0),
                exec_rev: (0, 0),
                base_addr: base,
            });
            img[..0x18].copy_from_slice(&full[..0x18]);
        }
        let last = img.len() - 2;
        img[last..].copy_from_slice(&RTC_MATCHWORD.to_be_bytes());

        let rom = KickRom::new(&img);
        assert_eq!(rom.scan().count(), 0);
    }

    #[test]
    fn two_residents_are_both_found() {
        let mut img = base_image();
        let base = DEFAULT_BASE_512K;
        write_cstr(&mut img, 0x500, b"exec.library");
        write_cstr(&mut img, 0x520, b"graphics.library");
        write_resident(
            &mut img,
            0x300,
            base,
            0,
            1,
            3,
            0,
            base + 0x500,
            base + 0x500,
            0,
            0,
        );
        write_resident(
            &mut img,
            0x340,
            base,
            0,
            2,
            3,
            0,
            base + 0x520,
            base + 0x520,
            0,
            0,
        );

        let rom = KickRom::new(&img);
        let hits: Vec<Resident> = rom.scan().collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].offset, 0x300);
        assert_eq!(hits[0].version, 1);
        assert_eq!(hits[1].offset, 0x340);
        assert_eq!(hits[1].version, 2);
        assert_eq!(hits[0].name, b"exec.library");
        assert_eq!(hits[1].name, b"graphics.library");
    }

    #[test]
    fn out_of_bounds_name_pointer_yields_empty_slice_not_panic() {
        let mut img = base_image();
        let base = DEFAULT_BASE_512K;
        let off = 0x300;
        write_resident(
            &mut img,
            off,
            base,
            0,
            0,
            0,
            0,
            0xFFFF_FFFF, // wildly out of bounds
            base + 0x500,
            0,
            0,
        );
        write_cstr(&mut img, 0x500, b"id.only");

        let rom = KickRom::new(&img);
        let hits: Vec<Resident> = rom.scan().collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, &[] as &[u8]);
        assert_eq!(hits[0].id_string, b"id.only");
    }

    #[test]
    fn no_nul_within_cap_yields_empty_slice() {
        let mut img = base_image();
        let base = DEFAULT_BASE_512K;
        let off = 0x300;
        let str_off = 0x500;
        // Fill far more than the search cap with non-NUL bytes.
        for b in img[str_off..str_off + 1024].iter_mut() {
            *b = b'A';
        }
        write_resident(
            &mut img,
            off,
            base,
            0,
            0,
            0,
            0,
            base + str_off as u32,
            0,
            0,
            0,
        );

        let rom = KickRom::new(&img);
        let hits: Vec<Resident> = rom.scan().collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, &[] as &[u8]);
    }

    #[test]
    fn zero_length_and_tiny_images_yield_empty_iterator_no_panic() {
        assert_eq!(KickRom::new(&[]).scan().count(), 0);
        assert_eq!(KickRom::new(&[0u8]).scan().count(), 0);
        assert_eq!(KickRom::new(&[0u8; 23]).scan().count(), 0);
        let matchword_only: [u8; 2] = RTC_MATCHWORD.to_be_bytes();
        assert_eq!(KickRom::new(&matchword_only).scan().count(), 0);
    }

    #[test]
    fn garbage_end_skip_does_not_affect_scan_control_flow() {
        // Two residents close enough together that a "trust rt_EndSkip"
        // implementation with a bogus/huge EndSkip on the first would
        // jump over (and thus miss) the second.
        let mut img = base_image();
        let base = DEFAULT_BASE_512K;
        write_resident(
            &mut img,
            0x300,
            base,
            0,
            1,
            3,
            0,
            0,
            0,
            0,
            0xFFFF_FFFF, // garbage/huge
        );
        write_resident(&mut img, 0x330, base, 0, 2, 3, 0, 0, 0, 0, 0);

        let rom = KickRom::new(&img);
        let hits: Vec<Resident> = rom.scan().collect();
        assert_eq!(
            hits.len(),
            2,
            "EndSkip must not be trusted for scan control flow"
        );
        assert_eq!(hits[0].version, 1);
        assert_eq!(hits[0].end_skip, 0xFFFF_FFFF);
        assert_eq!(hits[1].version, 2);
    }

    #[test]
    fn scan_is_empty_when_base_addr_is_unknown() {
        // Shorter than 0x18 bytes: base_addr() is None, so even a
        // structurally-valid-looking matchword scan yields nothing.
        let short = vec![0u8; 10];
        assert_eq!(KickRom::new(&short).scan().count(), 0);
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn empty_modules_yields_empty_vec() {
        let rom = vec![0u8; 16];
        assert_eq!(split(&rom, &[]), Ok(Vec::new()));
    }

    #[test]
    fn one_valid_module_in_bounds() {
        let rom: Vec<u8> = (0..16u8).collect();
        let specs = [ModuleSpec {
            name: "mod0",
            offset: 4,
            length: 4,
        }];
        let modules = split(&rom, &specs).unwrap();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "mod0");
        assert_eq!(modules[0].data, &rom[4..8]);
    }

    #[test]
    fn multiple_modules_including_adjacent_and_overlapping() {
        let rom: Vec<u8> = (0..32u8).collect();
        let specs = [
            ModuleSpec {
                name: "a",
                offset: 0,
                length: 8,
            },
            // Adjacent to "a".
            ModuleSpec {
                name: "b",
                offset: 8,
                length: 8,
            },
            // Deliberately overlaps "b" — overlap detection is out of
            // scope per PLAN.md, so this must succeed, not error.
            ModuleSpec {
                name: "c",
                offset: 12,
                length: 8,
            },
        ];
        let modules = split(&rom, &specs).unwrap();
        assert_eq!(modules.len(), 3);
        assert_eq!(modules[0].name, "a");
        assert_eq!(modules[0].data, &rom[0..8]);
        assert_eq!(modules[1].name, "b");
        assert_eq!(modules[1].data, &rom[8..16]);
        assert_eq!(modules[2].name, "c");
        assert_eq!(modules[2].data, &rom[12..20]);
    }

    #[test]
    fn out_of_bounds_module_fails_fast_with_no_partial_success() {
        let rom = vec![0u8; 16];
        let specs = [
            ModuleSpec {
                name: "ok",
                offset: 0,
                length: 4,
            },
            ModuleSpec {
                name: "too-long",
                offset: 10,
                length: 100,
            },
            ModuleSpec {
                name: "never-reached",
                offset: 0,
                length: 1,
            },
        ];
        // Fail-fast: the whole call is Err, full stop — no Vec with the
        // valid "ok" module is returned, and "never-reached" is simply
        // never inspected (its own out-of-bounds-ness, if any, wouldn't
        // matter here since index 1 already fails first).
        assert_eq!(
            split(&rom, &specs),
            Err(SplitError::ModuleOutOfBounds {
                index: 1,
                offset: 10,
                length: 100,
                rom_len: 16,
            })
        );
    }

    #[test]
    fn zero_length_module_succeeds_with_empty_slice() {
        let rom = vec![0u8; 16];
        let specs = [ModuleSpec {
            name: "empty",
            offset: 8,
            length: 0,
        }];
        let modules = split(&rom, &specs).unwrap();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].data, &[] as &[u8]);
    }

    #[test]
    fn offset_length_overflow_errors_instead_of_panicking() {
        let rom = vec![0u8; 16];
        let specs = [ModuleSpec {
            name: "overflow",
            offset: usize::MAX,
            length: 1,
        }];
        assert_eq!(
            split(&rom, &specs),
            Err(SplitError::ModuleOutOfBounds {
                index: 0,
                offset: usize::MAX,
                length: 1,
                rom_len: 16,
            })
        );
    }

    #[test]
    fn zero_length_rom_with_zero_length_specs_succeeds() {
        let rom: Vec<u8> = vec![];
        let specs = [
            ModuleSpec {
                name: "a",
                offset: 0,
                length: 0,
            },
            ModuleSpec {
                name: "b",
                offset: 0,
                length: 0,
            },
        ];
        let modules = split(&rom, &specs).unwrap();
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].data, &[] as &[u8]);
        assert_eq!(modules[1].data, &[] as &[u8]);
    }
}
