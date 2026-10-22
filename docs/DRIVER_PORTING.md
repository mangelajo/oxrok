# Porting a driver

How a libsigrok hardware driver becomes an oxrok crate, from the first
port (`intronix-logicport`). Expect this to grow with the next ones.

## Where things go

- `crates/drivers/<name>`: one crate per driver, named
  `oxrok-driver-<name>`. The CLI registers it in `registry()` of
  `crates/cli/src/main.rs`.
- Transport code that other drivers share goes under `crates/usb`
  (`Transport`, record and replay), `crates/ftdi` or a new crate (the
  EZ-USB firmware upload comes with fx2lafw).
- Protocol notes go in `docs/<NAME>.md`, firmware and permissions in
  `docs/DEVICES.md`.

## License

Code written from your own protocol notes, or ported from a driver you
hold the copyright of, is MIT OR Apache-2.0 like the rest. Code ported
from libsigrok by other authors is GPL and goes in its own feature gated
crate; see `docs/LICENSING.md`. Do not copy comments or tables from GPL
sources into MIT crates.

## Shape of a driver

1. `Driver::scan` finds devices with `oxrok_usb::find_devices` and the
   `conn` scan option (`UsbDeviceInfo::matches_conn`). It returns closed
   devices; `Device::open` claims the hardware. Keep a
   `from_transport(Box<dyn Transport>)` constructor so a replayed
   recording can stand in for the USB device.
2. Settings live in a plain struct on the device. `options`, `get`,
   `set` and `list` map them to sigrok keys. Use the same keys and the
   same list types libsigrok uses, so `--show` and `-c` behave the same.
3. Put the wire protocol in a `protocol.rs` with a `Link` (or similar)
   owning the transport, and keep pure functions pure: register tables,
   divisor math, sample decoding. Those get unit tests with no hardware.
4. Acquisition runs on a thread the device spawns in `start`; it owns
   the link while running and gives it back when done. Send
   `Packet::Header` once the device is armed (a bench starts its
   stimulus when it sees it), then `Logic`, `Trigger`, `FrameBegin` and
   `FrameEnd` as applicable, and `End` last, also on errors. `stop` sets
   a flag the thread polls, then joins it.
5. Avoid time based loop exits where a count of reads does the same job
   (the LogicPort's FPGA init wait). Time based loops replay
   differently from the recording.

## Testing

- Unit tests for the pure parts.
- A recorded session per important path, replayed in CI:
  `OXROK_USB_RECORD=file oxrok -d <driver> ...` records; the test builds
  the device with `from_transport(Replay)` and runs the same settings
  through a `Session`. Keep the expected output next to the fixture.
  Large payloads are stored as digests, so a firmware upload is one
  line. See `docs/USB.md` for the format.
- An oracle diff while libsigrok still has the driver: the same capture
  through sigrok-cli and `oxrok`, compared after the header line. The
  `bits` and `csv:trigger=true` outputs cover sample values and the
  trigger position.
- Hardware in the loop checks in `crates/hil` when a stimulus source is
  available. They should run through the library, not by parsing CLI
  output, and print one PASS/FAIL line per check.

## Pitfalls met so far

- Protocols with sequence numbers in their replies cannot have polls
  invented by the replay; make the loops count reads.
- `--all-features` on a leaf crate does not enable a dependency's
  features. The CLI selects the USB backend; drivers depend on
  `oxrok-usb` without default features.
- The FTDI status bytes come with every USB packet, not every read;
  strip them per `wMaxPacketSize`.
- A bench that reads CSV sees values compacted to the enabled channels;
  one that reads packets sees all channels. Compact explicitly.
