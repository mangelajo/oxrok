# Changelog

All notable changes to oxrok are listed here. The format follows Keep a
Changelog and the project uses semantic versioning. Until 1.0 a minor
release may change the library API.

## [Unreleased]

### Added

- Workspace with `oxrok-core`, `oxrok-formats`, `oxrok-driver-demo` and
  the `oxrok` command line tool; CI for Linux, macOS and Windows.
- Configuration model: the sigrok keys with their ids, names and value
  types, plus driver defined keys.
- Data feed packets, the `Driver` and `Device` traits with a per device
  acquisition thread, and a `Session` that fans packets out.
- Demo driver with the logic patterns `oxrok`, `random`, `incremental`,
  `walking-one`, `walking-zero`, `all-low`, `all-high`, `graycode` and the
  analog patterns `square`, `sine`, `triangle`, `sawtooth`, `random`,
  with amplitude, offset, averaging and frame limits.
- Outputs `bits`, `hex`, `ascii` and `analog`, byte compatible with
  sigrok-cli after the header line.
- Command line: `--scan`, `--show`, `-d`, `-c`, `-C`, `-g`, `-t` with
  several stages, `--samples`, `--time`, `--frames`, `--continuous`,
  `-O`, `-o`, `--get`, `--set`, `-L`, `-V`, `-l`.
- Session files (`.sr`): the `srzip` output writes them in one pass, and
  `-i file.sr` reads them, including analog channels. Files round trip
  with sigrok-cli in both directions.
- Outputs `vcd`, `csv` and `binary`, byte compatible with sigrok-cli.
- Inputs `vcd` (scalars, vectors, reals, scopes, `downsample`, `skip`,
  `compress`, `numchannels`, `samplerate_overwrite`) and `binary`;
  `-i FILE [-I module:options]` with format detection, `-C` on files,
  and `-i FILE --show` for a file's properties. `-I module --show` and
  `-O module --show` describe a module.
- `--strict`: warnings fail the run. File inputs stop at the first
  chunk that warned.
- `oxrok-usb`: the `Transport` trait with a nusb backend (default) and
  a libusb backend (`libusb` feature, `OXROK_USB_BACKEND` to choose at
  run time), device enumeration with `bus.address` matching, and a
  text fixture format with `Recorder` and `Replay` transports.
- `oxrok-ftdi`: FT232/FT245 vendor requests, baud rate encoding per
  AN_120, bit modes, pin and EEPROM reads, status byte stripping; a
  `probe` example that records a session. Verified on a LogicPort with
  both backends on macOS; the recording is a replay test.
- `oxrok-driver-logicport`: the Intronix LogicPort LA1034 with all its
  sample rates, state mode, compression, two trigger levels, frames and
  the threshold DAC. FPGA images load from the vendor's `.ccf` or from
  `.rbf` files found in the sigrok and oxrok firmware directories.
  Output matches the libsigrok driver byte for byte; two recorded
  sessions replay in CI.
- `OXROK_USB_RECORD=FILE` records any driver session; fixtures store
  large payloads as digests and tolerate repeated polls.
- `oxrok-hil`: the hardware bench, driven by the ESP32 signal generator.
- `docs/DRIVER_PORTING.md`, `docs/LOGICPORT.md`, `docs/DEVICES.md`.
- `Transport::bulk_in_stream`: queued bulk reads on both backends,
  recorded and replayed like single reads.
- `oxrok-ezusb`: FX2 firmware upload (raw or Intel HEX) and the wait
  for renumeration. `oxrok_core::firmware` holds the shared lookup of
  firmware files.
- `oxrok_core::SoftTrigger` and `PreTriggerBuffer`: host side triggers
  for streaming devices.
- `oxrok-driver-fx2lafw`: Saleae Logic clones, USBee, Cypress FX2 and
  sigrok FX2 LA boards, 8 or 16 channels, with the firmware upload and
  the software trigger. Replay tests over a written session; hardware
  checks pending.
