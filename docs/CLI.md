# The oxrok command line

`oxrok` accepts the sigrok-cli flags for the common flows and prints the
same output, byte for byte after the first header line (which names the
tool). The test suites carry reference output captured from sigrok-cli
0.7.2 with libsigrok 0.5.2.

## Differences from sigrok-cli

Deliberate, and kept small:

- The header line of the logic outputs says `oxrok <version>` instead of
  `libsigrok <version>`. Same for `$version` and `$scope` in VCD files,
  `sigrok version` in session metadata and the CSV comment header.
- `META samplerate: N` lines are printed only when the `analog` output
  is selected with `-O analog`. sigrok-cli prints them whenever a file
  input or a driver reports a sample rate, even into binary output.
- Analog packets go to the `analog` output only when the selected output
  cannot show them (`bits`, `hex`, `ascii`, `binary`). sigrok-cli also
  prints stray `A0: ...` lines next to `csv` and `vcd` output.
- The `binary` input parses `samplerate=200k` as 200 kHz. sigrok-cli
  reads it as 200 Hz, since it ignores the suffix for this option.
- The VCD input reads undefined bits (`x`, `z`, `u`, `-`) as 0 with one
  warning. sigrok-cli stops with an error. `--strict` turns every
  warning into a failed run, at any `-l` level, which gives the
  sigrok-cli behaviour for scripts that want it.
- Triggers: `-t` takes several stages separated by `;`, e.g.
  `-t 'D0=r;D1=1'`. One stage is written as in sigrok-cli.
- The demo device's default logic pattern is `oxrok`, a bitmap of that
  word. The other pattern names are the same. Disabled channels read low
  individually; sigrok's demo masks by the number of enabled channels.
- A `.sr` file written by oxrok is created when the first data arrives
  and finished at the end of the capture, in one pass. sigrok-cli reopens
  the archive for every chunk.

## Not available yet

- `-P`, `-A`, `-B`, `-M` (protocol decoders): phase 6 of the roadmap.
- `-T` (transform modules), `--list-supported-wiki`.
- The `csv` input and the other long tail input modules (ChronoVu LA8,
  STF, Trace32, ISF, WAV, LogicPort .lpf).
- fx2lafw and the other hardware drivers: phase 4 of the roadmap.
