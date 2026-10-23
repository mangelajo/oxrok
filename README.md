# oxrok

A sigrok compatible logic analyzer stack in Rust: a library, drivers and
a command line tool. No C libraries, no Python runtime, one `cargo build`.

oxrok is not part of the sigrok project. It keeps sigrok's file formats,
command line flags and decoder ids so existing tools, scripts and habits
carry over.

## How it was written

oxrok is, so far, a translation of the sigrok C and Python code into
Rust done by a large language model (Claude), with a human directing
the work: choosing what to port, reviewing the structure and the
protocol handling, wiring up the bench hardware and running the
comparisons. Every driver and format is checked against sigrok-cli's
output for the same command, byte for byte after the version line,
either on real hardware or on a microcontroller that emulates the
device. Treat code that has only seen the emulator or a recording as
untested until the table below says otherwise, and read it with that
in mind.

## Status

Early. The crate builds on Linux, macOS and Windows with a sigrok-cli
style command line, the `bits`, `hex`, `ascii`, `analog`, `csv`, `vcd`,
`binary` and `srzip` outputs, `.sr`, VCD and binary inputs, USB, serial,
HID and SCPI transports with record and replay, eight driver families
and native `uart`, `i2c` and `spi` decoders that print what
libsigrokdecode prints. With the `python` feature the unchanged
libsigrokdecode decoders run too. See `docs/ROADMAP.md`.

### Hardware validated so far

| driver | checked on | how |
|---|---|---|
| `intronix-logicport` | two LogicPort LA1034 units | 60 bench checks with an ESP32 signal generator, captures identical to sigrok-cli |
| `fx2lafw` | a Saleae Logic clone (`0925:3881`) | firmware upload, captures identical to sigrok-cli |
| `rigol-ds` | a Rigol DS1102E | `--show`, frames and settings identical to sigrok-cli; also on a Pico emulating the scope over USBTMC |
| `raspberrypi-pico` | a Pico running the sigrok-pico firmware | digital, analog and mixed captures identical to sigrok-cli |
| `serial-dmm` (UT61E, UT60A) | meter streams replayed by an ESP32 | readings identical to sigrok-cli; no real meter yet |
| HID cables (BU-86X, CH9325, Victor), UT-D04 meters, UT32x | a Pico emulating the cable | readings identical to sigrok-cli; CP2110 unverified |
| `dcttech-usbrelay`, `microchip-pickit2` | a Pico emulating the device | identical to sigrok-cli; no real board yet |
| `uart`, `i2c`, `spi` decoders | captures from an ESP32 | annotations identical to libsigrokdecode |

The other 67 multimeters, the other Rigol and Agilent scope series and
the CP2110 cable are ports with replay tests only. `docs/DEVICES.md`
has the details per device and `hil/` the firmware used on the bench.

```sh
cargo install --path .                             # add --features python for the Python decoders
oxrok --scan
oxrok -d demo:logic_channels=4 -g Logic -c pattern=walking-one --samples 20 -O bits
oxrok -d demo --show
oxrok -d demo --samples 1k -o capture.sr      # a session file PulseView opens
oxrok -i capture.sr -O csv
oxrok -d intronix-logicport -c samplerate=100M -t D0=r --samples 2k -O vcd -o cap.vcd
oxrok -i cap.vcd -P uart:baudrate=115200:rx=D0 -A uart=rx-data
```

The output after the first header line matches sigrok-cli byte for byte
for the same command, which is how the test suite checks it.

## Layout

One crate, `oxrok`, with the library and the binary. Modules:

| module | contents |
|---|---|
| `core` | config keys and values, packets, the `Driver` and `Device` traits, sessions, triggers, parsing helpers |
| `formats` | text outputs, the `.sr` session format, VCD, CSV and binary readers and writers |
| `usb`, `serial`, `hid`, `scpi` | transports, each with record and replay fixtures |
| `ftdi`, `ezusb` | FTDI vendor requests and FIFO reads; Cypress FX2 firmware upload |
| `dmm` | multimeter packet parsers |
| `drivers::*` | demo, logicport, fx2lafw, serial_dmm, rigol_ds, raspberrypi_pico, dcttech_usbrelay, microchip_pickit2 |
| `decode`, `decoders` | the decoder API and the native `uart`, `i2c`, `spi` decoders |
| `pydecode` | runs libsigrokdecode's Python decoders (feature `python`) |

Cargo features: `cli` (the binary, default), `nusb` (default) and
`libusb` USB backends, `python`. `hil/oxrok-hil` is the bench tool, not
published.

## Compatibility with sigrok

- Configuration keys use the sigrok ids (`samplerate`, `limit_samples`,
  `voltage_threshold`, ...), so `-c` options and `.sr` metadata mean the
  same thing. Drivers can add their own keys; front ends show them like
  the rest.
- The command line accepts the sigrok-cli flags for the common flows and
  prints the same output. Differences are listed in `docs/CLI.md` as they
  appear.
- The C sigrok stack is never linked. It is used only as a test oracle
  during development.

## Building

Stable Rust, 1.95 or newer.

```sh
cargo build --workspace
cargo nextest run --workspace     # or cargo test
cargo clippy --workspace --all-targets
cargo fmt --all --check
```

## License

GPL-3.0-or-later, the license of the sigrok code it ports. See
`docs/LICENSING.md`.
