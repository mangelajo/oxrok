# Porting a driver

How a libsigrok hardware driver becomes an oxrok crate, from the first
two ports (`intronix-logicport`, a command and response device behind
an FTDI FIFO, and `fx2lafw`, a streaming device with a firmware
upload). Expect this to grow with the next ones.

## Before writing code

- Read the libsigrok driver for the facts: USB ids, request numbers,
  register maps, rate tables, the order settings are written in, the
  quirks its comments explain. Then write the Rust from those facts.
  The protocol summary you write first (`docs/<NAME>.md`) is also the
  clean room record of what was taken.
- Find the oracle: a sigrok-cli build that has the driver. Its version
  matters; libsigrok 0.5 and 0.6 print some formats differently, and
  oxrok follows 0.6 (`docs/CLI.md`).
- Find the firmware files the driver needs and where sigrok installs
  them; `oxrok_core::firmware` already searches those directories.
- Decide what stimulus the bench can give the device. Without one, the
  checks are limited to the device's own patterns and idle inputs.

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
6. Match libsigrok's option list, order included, and its default
   values: `--show` prints options in the driver's order, and the
   differential script diffs that output. Channel groups count too
   (fx2lafw has a `Logic` group with no options of its own).
7. Keep the sigrok ids and names of channels, and support
   `probe_names` where libsigrok does, through
   `oxrok_core::util::parse_probe_names`.

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

## Streaming devices

The fx2lafw driver is the model: open a `bulk_in_stream` sized for
10 ms of data per transfer and about 500 ms in flight, send the start
command, then loop on `next` until the sample limit or the stop flag.
`oxrok_core::SoftTrigger` matches a `Trigger` on the host, and
`PreTriggerBuffer` keeps the capture ratio's worth of samples before
it. Firmware uploads use `oxrok-ezusb` and `wait_for_renumeration`.

## Order of work that paid off

1. Transport first: enumerate, open, claim, run the pin-safe requests,
   record a fixture. For the LogicPort this was the FTDI `probe`
   example; for an FX2 it is the version request.
2. The pure parts with unit tests: tables, encodings, sample decoding.
3. A first capture through the CLI with `-l 4`, then the oracle diff.
4. The bench checks, ported from whatever suite existed for the C
   driver.
5. Record the fixtures last, once the sequence of calls is stable, and
   write the replay tests from the output printed during the recording.

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
- Right after a firmware upload and renumeration, macOS leaves the
  device unconfigured; claiming an interface fails with "interface not
  found". Call `set_configuration` first and retry for a second or so.
  libusb does this quietly, nusb does not.
- A device clone keeps the uploaded firmware until it is power cycled,
  so the upload path runs once per plug in. Verify it before anything
  else, and remember it is not re-run by later captures.
- Fixture tokens must not be ambiguous next to hex: `00*59` followed
  by `3160` read as a run of 593160. Runs are `00{59}` now.
- Streaming devices lose data in the gap between two blocking reads;
  use `bulk_in_stream`, which keeps transfers queued.
- A replay invents nothing: when the live run polled n times and the
  replay polls m times, the fixture layer repeats or skips polls only
  for reads without sequence numbers. Everything else must be
  deterministic in the number of calls.
- libsigrok has habits worth not copying: the demo ignores triggers,
  fx2lafw wraps a single capture in a frame and re-arms before
  stopping, the binary input ignores the size suffix. Do the sane
  thing, document the difference in `docs/CLI.md`, and exclude or
  filter the case in `tools/diffcli.sh`.
- The user's `-l 4` log is the first debugging tool: keep the debug
  messages at the protocol steps (resync, image probe, readout window,
  transfer plan), and filter library noise out of it.
- macOS after a firmware renumeration: the device's configuration is
  active and the descriptors list the interface, yet claiming it fails
  with "interface not found". libusb never shows this because it sets
  the configuration on every open; the nusb backend now does the same
  when a claim fails, then reopens the device. A device that uploads
  firmware needs a power cycle to test this path, not a replug of the
  cable alone if the board keeps power.
- A bench driven over a serial port: opening the port resets an ESP32
  (DTR), so state set in one process (pin maps, levels) is gone in the
  next. Keep one connection open across an experiment; the bench tool
  has `--delay` for that. The symptom was a capture of boot glitches
  and a sequence played on the wrong pins.
- Ad hoc comparison loops in zsh: an unquoted `$var` stays one word, so
  both tools get a mangled command, both print nothing, and the
  outputs compare equal. Count the oracle's lines in every comparison
  and treat zero as a failure; use `${=var}` or `sh`.
- Run the whole workspace's tests before committing, not the crate's
  alone; a changed listing or default breaks goldens elsewhere.

## Porting a decoder

The decoder runtime mirrors libsigrokdecode's Python API, so a native
decoder is written from the protocol and the `pd.py` is the oracle for
classes, rows, texts and sample ranges. What mattered for uart, i2c and
spi:

- Keep the class indices, rows and binary classes of the upstream
  decoder, including its quirks that selections depend on (uart tags
  stop bits with the parity-ok class).
- `wait` semantics: the first sample is examined in place, edges need a
  previous sample, `skip` counts from the current sample, unassigned
  channels read 0xff. Stacked decoders get their items synchronously,
  before the lower decoder continues, which fixes the output order.
- Compare at `-l 4` (sample ranges, class names, all texts) against
  sigrok-cli on a synthetic capture first, then on a bench capture.
- The Python host sets `options`, `samplenum` and `matched` on the
  instance, not as properties of the base class: decoders define class
  attributes of the same names (`options` is their option table) and
  an instance attribute is what shadows them.
- In a crate that imports `oxrok_core::Result`, write
  `std::result::Result::ok` in iterator chains; the alias changes the
  meaning of `Result::ok`.
