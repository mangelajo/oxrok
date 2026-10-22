# oxrok

A sigrok compatible logic analyzer stack in Rust: a library, drivers and
a command line tool. No C libraries, no Python runtime, one `cargo build`.

oxrok is not part of the sigrok project. It keeps sigrok's file formats,
command line flags and decoder ids so existing tools, scripts and habits
carry over.

## Status

Early. The workspace builds on Linux, macOS and Windows with a demo
device, the `bits`, `hex`, `ascii` and `analog` outputs and a sigrok-cli
style command line. Hardware drivers (Intronix LogicPort, fx2lafw) and the
`.sr` session format come next; see `docs/ROADMAP.md`.

```sh
cargo install --path crates/cli
oxrok --scan
oxrok -d demo:logic_channels=4 -g Logic -c pattern=walking-one --samples 20 -O bits
oxrok -d demo --show
```

The output after the first header line matches sigrok-cli byte for byte
for the same command, which is how the test suite checks it.

## Layout

| Crate | Content |
|---|---|
| `oxrok-core` | config keys and values, packets, the `Driver` and `Device` traits, sessions, parsing helpers |
| `oxrok-formats` | text outputs; file formats later |
| `oxrok-driver-demo` | a logic and analog pattern generator with no hardware |
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
