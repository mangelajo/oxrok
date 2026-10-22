# oxrok

A sigrok compatible logic analyzer stack in Rust: a library, drivers and
a command line tool. No C libraries, no Python runtime, one `cargo build`.

oxrok is not part of the sigrok project. It keeps sigrok's file formats,
command line flags and decoder ids so existing tools, scripts and habits
carry over.

## Status

Early. The workspace builds on Linux, macOS and Windows with a demo
device, a sigrok-cli style command line, the `bits`, `hex`, `ascii`,
`analog`, `csv`, `vcd`, `binary` and `srzip` outputs, and `.sr`, VCD and
binary inputs, a USB transport (nusb or libusb) with record and replay,
and two hardware drivers: the Intronix LogicPort LA1034, checked against
the libsigrok driver byte for byte and against a signal generator on the
bench, and fx2lafw for Saleae Logic clones and other FX2 boards, with
the firmware upload. Protocol decoding has started with a native `uart`
decoder whose output matches libsigrokdecode. See `docs/ROADMAP.md`.

```sh
cargo install --path crates/cli
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

| Crate | Content |
|---|---|
| `oxrok-core` | config keys and values, packets, the `Driver` and `Device` traits, sessions, parsing helpers |
| `oxrok-formats` | text outputs, the `.sr` session format, VCD, CSV and binary readers and writers |
| `oxrok-usb` | the `Transport` trait, nusb and libusb backends, record and replay fixtures |
| `oxrok-ftdi` | FT232/FT245 vendor requests and FIFO reads over a transport |
| `oxrok-driver-demo` | a logic and analog pattern generator with no hardware |
| `oxrok-ezusb` | Cypress FX2 firmware upload and renumeration |
| `oxrok-driver-logicport` | Intronix LogicPort LA1034, see `docs/LOGICPORT.md` and `docs/DEVICES.md` |
| `oxrok-driver-fx2lafw` | FX2 based analyzers running the sigrok fx2lafw firmware |
| `oxrok-decode` | the decoder API, the sample scanner behind `wait`, instance threads |
| `oxrok-decoders` | native decoders: `uart` so far |
| `oxrok-hil` | hardware in the loop checks, not published |
| `oxrok` | the command line tool |

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

Code written for oxrok is dual licensed under MIT or Apache-2.0, at your
option. Crates that port code from sigrok keep the GPL and say so in
their own LICENSE file; none exist yet. See `docs/LICENSING.md`.
