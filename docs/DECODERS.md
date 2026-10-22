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

`uart` reproduces libsigrokdecode's annotation classes, rows, texts and
sample ranges; `crates/cli/tests/decode_cli.rs` compares its output
with sigrok-cli's byte for byte. It also keeps one upstream quirk that
selections depend on: stop bits are tagged with the parity-ok class.

Each native decoder's source names the `pd.py` it corresponds to and
that file's authors, like the rest of the code (`docs/LICENSING.md`),
and is written from the protocol and the decoder's documented output.

## Python decoders

The plan is to run unchanged `pd.py` files from an existing
libsigrokdecode installation through a host that provides the
`sigrokdecode` module on top of the same runtime. The interpreter spike
on 2026-10-22:

- PyO3 0.30 with the system Python 3.14 ran the unmodified `uart`
  decoder with a Rust `wait` behind it at 3 million samples per second
  (5.5 million samples, 850 thousand annotations, 1.8 s). Loading took
  2 ms. Everything the decoders import (`re`, `collections`, `math`,
  `struct`, `functools`, the `common` package) is there.
- RustPython 0.6.0 from crates.io does not build: its own sub-crates
  resolve to incompatible versions and pinning them hits a second
  conflict. It stays an option for a static binary later, behind the
  same host interface, when a consistent release exists.

So the host uses PyO3 behind a `python` cargo feature; the default
build has no Python dependency.

## Not yet

Stacked decoders (`-P uart,midi`), the Python host, `i2c` and `spi`,
and the conformance fixtures from real captures.
