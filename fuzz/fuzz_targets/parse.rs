//! Fuzz every entry point that takes attacker-controlled bytes directly:
//! [`Loader::detect`]/[`Loader::normalize`], [`KickRom`]'s checks/`info()`,
//! [`merge_hi_lo`]/[`split_hi_lo`], [`seal_checksum`]/
//! [`verify_check_sum`], and [`split`].
//!
//! Every one of these is documented to never panic on any input — bounds
//! are checked, not assumed — so the only property under test is "no
//! panic (and no sanitizer report)". `Result`/`Option`/`bool` returns are
//! deliberately discarded throughout: an `Err`/`None`/`false` on hostile
//! input is a correct outcome, not a finding.
//!
//! # Input mapping
//!
//! The whole fuzz input is used as one raw byte buffer, reused several
//! ways:
//!
//! * `Loader::detect(data)` / `Loader::normalize(data, None)` — the raw
//!   bytes as a candidate ROM dump.
//! * `Loader::normalize(data, Some(key))` — same bytes, with a short
//!   fixed key so the Cloanto-decode leg (`decode_cloanto`) is exercised
//!   too, not just the byte-reorder leg.
//! * `KickRom::new(data).info()` — every check/value method, over
//!   whatever length libFuzzer hands in (mostly far short of a real
//!   256 KiB/512 KiB image, which is the point: these must stay
//!   panic-free on tiny/misaligned/truncated buffers, not just full-size
//!   ones).
//! * `check_kickety_split()` on its own, called again explicitly for
//!   clarity even though `info()` already reaches it.
//! * `KickRom::new(data).scan().collect()` — walks every `Resident`
//!   candidate `ResidentScan` finds, per milestone 3's hostile-input
//!   discipline item (matchword at the buffer's edge, self-pointers
//!   outside the image, unterminated name/id strings). Collecting into
//!   a `Vec` also proves the iterator terminates on adversarial input,
//!   not just that each step doesn't panic.
//! * `merge_hi_lo` — the input is split into two halves (front/back) and
//!   fed in as the hi/lo pair, so mismatched-length and odd-length
//!   inputs are reached as directly as split-in-half arithmetic allows.
//! * `split_hi_lo(data)` — the same bytes as one canonical image.
//! * `seal_checksum` on a mutable copy, followed by `verify_check_sum` —
//!   the one property that *is* asserted rather than discarded: sealing
//!   that reports `Ok` must make the checksum verify.
//! * `split(data, &modules)` — a handful of `ModuleSpec`s whose
//!   `offset`/`length` are derived deterministically from the input
//!   bytes themselves (see `module_specs_from_input` below), so absurd
//!   offsets, absurd lengths, and offset+length pairs that overflow
//!   `usize` are all actually exercised against `split`'s bounds-check,
//!   not just a fixed empty/trivial `modules` slice.
#![no_main]

use amiga_rom::{merge_hi_lo, seal_checksum, split, split_hi_lo, KickRom, Loader, ModuleSpec};
use libfuzzer_sys::fuzz_target;

/// Short, fixed Cloanto key — real `rom.key` files are a handful of
/// bytes, and a short cycling key stresses `decode_cloanto`'s modulo
/// indexing more than a long one would.
const FIXED_KEY: &[u8] = b"fuzzkey!";

/// Reads an 8-byte little-endian `usize` out of `data` starting at
/// `at`, or `0` if `data` is too short there — never panics.
fn usize_at(data: &[u8], at: usize) -> usize {
    let mut buf = [0u8; 8];
    if let Some(slice) = data.get(at..).and_then(|s| s.get(..s.len().min(8))) {
        buf[..slice.len()].copy_from_slice(slice);
    }
    u64::from_le_bytes(buf) as usize
}

/// Derives a few [`ModuleSpec`]s straight from the fuzz input's own
/// bytes, so `split`'s bounds-checking is exercised against offsets and
/// lengths that actually vary with the input (in-bounds, out-of-bounds,
/// zero-length, and `usize`-overflowing) rather than a fixed trivial
/// slice.
fn module_specs_from_input(data: &[u8]) -> [ModuleSpec<'static>; 4] {
    [
        // Likely in-bounds for small inputs, likely out-of-bounds for
        // larger/adversarial ones.
        ModuleSpec {
            name: "a",
            offset: usize_at(data, 0),
            length: usize_at(data, 8),
        },
        // Zero-length is always valid, whatever the offset.
        ModuleSpec {
            name: "b",
            offset: usize_at(data, 16),
            length: 0,
        },
        // Deliberately pushed toward usize::MAX so offset+length has a
        // real chance of overflowing in the checked-add path.
        ModuleSpec {
            name: "c",
            offset: usize::MAX - usize_at(data, 24).min(4),
            length: usize_at(data, 32),
        },
        // Whole-input range, valid whenever data is non-empty.
        ModuleSpec {
            name: "d",
            offset: 0,
            length: data.len(),
        },
    ]
}

fuzz_target!(|data: &[u8]| {
    // --- Loader::detect ------------------------------------------------
    let _ = Loader::detect(data);

    // --- Loader::normalize, no key --------------------------------------
    let _ = Loader::normalize(data, None);

    // --- Loader::normalize, short fixed key -----------------------------
    let _ = Loader::normalize(data, Some(FIXED_KEY));

    // --- KickRom: every check/value method via info() -------------------
    let rom = KickRom::new(data);
    let _ = rom.info();

    // --- check_kickety_split, explicitly -------------------------------
    let _ = rom.check_kickety_split();

    // --- scan: must terminate and never panic on adversarial input -----
    let _: Vec<_> = rom.scan().collect();

    // --- merge_hi_lo: split the input in half as a hi/lo pair -----------
    let mid = data.len() / 2;
    let (hi, lo) = data.split_at(mid);
    let _ = merge_hi_lo(hi, lo);

    // --- split_hi_lo -----------------------------------------------------
    let _ = split_hi_lo(data);

    // --- seal_checksum, then verify_check_sum must succeed --------------
    let mut sealed = data.to_vec();
    if seal_checksum(&mut sealed).is_ok() {
        assert!(
            KickRom::new(&sealed).verify_check_sum(),
            "seal_checksum reported Ok but verify_check_sum failed on the sealed image"
        );
    }

    // --- split: bounds-check derived-from-input module ranges ----------
    let specs = module_specs_from_input(data);
    let _ = split(data, &specs);
});
