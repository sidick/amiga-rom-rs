# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this crate is

`amiga-rom` is a pure-Rust, `no_std` + `alloc` library for Amiga Kickstart ROM images: normalizing raw dumps (byte-order variants, Cloanto/Amiga Forever encoding, hi/lo EPROM splits) and inspecting/validating a canonical image (`KickRom`). Zero dependencies. The `std` feature (default) adds only `std::error::Error` impls.

**This repository is the library only.** A CLI consumer lives in a separate repo/crate and depends on this one for all parsing/validation/building logic.

**Read PLAN.md before starting any non-trivial work.** It is the actual design document — a staged (facts → inspect → scan → build/patch) plan with every decision's reasoning attached, checkboxes ticked as items land, licensing constraints on data this crate deliberately does not bundle, and a rule that governs this whole project: *anything discovered missing gets added to PLAN.md first, so the plan stays the map.* Do not implement something PLAN.md doesn't mention without adding it there.

## Commands

```bash
# Full test suite (unit tests + doctest)
cargo test

# no_std + alloc build (the core promise — must always pass)
cargo build --no-default-features
cargo test --no-default-features --all-targets

# Lint / format / docs (all must be clean; CI enforces all of these)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

# MSRV check (1.63; install once with `rustup toolchain install 1.63.0 --profile minimal`)
cargo +1.63.0 test
```

## Architecture

### No I/O in this crate

Everything operates on `&[u8]` / `&mut [u8]`. Callers (the CLI, an emulator, a test) own file I/O. This mirrors `amiga-ffs-rs`/`amiga-rdb-rs`'s `BlockSource` separation, but simpler: a ROM image is one flat blob, not a block device, so there's no analogous trait — just borrowed slices.

### Two layers

1. **Loader** (`Loader::detect`/`normalize`, `merge_hi_lo`/`split_hi_lo`) — turns whatever bytes a user actually has (byte-swapped, Cloanto-encoded, split hi/lo EPROM pair) into a canonical raw image. Needs `alloc` because it owns the output buffer.

2. **KickRom** — read-only inspection/validation of a canonical image, allocation-free, borrowing the caller's `&[u8]`. `KickRom::info()` aggregates every check into `RomInfo`, matching `romtool info`'s field set.

### GPL-adjacent sources: independent implementation only

This format is documented independently (Hardware Reference Manual, well-known community write-ups) precisely so this crate never needs to read amitools' (GPL-3) `KickRomAccess.py`/`RomImage.py`. Two genuinely-licensed cross-check sources are named in PLAN.md (kicksmash32, BSD-2-Clause; AmigaROMUtil, MIT) — use them to *verify* behavior, never as a source to port from. The Remus/Romsplit module-boundary catalog is explicitly all-rights-reserved (checked directly against the archives, see PLAN.md) — never bundle it; split/build load a pluggable, user-supplied catalog instead.

### Header/footer layout: unresolved, tracked in PLAN.md

`KickRom`'s check/value methods are currently `todo!()` stubs — the exact byte offsets (header, footer, "kickety split" signature, magic-reset opcode, stored-checksum location) haven't been nailed down against a primary source yet. PLAN.md's **milestone 1 — the facts pass** lists each missing fact with its acceptable sources; do that research (and record the answers in PLAN.md with citations) before implementing any of these stubs. The one piece already implementable independently — the ones'-complement-fold checksum algorithm — lives in `checksum_ones_complement`; route all checksum math through it.

### Test strategy: no ROM bytes in the repo, ever

Kickstart ROMs are copyrighted and never ship here, not even as fixtures. Tests use synthetic images built in code (milestone 1's fixture builder); real-ROM assertions are env-gated (`AMIGA_ROM_DIR`, local-only); the amitools differential oracle (`AMIGA_ROM_DIFFERENTIAL=1`, pinned version, CI-friendly) runs against synthetic images, so CI needs no ROMs at all. See PLAN.md's "ROM images themselves" section.
