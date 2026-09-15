//! Fuzz every entry point that takes attacker-controlled bytes directly:
//! [`Loader::detect`]/[`Loader::normalize`], [`KickRom`]'s checks/`info()`,
//! [`merge_hi_lo`]/[`split_hi_lo`], and [`seal_checksum`]/
//! [`verify_check_sum`].
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
//! * `merge_hi_lo` — the input is split into two halves (front/back) and
//!   fed in as the hi/lo pair, so mismatched-length and odd-length
//!   inputs are reached as directly as split-in-half arithmetic allows.
//! * `split_hi_lo(data)` — the same bytes as one canonical image.
//! * `seal_checksum` on a mutable copy, followed by `verify_check_sum` —
//!   the one property that *is* asserted rather than discarded: sealing
//!   that reports `Ok` must make the checksum verify.
#![no_main]

use amiga_rom::{merge_hi_lo, seal_checksum, split_hi_lo, KickRom, Loader};
use libfuzzer_sys::fuzz_target;

/// Short, fixed Cloanto key — real `rom.key` files are a handful of
/// bytes, and a short cycling key stresses `decode_cloanto`'s modulo
/// indexing more than a long one would.
const FIXED_KEY: &[u8] = b"fuzzkey!";

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
});
