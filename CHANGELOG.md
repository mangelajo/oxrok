# Changelog

All notable changes to oxrok are listed here. The format follows Keep a
Changelog and the project uses semantic versioning. Until 1.0 a minor
release may change the library API.

## [Unreleased]

### Changed

- The reference for byte compatibility is now sigrok-cli 0.8.0-git with
  libsigrok 0.6.0-git: the CSV time column starts at 0 and the VCD body
  has a blank line after the definitions and two spaces after each
  timestamp. `-I` and `-O --show` print options as 0.8 does.

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
- Every Rust source names its sigrok counterpart and that file's
  copyright holders in a header comment; see `docs/LICENSING.md`.
- `docs/DRIVER_PORTING.md` grew with the fx2lafw and parity lessons.
- `tools/diffcli.sh`: runs the documented flows through sigrok-cli and
  oxrok and diffs them, with `--hw` for the LogicPort and fx2lafw cases.
- The demo device honours triggers through the software trigger.
- `fx2lafw`: `probe_names` scan option and `limit_frames`.
- `oxrok-decode`: the protocol decoder runtime (decoder API with
  libsigrokdecode's `wait` semantics, instance threads, output
  routing) and `oxrok-decoders` with native `uart`, `i2c` and `spi`
  decoders. The CLI
  gains `-P`, `-A`, `-B`, `-M`, `--protocol-decoder-samplenum` and
  `--protocol-decoder-ann-class`, `-P dec --show` and the decoder
  section of `-L`. Their output is identical to sigrok-cli's in every
  mode on synthetic captures.
- Conformance fixtures from the bench: a UART and an SPI capture taken
  with the LogicPort from the ESP32, decoded identically to sigrok-cli.
- Stacked decoders (`-P i2c,eeprom24xx`), synchronous as in
  libsigrokdecode, and `oxrok-pydecode` behind the `python` feature of
  the CLI: libsigrokdecode's Python decoders run unchanged on the same
  runtime through PyO3. All 112 upstream decoders load; the checked
  ones print what sigrok-cli prints.
- `oxrok-driver-fx2lafw`: Saleae Logic clones, USBee, Cypress FX2 and
  sigrok FX2 LA boards, 8 or 16 channels, with the firmware upload and
  the software trigger. Verified on a Saleae Logic clone: firmware
  upload, 60 s at 24 MS/s without drops, `--show` identical to
  sigrok-cli. A written session and a recorded one replay as tests.
