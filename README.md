# amiga-rom

A pure-Rust, `no_std` + `alloc` library for Amiga Kickstart ROM images:
normalizing raw dumps (byte-order variants, Cloanto/Amiga Forever
encoding, hi/lo EPROM splits) and inspecting/validating a canonical image.

Independent implementation against the public Kickstart ROM header/footer
format — not a port of amitools' GPL-3 `romtool`. Every fact this crate
relies on (header/footer offsets, the checksum algorithm, the byte-order
signature table, the Cloanto container framing, the hi/lo interleave) is
confirmed and cited in [`docs/research/`](docs/research), and cross-checked
against amitools' `romtool` purely as a black-box oracle — never read as
source. See [PLAN.md](PLAN.md) for the full design rationale, including
why the Remus/Romsplit module-boundary catalog is deliberately not bundled.

This repository is the library only. A CLI consumer lives in a separate
repository and depends on this crate for all parsing/validation/building
logic.

Zero dependencies. MSRV 1.63. Licensed under MIT OR Apache-2.0.

## Status

Milestone 2 (`info` parity) is complete: normalizing a raw ROM dump
(byte-order detection/reordering, Cloanto decode, hi/lo split/merge) and
inspecting/validating a canonical image are both fully implemented,
bounds-checked against arbitrary input, and verified — against a
differential harness comparing field-for-field with `romtool`, a sweep of
real ROM dumps, and 20M+ fuzz executions with zero crashes. See
[PLAN.md](PLAN.md) for what's next: resident scanning (`scan`), then the
pluggable split/build catalog.

## Example

```rust
use amiga_rom::{KickRom, Loader};

// `data` is a raw ROM dump in whatever byte order/encoding it was found
// in — Loader figures out which and hands back a canonical image.
fn inspect(data: &[u8], cloanto_key: Option<&[u8]>) -> Option<String> {
    let canonical = Loader::normalize(data, cloanto_key).ok()?;
    let rom = KickRom::new(&canonical);
    let info = rom.info();

    Some(format!(
        "is_kick={} rom_rev={:?} exec_rev={:?}",
        info.is_kick, info.rom_rev, info.exec_rev
    ))
}
# fn make_synthetic_rom() -> Vec<u8> {
#     // Same shape as the confirmed facts in docs/research/header-footer-facts.md:
#     // header, magic-reset opcode, footer, sealed checksum.
#     let mut d = vec![0u8; amiga_rom::ROM_SIZE_512K];
#     d[0x00..0x02].copy_from_slice(&0x1114u16.to_be_bytes());
#     d[0x02..0x04].copy_from_slice(&0x4EF9u16.to_be_bytes());
#     d[0x04..0x08].copy_from_slice(&0x00F800D2u32.to_be_bytes());
#     d[0x08..0x0C].copy_from_slice(&0x0000FFFFu32.to_be_bytes());
#     d[0x0C..0x10].copy_from_slice(&[0, 40, 0, 10]);
#     d[0x10..0x14].copy_from_slice(&[0, 37, 0, 175]);
#     d[0x14..0x18].copy_from_slice(&0xFFFFFFFFu32.to_be_bytes());
#     d[0xD0..0xD2].copy_from_slice(&0x4E70u16.to_be_bytes());
#     let len = d.len();
#     d[len - 20..len - 16].copy_from_slice(&(len as u32).to_be_bytes());
#     let words: [u16; 7] = [0x0019, 0x001A, 0x001B, 0x001C, 0x001D, 0x001E, 0x001F];
#     for (i, w) in words.iter().enumerate() {
#         let off = len - 14 + i * 2;
#         d[off..off + 2].copy_from_slice(&w.to_be_bytes());
#     }
#     amiga_rom::seal_checksum(&mut d).unwrap();
#     d
# }
# let rom = make_synthetic_rom();
# assert_eq!(
#     inspect(&rom, None).unwrap(),
#     "is_kick=true rom_rev=Some((40, 10)) exec_rev=Some((37, 175))"
# );
```

No file I/O in this crate — callers pass `&[u8]`/`&mut [u8]` and own
reading/writing the bytes.
