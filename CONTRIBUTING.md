# Contributing

## Ground rules

- Plain ASCII in code and docs. Terse technical English.
- `cargo fmt`, `cargo clippy --workspace --all-targets` with no warnings,
  `cargo nextest run --workspace` green. CI runs the same on Linux, macOS
  and Windows.
- Every change that a user can see gets a line in `CHANGELOG.md`.
- No `unsafe` outside the USB transport crates, and there only with a
  comment saying why it is sound.

## Licensing of contributions

New code is MIT OR Apache-2.0. Code ported from a GPL project (libsigrok,
libsigrokdecode, the sigrok decoders) goes into a crate of its own with
a GPL LICENSE file and a cargo feature, never into a permissive crate.
Protocol facts (register maps, command bytes, file layouts) are not
copyrightable; write them down in `docs/PROTOCOLS/` first and implement
from that document.

## Adding a driver

See `docs/DRIVER_PORTING.md` once it exists (written from the first two
ports). Until then: implement `Driver` and `Device` from `oxrok::core`,
keep the acquisition on the device's own thread, send packets through the
`PacketSink`, and add a replay fixture so the driver is tested without
the hardware.

## Compatibility with sigrok-cli

The outputs are compared with sigrok-cli byte for byte after the header
line. When adding an output or changing one, capture the reference with
sigrok-cli and put it in the test next to the command that produced it,
with the sigrok-cli and libsigrok versions.
