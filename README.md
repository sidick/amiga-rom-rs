# amiga-rom

A pure-Rust, `no_std` + `alloc` library for Amiga Kickstart ROM images:
normalizing raw dumps (byte-order variants, Cloanto/Amiga Forever
encoding, hi/lo EPROM splits) and inspecting/validating a canonical image.

Independent implementation against the public Kickstart ROM header/footer
format — not a port of amitools' GPL-3 `romtool`. See [PLAN.md](PLAN.md)
for the full design rationale, including why the Remus/Romsplit
module-boundary catalog is deliberately not bundled.

This repository is the library only. A CLI consumer lives in a separate
repository and depends on this crate for all parsing/validation/building
logic.

Zero dependencies. MSRV 1.63. Licensed under MIT OR Apache-2.0.

## Status

Early scaffolding — the loader and `KickRom` API surface exist, but the
exact Kickstart header/footer byte layout is still being nailed down from
primary sources (see PLAN.md's milestone 1, "the facts pass"). Not yet
usable.
