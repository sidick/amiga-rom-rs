//! Real-ROM harness: asserts internal consistency (and, for a handful of
//! well-known checksums, exact known version numbers) against whatever
//! legally-owned Kickstart dumps the developer points it at.
//!
//! **Env-gated, silent no-op by default.** Every test checks
//! `AMIGA_ROM_DIR` first and returns immediately (no failure, no output)
//! when it isn't set to an existing directory — same shape as
//! `differential.rs`'s `AMIGA_ROM_DIFFERENTIAL` gate and `PLAN.md`'s
//! cross-cutting "Real-ROM harness" item. Local-only; CI never sets this
//! (it has no ROMs, nor may it, per `PLAN.md`'s "ROM images themselves"
//! section) so this file is always a pass. Run it explicitly:
//!
//! ```text
//! AMIGA_ROM_DIR=/path/to/roms cargo test --test real_roms
//! ```
//!
//! `AMIGA_ROM_DIR` should name a directory of `*.rom` files (scanned
//! non-recursively). Never prints ROM contents — only filenames and
//! version numbers, and only on failure.

#![cfg(feature = "std")]

use amiga_rom::{KickRom, Loader, RomEncoding};

/// checksum, (rom_rev major, rom_rev minor), (exec_rev major, exec_rev
/// minor).
type KnownRom = (u32, (u16, u16), (u16, u16));

/// (checksum, rom_rev, exec_rev) triples verified during this project's
/// research (`docs/research/header-footer-facts.md`) against real dumps
/// on this machine — publicly known version numbers, not reproduced ROM
/// content.
const KNOWN_ROMS: &[KnownRom] = &[
    // Kickstart 3.1, A3000, 512 KiB.
    (0x8F4C_0C67, (40, 68), (40, 10)),
    // Kickstart 2.04, A3000, 512 KiB.
    (0x5487_6DAB, (37, 175), (37, 132)),
    // Kickstart 1.3, A3000, 256 KiB.
    (0x150B_7DB3, (34, 5), (34, 2)),
];

fn rom_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("AMIGA_ROM_DIR")?;
    let path = std::path::PathBuf::from(dir);
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

fn rom_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("rom"))
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

struct Summary {
    passed: usize,
    skipped: usize,
    failures: Vec<String>,
}

#[test]
fn real_roms_are_internally_consistent() {
    let dir = match rom_dir() {
        Some(dir) => dir,
        None => return,
    };
    let files = rom_files(&dir);
    let key = std::env::var("AMIGA_ROM_KEY").ok();

    let mut summary = Summary {
        passed: 0,
        skipped: 0,
        failures: Vec::new(),
    };

    for path in &files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<unnamed>".to_string());

        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                summary.failures.push(format!("{name}: read error: {e}"));
                continue;
            }
        };

        let encoding = Loader::detect(&data);

        // Scope, per the task: `Raw(Normal)` files are checked directly.
        // Cloanto-encoded files are checked only when `AMIGA_ROM_KEY` is
        // set (then normalized with it first). Every other case
        // (non-Normal byte orders, unrecognized data — expansion ROMs,
        // diagnostic modules, etc., per the research doc's addendum) is
        // out of scope for this harness and silently skipped.
        let canonical: Vec<u8> = match encoding {
            Some(RomEncoding::Raw(amiga_rom::ByteOrder::Normal)) => data,
            Some(RomEncoding::CloantoEncoded) => {
                let key_str = match key.as_deref() {
                    Some(k) => k,
                    None => {
                        summary.skipped += 1;
                        continue;
                    }
                };
                match Loader::normalize(&data, Some(key_str.as_bytes())) {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        summary
                            .failures
                            .push(format!("{name}: Cloanto normalize error: {e}"));
                        continue;
                    }
                }
            }
            _ => {
                summary.skipped += 1;
                continue;
            }
        };

        let rom = KickRom::new(&canonical);
        let info = rom.info();

        // --- internal consistency -----------------------------------
        if info.is_kick {
            if !info.chk_sum_ok {
                summary
                    .failures
                    .push(format!("{name}: is_kick but chk_sum not ok"));
                continue;
            }
            if !info.size_field_ok {
                summary
                    .failures
                    .push(format!("{name}: is_kick but size_field not ok"));
                continue;
            }
        }

        if let Some(base_addr) = info.base_addr {
            if info.is_kick {
                let bytes = base_addr.to_be_bytes();
                let high_ok = bytes[0] == 0x00;
                let second_ok = (0xF0..=0xFF).contains(&bytes[1]);
                if !(high_ok && second_ok) {
                    summary.failures.push(format!(
                        "{name}: is_kick with implausible base_addr {base_addr:08X}"
                    ));
                    continue;
                }
            }
        }

        // --- known-checksum version table -----------------------------
        if let Some(checksum) = info.check_sum {
            if let Some(&(_, expected_rom_rev, expected_exec_rev)) =
                KNOWN_ROMS.iter().find(|(cs, _, _)| *cs == checksum)
            {
                if info.rom_rev != Some(expected_rom_rev)
                    || info.exec_rev != Some(expected_exec_rev)
                {
                    summary.failures.push(format!(
                        "{name}: known checksum {checksum:08X} expected rom_rev {expected_rom_rev:?} exec_rev {expected_exec_rev:?}, got rom_rev {:?} exec_rev {:?}",
                        info.rom_rev, info.exec_rev
                    ));
                    continue;
                }
            }
        }

        summary.passed += 1;
    }

    eprintln!(
        "real_roms_are_internally_consistent: {} files, {} passed, {} skipped, {} failed",
        files.len(),
        summary.passed,
        summary.skipped,
        summary.failures.len()
    );

    assert!(
        summary.failures.is_empty(),
        "real-ROM consistency failures:\n{}",
        summary.failures.join("\n")
    );
}
