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
//! amitools' `romtool` accepts as `is_kick: ok`. The [`KickRom`] check/
//! value methods and the byte-order leg of [`Loader::detect`]/
//! [`Loader::normalize`] remain `todo!()` stubs until milestone 2
//! transcribes those facts into the public API — the API shape is
//! fixed, and nothing here reports a false "ok". What *is* implemented
//! — ROM size checks, Cloanto container detection + decode (the
//! `AMIROMTYPE1` framing), and the ones'-complement-fold checksum
//! primitive — is real.

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
    /// The byte-order leg of detection/normalization isn't implemented
    /// yet — see this crate's `PLAN.md` milestone-1 facts pass.
    NotYetImplemented,
    /// [`merge_hi_lo`] was given two images of different lengths.
    MismatchedHiLoLength,
}

impl fmt::Display for LoaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoaderError::KeyRequired => {
                write!(f, "Cloanto-encoded ROM: a rom.key is required to decode it")
            }
            LoaderError::NotYetImplemented => {
                write!(
                    f,
                    "not yet implemented — see PLAN.md's milestone-1 facts pass"
                )
            }
            LoaderError::MismatchedHiLoLength => {
                write!(f, "hi and lo EPROM images must be the same length")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LoaderError {}

/// Normalizes a raw ROM file's bytes into a canonical image.
pub struct Loader;

impl Loader {
    /// Inspects leading bytes against known boot-vector signatures under
    /// all four [`ByteOrder`] permutations (plus the Cloanto
    /// `AMIROMTYPE1` container magic), and classifies. Does not
    /// decode/reorder — see [`Loader::normalize`] for that.
    ///
    /// Cloanto detection is implemented (an 11-byte fixed magic,
    /// confirmed sufficient and reliable — see
    /// `docs/research/cloanto-hilo-facts.md`). Byte-order detection is
    /// not yet — the signature table is confirmed
    /// (`docs/research/byte-order-facts.md`) but lands with milestone 2.
    ///
    /// # Panics
    ///
    /// Panics with `todo!()` when `data` doesn't match the Cloanto
    /// magic, since the byte-order leg isn't implemented yet.
    pub fn detect(data: &[u8]) -> RomEncoding {
        if data.starts_with(CLOANTO_MAGIC) {
            return RomEncoding::CloantoEncoded;
        }
        todo!(
            "byte-order detection needs a verified known-boot-vector table; \
             see PLAN.md's milestone-1 facts pass"
        )
    }

    /// Normalizes `data` into a canonical raw image. `key` is required
    /// iff [`RomEncoding::CloantoEncoded`] is detected.
    ///
    /// # Panics
    ///
    /// Panics with `todo!()` on the [`ByteOrder`] leg — not yet
    /// implemented, see [`Loader::detect`].
    pub fn normalize(data: &[u8], key: Option<&[u8]>) -> Result<Vec<u8>, LoaderError> {
        match Self::detect(data) {
            RomEncoding::CloantoEncoded => {
                let key = key.ok_or(LoaderError::KeyRequired)?;
                decode_cloanto(data, key)
            }
            RomEncoding::Raw(ByteOrder::Normal) => Ok(data.to_vec()),
            RomEncoding::Raw(_) => Err(LoaderError::NotYetImplemented),
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
    debug_assert!(!key.is_empty(), "caller must supply a non-empty key");
    let payload = &data[CLOANTO_MAGIC.len()..];
    let mut out = Vec::with_capacity(payload.len());
    for (i, &b) in payload.iter().enumerate() {
        out.push(b ^ key[i % key.len()]);
    }
    Ok(out)
}

/// Merges two same-size hi/lo EPROM dumps into one canonical image.
///
/// # Panics
///
/// Panics with `todo!()` — the hi/lo interleave width (byte vs
/// 16-bit-word split) hasn't been confirmed against a real sample yet;
/// see `PLAN.md`'s loader section.
pub fn merge_hi_lo(hi: &[u8], lo: &[u8]) -> Result<Vec<u8>, LoaderError> {
    if hi.len() != lo.len() {
        return Err(LoaderError::MismatchedHiLoLength);
    }
    todo!("hi/lo interleave width not yet confirmed; see PLAN.md's loader section")
}

/// Inverse of [`merge_hi_lo`]: splits a canonical image into hi/lo EPROM
/// dumps.
///
/// # Panics
///
/// Panics with `todo!()` — see [`merge_hi_lo`].
pub fn split_hi_lo(_rom: &[u8]) -> (Vec<u8>, Vec<u8>) {
    todo!("hi/lo interleave width not yet confirmed; see PLAN.md's loader section")
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
    /// the image.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — the header layout isn't nailed
    /// down yet; see `PLAN.md`'s milestone-1 facts pass.
    pub fn check_header(&self) -> bool {
        todo!("Kickstart header layout not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// `true` iff a valid Kickstart ROM footer is found at the end of
    /// the image.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn check_footer(&self) -> bool {
        todo!("Kickstart footer layout not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// `true` iff the footer's size field matches the image's actual
    /// length.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn check_size_field(&self) -> bool {
        todo!("Kickstart footer layout not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// `true` iff the stored checksum matches
    /// [`checksum_ones_complement`]'s result (folding to `0xFFFFFFFF`).
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — the stored checksum word's offset
    /// isn't confirmed yet; see `PLAN.md`'s milestone-1 facts pass. The
    /// underlying arithmetic ([`checksum_ones_complement`]) is already
    /// implemented.
    pub fn verify_check_sum(&self) -> bool {
        todo!(
            "stored checksum word offset not yet confirmed; \
             see PLAN.md's milestone-1 facts pass"
        )
    }

    /// `true` iff the "kickety split" signature (an extra 256 KiB
    /// module, found in some 512 KiB ROMs) is present.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn check_kickety_split(&self) -> bool {
        todo!("kickety-split signature not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// `true` iff the magic reset opcode is present at its expected
    /// location.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn check_magic_reset(&self) -> bool {
        todo!("magic-reset opcode location not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// `true` iff the image passes every check that gates "is a
    /// Kickstart ROM": size, header, footer, size field, checksum.
    /// Kickety-split and magic-reset are informational and
    /// deliberately *not* part of this conjunction — confirmed by
    /// oracle (`docs/research/header-footer-facts.md` §4/§5).
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn is_kick_rom(&self) -> bool {
        self.check_size()
            && self.check_header()
            && self.check_footer()
            && self.check_size_field()
            && self.verify_check_sum()
    }

    /// Reads the stored checksum value from the footer.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn read_check_sum(&self) -> u32 {
        todo!(
            "stored checksum word offset not yet confirmed; \
             see PLAN.md's milestone-1 facts pass"
        )
    }

    /// The ROM's base address in the Amiga's memory map.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn base_addr(&self) -> u32 {
        todo!("ROM header layout not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// The boot program counter (the reset vector's target).
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn boot_pc(&self) -> u32 {
        todo!("ROM header layout not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// The ROM's (major, minor) revision.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    pub fn rom_rev(&self) -> (u16, u16) {
        todo!("ROM header layout not yet confirmed; see PLAN.md's milestone-1 facts pass")
    }

    /// The embedded Exec's (major, minor) revision.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` — see [`KickRom::check_header`].
    /// The source is settled: a fixed read at header offset 0x10, not
    /// resident-derived (proven by oracle,
    /// `docs/research/header-footer-facts.md` §6) — lands with the
    /// other milestone-2 offset reads.
    pub fn exec_rev(&self) -> (u16, u16) {
        todo!("header reads land with milestone 2; see PLAN.md")
    }

    /// Aggregates every check/value into one [`RomInfo`], matching
    /// `romtool info`'s field set.
    ///
    /// # Panics
    ///
    /// Always panics with `todo!()` until the checks above are
    /// implemented — see [`KickRom::check_header`].
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

/// The aggregate result of every [`KickRom`] check, matching `romtool
/// info`'s field set.
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
    pub check_sum: u32,
    pub base_addr: u32,
    pub boot_pc: u32,
    pub rom_rev: (u16, u16),
    pub exec_rev: (u16, u16),
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

        // --- Checksum seal (§3): end-around-carry sum of the whole
        // image, with the checksum field zeroed, complemented and
        // stored back. Routed through checksum_ones_complement per
        // PLAN.md's rule against hand-rolled arithmetic.
        let partial_sum = checksum_ones_complement(&data);
        let sealed = !partial_sum;
        put_u32(&mut data, checksum_off, sealed);

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
        assert_eq!(Loader::detect(&data), RomEncoding::CloantoEncoded);
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
}
