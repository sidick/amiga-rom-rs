//! Differential oracle: `amiga_rom::KickRom::info()` vs. amitools'
//! `romtool info`, field for field, over synthetic fixtures built in this
//! file (never real ROM bytes — see `PLAN.md`'s "ROM images themselves"
//! section).
//!
//! **Env-gated, silent no-op by default.** Every test in this file
//! checks `AMIGA_ROM_DIFFERENTIAL=1` first and returns immediately
//! (without failing or printing anything) when it isn't set to exactly
//! `"1"` — the same shape as the sibling `amiga-rdb` crate's
//! `AMIGA_RDB_DIFFERENTIAL` harness. `cargo test` alone always passes
//! this file trivially. To actually run the oracle:
//!
//! ```text
//! AMIGA_ROM_DIFFERENTIAL=1 AMIGA_ROM_ROMTOOL=/path/to/romtool cargo test --test differential
//! ```
//!
//! `AMIGA_ROM_ROMTOOL` names the `romtool` binary to invoke; when unset,
//! plain `romtool` is looked up on `PATH`. amitools is GPL-3 — it is run
//! here purely as a black-box CLI oracle (spawned as a subprocess, its
//! stdout parsed as plain text); no amitools source is read or ported,
//! per `PLAN.md`'s source-discipline rule.
//!
//! One documented asymmetry (`PLAN.md`'s milestone-2 "ours-is-stricter"
//! caveat, `docs/research/header-footer-facts.md` §2): `romtool`'s
//! printed `size_field` status line is cosmetic and never goes `NOK`
//! under any mutation, while this crate's `check_size_field`/
//! `size_field_ok` honestly reports whether the stored field equals the
//! image's real length. So the `size_field` *status line* is compared
//! only when the fixture's field is actually correct (both agree `ok`);
//! when a test deliberately corrupts it, the assertion instead checks
//! the asymmetry itself: ours is `false`, romtool's line still reads
//! `ok`, and both agree `is_kick` goes `NOK`.

#![cfg(feature = "std")]

use amiga_rom::{seal_checksum, KickRom, Loader, RomInfo, ROM_SIZE_256K, ROM_SIZE_512K};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// --- gating -----------------------------------------------------------

fn differential_enabled() -> bool {
    std::env::var("AMIGA_ROM_DIFFERENTIAL")
        .map(|v| v == "1")
        .unwrap_or(false)
}

// --- local fixture builder ---------------------------------------------
//
// Deliberately a small, standalone copy of the shape of the crate's
// `#[cfg(test)]` `fixtures::synthetic_rom` (header/footer/magic-reset
// per `docs/research/header-footer-facts.md`) — that module is
// test-only and not visible to an integration test binary, so this is
// minimal test support, sealed via the crate's *public*
// [`amiga_rom::seal_checksum`] rather than any hand-rolled arithmetic.

const JMP_OPCODE: u16 = 0x4EF9;
const HEADER_CONST_08: u32 = 0x0000_FFFF;
const HEADER_SEPARATOR: u32 = 0xFFFF_FFFF;
const RESET_OPCODE: u16 = 0x4E70;
const RESET_OFFSET: usize = 0xD0;
const FOOTER_VECTOR_WORDS: [u16; 8] = [
    0x0018, 0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F,
];

const DEFAULT_BASE_256K: u32 = 0x00FC_0000;
const DEFAULT_BASE_512K: u32 = 0x00F8_0000;

#[derive(Debug, Clone, Copy)]
struct FixtureParams {
    size: usize,
    rom_rev: (u16, u16),
    exec_rev: (u16, u16),
    base_addr: u32,
}

impl FixtureParams {
    fn new_256k() -> Self {
        FixtureParams {
            size: ROM_SIZE_256K,
            rom_rev: (34, 5),
            exec_rev: (34, 2),
            base_addr: DEFAULT_BASE_256K,
        }
    }

    fn new_512k() -> Self {
        FixtureParams {
            size: ROM_SIZE_512K,
            rom_rev: (40, 68),
            exec_rev: (40, 10),
            base_addr: DEFAULT_BASE_512K,
        }
    }
}

fn put_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn put_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

/// Marker word at header offset 0x00 for `size` (§1's size-dependent
/// rule: `0x1111` only valid at 256 KiB, `0x1114` at any size).
fn marker_for_size(size: usize) -> u16 {
    if size == ROM_SIZE_512K {
        0x1114
    } else {
        0x1111
    }
}

/// Builds a minimal, valid, sealed synthetic Kickstart image: header
/// (§1) with magic reset (§5), zero filler, footer (§2), checksum sealed
/// via the crate's public `seal_checksum`.
fn build_fixture(params: FixtureParams) -> Vec<u8> {
    assert!(matches!(params.size, ROM_SIZE_256K | ROM_SIZE_512K));
    let mut data = vec![0u8; params.size];

    let marker = marker_for_size(params.size);
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

    put_u16(&mut data, RESET_OFFSET, RESET_OPCODE);

    let len = data.len();
    let checksum_off = len - 24;
    let size_field_off = len - 20;
    let vectors_off = len - 16;
    put_u32(&mut data, checksum_off, 0); // sealed below
    put_u32(&mut data, size_field_off, len as u32);
    for (i, &word) in FOOTER_VECTOR_WORDS.iter().enumerate() {
        put_u16(&mut data, vectors_off + i * 2, word);
    }

    seal_checksum(&mut data).expect("fixture is always >= 24 bytes and 4-aligned");
    data
}

// --- romtool subprocess plumbing ---------------------------------------

/// A temp file that removes itself when dropped, regardless of test
/// outcome (including panics during `#[test]` unwinding).
struct TempRomFile {
    path: PathBuf,
}

impl TempRomFile {
    fn write(name_hint: &str, bytes: &[u8]) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "amiga_rom_differential_{}_{}_{name_hint}.rom",
            std::process::id(),
            id
        ));
        std::fs::write(&path, bytes).expect("write temp ROM fixture for romtool");
        TempRomFile { path }
    }
}

impl Drop for TempRomFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A self-cleaning temp directory. Needed for the Cloanto case only:
/// this build of `romtool` (amitools 0.8.1) does not honor `-k
/// <path>`'s path for a `AMIROMTYPE1`-magic image — empirically (CLI
/// black-box observation, not source-read), it looks for a file
/// literally named `rom.key` beside the ROM image instead. So the key
/// must live at `<dir>/rom.key`, and the ROM at a unique path in that
/// same freshly-made directory, to avoid clashing with any other test
/// or a real `rom.key` a developer might have lying around.
struct TempRomDir {
    path: PathBuf,
}

impl TempRomDir {
    fn new(name_hint: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "amiga_rom_differential_dir_{}_{}_{name_hint}",
            std::process::id(),
            id
        ));
        std::fs::create_dir(&path).expect("create temp dir for Cloanto differential test");
        TempRomDir { path }
    }
}

impl Drop for TempRomDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn romtool_binary() -> String {
    std::env::var("AMIGA_ROM_ROMTOOL").unwrap_or_else(|_| "romtool".to_string())
}

/// Runs `romtool info <rom_path>` (optionally with `-k <key_path>`
/// before the subcommand) and parses its "field<whitespace>value" output
/// lines into a map.
fn run_romtool_info(rom_path: &Path, key_path: Option<&Path>) -> BTreeMap<String, String> {
    let romtool = romtool_binary();
    let mut cmd = std::process::Command::new(&romtool);
    if let Some(key) = key_path {
        cmd.arg("-k").arg(key);
    }
    cmd.arg("info").arg(rom_path);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to run `{romtool} info`: {e}"));
    assert!(
        output.status.success(),
        "romtool info failed (status {:?}): stdout={} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_romtool_info(&String::from_utf8_lossy(&output.stdout))
}

fn parse_romtool_info(stdout: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(idx) = line.find(char::is_whitespace) {
            let key = line[..idx].trim().to_string();
            let value = line[idx..].trim().to_string();
            fields.insert(key, value);
        }
    }
    fields
}

// --- field comparisons ---------------------------------------------------

fn ok_field(fields: &BTreeMap<String, String>, key: &str) -> bool {
    match fields
        .get(key)
        .unwrap_or_else(|| panic!("romtool output missing field {key:?}: {fields:?}"))
        .as_str()
    {
        "ok" => true,
        "NOK" => false,
        other => panic!("unexpected romtool value for {key:?}: {other:?}"),
    }
}

fn assert_ok_field_matches(fields: &BTreeMap<String, String>, key: &str, expected: bool) {
    let got = ok_field(fields, key);
    assert_eq!(
        got, expected,
        "field {key:?}: romtool={got} ours={expected}"
    );
}

fn hex_field(fields: &BTreeMap<String, String>, key: &str) -> u32 {
    let raw = fields
        .get(key)
        .unwrap_or_else(|| panic!("romtool output missing field {key:?}: {fields:?}"));
    u32::from_str_radix(raw, 16).unwrap_or_else(|e| panic!("bad hex for {key:?} ({raw:?}): {e}"))
}

fn rev_field(fields: &BTreeMap<String, String>, key: &str) -> (u16, u16) {
    let raw = fields
        .get(key)
        .unwrap_or_else(|| panic!("romtool output missing field {key:?}: {fields:?}"));
    let (major, minor) = raw
        .split_once('.')
        .unwrap_or_else(|| panic!("bad rev format for {key:?}: {raw:?}"));
    (
        major
            .parse()
            .unwrap_or_else(|e| panic!("bad major for {key:?} ({major:?}): {e}")),
        minor
            .parse()
            .unwrap_or_else(|e| panic!("bad minor for {key:?} ({minor:?}): {e}")),
    )
}

/// Compares every field *except* `size_field` (handled separately per
/// the documented asymmetry). romtool always prints the hex/rev value
/// lines for our fixture sizes, so the `Option`s are expected `Some`.
fn assert_common_fields_match(info: &RomInfo, fields: &BTreeMap<String, String>) {
    assert_ok_field_matches(fields, "size", info.size_ok);
    assert_ok_field_matches(fields, "header", info.header_ok);
    assert_ok_field_matches(fields, "footer", info.footer_ok);
    assert_ok_field_matches(fields, "chk_sum", info.chk_sum_ok);
    assert_ok_field_matches(fields, "kickety_split", info.kickety_split_ok);
    assert_ok_field_matches(fields, "magic_reset", info.magic_reset_ok);
    assert_ok_field_matches(fields, "is_kick", info.is_kick);
    assert_eq!(info.check_sum, Some(hex_field(fields, "check_sum")));
    assert_eq!(info.base_addr, Some(hex_field(fields, "base_addr")));
    assert_eq!(info.boot_pc, Some(hex_field(fields, "boot_pc")));
    assert_eq!(info.rom_rev, Some(rev_field(fields, "rom_rev")));
    assert_eq!(info.exec_rev, Some(rev_field(fields, "exec_rev")));
}

/// The fixture's stored size field is correct: both sides agree `ok`.
fn assert_size_field_agrees_when_correct(info: &RomInfo, fields: &BTreeMap<String, String>) {
    assert!(info.size_field_ok, "fixture's size field should be correct");
    assert_ok_field_matches(fields, "size_field", true);
}

/// The fixture's stored size field was deliberately corrupted: this is
/// the documented "ours-is-stricter" asymmetry, not a bug — assert it
/// directly instead of expecting equality.
fn assert_size_field_asymmetry_when_corrupted(info: &RomInfo, fields: &BTreeMap<String, String>) {
    assert!(
        !info.size_field_ok,
        "ours should report the corrupted size field as wrong"
    );
    assert_ok_field_matches(fields, "size_field", true); // cosmetic, always "ok"
    assert!(
        !info.is_kick,
        "ours: is_kick should be false due to the size-field mismatch"
    );
    assert_ok_field_matches(fields, "is_kick", false); // romtool still catches it here
}

fn info_and_romtool_fields(rom: &[u8], name_hint: &str) -> (RomInfo, BTreeMap<String, String>) {
    let info = KickRom::new(rom).info();
    let temp = TempRomFile::write(name_hint, rom);
    let fields = run_romtool_info(&temp.path, None);
    (info, fields)
}

// --- (a)/(b): valid fixtures, full field-for-field match ----------------

#[test]
fn valid_256k_fixture_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let rom = build_fixture(FixtureParams::new_256k());
    let (info, fields) = info_and_romtool_fields(&rom, "valid256");
    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
    assert!(info.is_kick);
}

#[test]
fn valid_512k_fixture_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let rom = build_fixture(FixtureParams::new_512k());
    let (info, fields) = info_and_romtool_fields(&rom, "valid512");
    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
    assert!(info.is_kick);
}

// --- (c): corrupted checksum ---------------------------------------------

#[test]
fn corrupted_checksum_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let mut rom = build_fixture(FixtureParams::new_512k());
    // Corrupt a filler byte without re-sealing: only the checksum breaks.
    rom[0x1000] ^= 0xFF;
    let (info, fields) = info_and_romtool_fields(&rom, "badchecksum");
    assert!(!info.chk_sum_ok);
    assert!(!info.is_kick);
    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
}

// --- (d): truncated/odd stored size field value --------------------------

#[test]
fn corrupted_size_field_matches_romtool_asymmetry() {
    if !differential_enabled() {
        return;
    }
    let mut rom = build_fixture(FixtureParams::new_512k());
    let len = rom.len();
    let off = len - 20;
    // An odd, truncated-looking stored value instead of the real length.
    put_u32(&mut rom, off, (len as u32) - 1);
    seal_checksum(&mut rom).unwrap();
    let (info, fields) = info_and_romtool_fields(&rom, "badsizefield");
    assert_common_fields_match(&info, &fields);
    assert_size_field_asymmetry_when_corrupted(&info, &fields);
}

// --- (e): missing footer words --------------------------------------------

#[test]
fn missing_footer_words_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let mut rom = build_fixture(FixtureParams::new_512k());
    let len = rom.len();
    // Zero out the last two checked trailing words (len-4..len).
    for b in &mut rom[len - 4..len] {
        *b = 0;
    }
    seal_checksum(&mut rom).unwrap();
    let (info, fields) = info_and_romtool_fields(&rom, "missingfooter");
    assert!(!info.footer_ok);
    assert!(!info.is_kick);
    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
}

// --- (f): wrong header marker (0x1111 at 512 KiB) -------------------------

#[test]
fn wrong_header_marker_at_512k_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let mut rom = build_fixture(FixtureParams::new_512k());
    put_u16(&mut rom, 0x00, 0x1111);
    seal_checksum(&mut rom).unwrap();
    let (info, fields) = info_and_romtool_fields(&rom, "wrongmarker512");
    assert!(!info.header_ok);
    assert!(!info.is_kick);
    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
}

// --- (g): kickety_split ok case (0x1111 + 0x4EF9 at midpoint) -------------

#[test]
fn kickety_split_ok_case_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let mut rom = build_fixture(FixtureParams::new_512k());
    let mid = rom.len() / 2;
    put_u16(&mut rom, mid, 0x1111);
    put_u16(&mut rom, mid + 2, JMP_OPCODE);
    seal_checksum(&mut rom).unwrap();
    let (info, fields) = info_and_romtool_fields(&rom, "kicketysplit");
    assert!(info.kickety_split_ok);
    // Not part of is_kick, per docs/research/header-footer-facts.md §4.
    assert!(info.is_kick);
    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
}

// --- (h): magic reset removed ---------------------------------------------

#[test]
fn magic_reset_removed_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let mut rom = build_fixture(FixtureParams::new_512k());
    put_u16(&mut rom, RESET_OFFSET, 0x0000);
    seal_checksum(&mut rom).unwrap();
    let (info, fields) = info_and_romtool_fields(&rom, "noreset");
    assert!(!info.magic_reset_ok);
    // Not part of is_kick, per docs/research/header-footer-facts.md §5.
    assert!(info.is_kick);
    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
}

// --- Cloanto container differential ---------------------------------------

/// Not a real Cloanto `rom.key` — a made-up key for this synthetic
/// round trip, same rule as the fixture bytes themselves (never real
/// Commodore/Amiga/Cloanto material). Length chosen to not evenly
/// divide the fixture size, so the cycling wraps mid-image.
const TEST_KEY: &[u8] = b"amiga-rom-differential-test-key-not-real-9137";

#[test]
fn cloanto_container_matches_romtool() {
    if !differential_enabled() {
        return;
    }
    let plain = build_fixture(FixtureParams::new_512k());

    let mut container = b"AMIROMTYPE1".to_vec();
    for (i, &b) in plain.iter().enumerate() {
        container.push(b ^ TEST_KEY[i % TEST_KEY.len()]);
    }

    let decoded =
        Loader::normalize(&container, Some(TEST_KEY)).expect("normalize Cloanto container");
    assert_eq!(decoded, plain);
    let info = KickRom::new(&decoded).info();

    // See `TempRomDir`'s doc comment: this romtool build resolves the
    // Cloanto key as a fixed `rom.key` filename beside the ROM image,
    // not via `-k <path>`'s value, so both files must live together in
    // one fresh directory.
    let dir = TempRomDir::new("cloanto");
    let rom_path = dir.path.join("container.rom");
    let key_path = dir.path.join("rom.key");
    std::fs::write(&rom_path, &container).expect("write Cloanto container fixture");
    std::fs::write(&key_path, TEST_KEY).expect("write rom.key fixture");
    let fields = run_romtool_info(&rom_path, Some(&key_path));

    assert_common_fields_match(&info, &fields);
    assert_size_field_agrees_when_correct(&info, &fields);
    assert!(info.is_kick);
}
