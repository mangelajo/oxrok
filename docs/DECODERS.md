# Protocol decoders

`oxrok-decode` is the runtime, `oxrok-decoders` the native decoders.
The command line flags are sigrok-cli's: `-P uart:baudrate=9600:rx=D0`
runs a decoder, `-A uart=rx-data:rx-start` selects annotation classes
or rows, `-B uart=rx` writes a binary class to stdout, `-M` shows
metadata, `--protocol-decoder-samplenum` and
`--protocol-decoder-ann-class` add fields, and `-l 4` shows everything
with quotes and abbreviations as sigrok-cli does.

## The API

A decoder is a `Decoder` with a static `DecoderInfo`: id, names,
channels (required and optional), options with typed defaults and
allowed values, annotation classes and rows, binary classes. Its
`decode` loop asks a `Context` for samples with `wait`, which takes
groups of conditions (levels, edges, a sample count to skip) and
returns the pin levels at the first sample where a group holds, with
`matched` saying which. It emits with `put_ann`, `put_binary`,
`put_python` (items for stacked decoders) and `put_meta`.

This is libsigrokdecode's Python API with Rust types, so a Python
decoder maps onto it one to one and a native decoder reads like one.
`wait` follows the same rules: the first sample is examined in place,
edges compare with the previous sample (none before the first), `skip`
counts from the current sample.

## Running

A `Stack` runs each instance on its own thread fed by a bounded sample
stream; outputs arrive on a channel in emission order. The command line
creates the stack on the first logic packet, when the sample width is
known, feeds every logic packet and prints as output arrives. Instance
ids are `<decoder>-<n>`.

Unassigned decoder channels default to the logic channel of the same
index, as in libsigrokdecode; a channel name that does not exist is a
warning. Option values are parsed to the type of their default and
checked against the allowed values.

## Native decoders

`uart`, `i2c` and `spi` reproduce libsigrokdecode's annotation classes,
rows, texts, sample ranges, binary output and metadata;
`crates/cli/tests/decode_cli.rs` compares their output with sigrok-cli's
byte for byte on synthetic captures, and `tools/diffcli.sh` repeats the
comparison in every output mode. Two details to know: `uart` keeps an
upstream quirk that selections depend on (stop bits are tagged with the
parity-ok class), and the i2c decoder's short name is `I2C`, not the
upstream `I2C` with a superscript two, so the sources stay ASCII.

Each native decoder's source names the `pd.py` it corresponds to and
that file's authors, like the rest of the code (`docs/LICENSING.md`),
and is written from the protocol and the decoder's documented output.

## Python decoders

With the `python` cargo feature (`cargo install --path crates/cli
--features python`) oxrok runs the unchanged `pd.py` files of a
libsigrokdecode installation. The decoders are looked for in
`OXROK_DECODERS_DIR`, `SIGROKDECODE_DIR`, `~/.local/share/libsigrokdecode/decoders`
and the system directories. Native decoders take precedence over a
Python decoder of the same id.

`oxrok-pydecode` provides the `sigrokdecode` module the decoders
import: the `Decoder` base class with `wait`, `put`, `register` and
`has_channel`, the output constants and `SRD_CONF_SAMPLERATE`. Each
Python decoder is a runtime `Decoder`; its `decode` runs on the
instance's thread with the interpreter attached, `wait` releases the
interpreter while it blocks for samples, and `options`, `samplenum`
and `matched` are set on the instance as libsigrokdecode does. All 112
decoders of libsigrokdecode 0.5.3 load; `timing`, chains such as
`uart,midi`, `i2c,eeprom24xx`, `i2c,rtc8564`, `spi,sdcard_spi` and
`parallel` print what sigrok-cli prints.

Stacking is synchronous: a decoder's Python output is handed to the
decoder above before the lower one continues, so annotations of a chain
interleave as in libsigrokdecode. An exception in the upper decoder on
one item is logged and the next item is delivered anyway, which is
also libsigrokdecode's behaviour. Items pass between two Python
decoders as the original objects; between a native and a Python decoder
they are converted (lists, numbers, strings, bytes, None).

The interpreter choice came from the spike on 2026-10-22:

- PyO3 0.30 with the system Python 3.14 ran the unmodified `uart`
  decoder with a Rust `wait` behind it at 3 million samples per second
  (5.5 million samples, 850 thousand annotations, 1.8 s). Loading took
  2 ms. Everything the decoders import (`re`, `collections`, `math`,
  `struct`, `functools`, the `common` package) is there.
- RustPython 0.6.0 from crates.io does not build: its own sub-crates
  resolve to incompatible versions and pinning them hits a second
  conflict. It stays an option for a static binary later, behind the
  same host interface, when a consistent release exists.

The default build has no Python dependency.

## Fixtures

`crates/cli/tests/fixtures` holds the captures the conformance tests
run on, each with sigrok-cli's output next to it:

- `uart-hi.vcd`, `i2c-write-read.vcd`, `spi-mode0.vcd`: synthetic,
  generated by the scripts described in the test files, covering the
  frame formats precisely.
- `uart-esp32.sr`: the LogicPort at 10 MHz with compression on the
  ESP32's TX line, which carries the boot banner at 115200 baud when
  the serial port is opened. `spi-esp32.sr`: an SPI transfer from the
  generator's `SPI` command at 20 MHz, four lines.

A real I2C capture is still to be made: the generator's hardware I2C
pins have no pull-ups on the bench, and its sequence playback did not
reproduce a bus waveform; the synthetic one covers the decoder for now.

## Not yet

`--protocol-decoder-jsontrace`, and a Python host that needs no
interpreter on the machine.
