# Devices

## Intronix LogicPort LA1034

Firmware: the vendor's `LogicPort.ccf`, unmodified, installed as
`intronix-logicport.ccf` in one of the firmware directories (first hit
wins):

- `$OXROK_FIRMWARE_DIR`, `$SIGROK_FIRMWARE_DIR` (colon separated lists)
- `~/.local/share/oxrok-firmware`, `~/.local/share/sigrok-firmware`
- `/usr/local/share/oxrok-firmware`, `/usr/local/share/sigrok-firmware`,
  `/opt/homebrew/share/sigrok-firmware`, `/usr/share/oxrok-firmware`,
  `/usr/share/sigrok-firmware`

`sigrok-fwextract-intronix-logicport` from sigrok-util gets the file
from the vendor's Windows installer. Per image `.rbf` files written by
that tool (`intronix-logicport-<image>.rbf`) are accepted as a fallback.

Permissions: on Linux the device is root only without a udev rule; until
oxrok ships rules (phase 7), a rule with `MODE="0666"` for `0403:dc48`
does. macOS and Windows need nothing beyond `docs/USB.md`.

Options beyond libsigrok's, set with `-c`:

- `clock_qualifier=off|active-high|active-low`: in state mode, sample
  only while the qualifier input is high or low.
- `sample_point=on|before|after`: in state mode, sample on the clock
  edge, 2.5 ns before it or 2.5 ns after it (the FPGA image changes).
- `invert=D0,CLK1` or a hex mask, `none` to clear: inputs the device
  reads inverted, for the trigger and the stored data.
- `frequency_channel=CLK1`: the input of the frequency counter.
- `frequency`: read only, `--get frequency` measures the frequency on
  that input to 10 Hz, up to 300 MHz. A `--show` also measures it, which
  configures the FPGA on a cold device.

Triggers take the forms in `docs/CLI.md`: occurrence counts (plain or
consecutive), durations, value ranges on a channel group, and two stages
in sequence, or, or and.

Bench: `oxrok-hil` runs the hardware checks (levels, threshold, timebase
up to 500 MHz, triggers, counts, durations, value ranges, combine modes,
sample points, inversion, the frequency counter, compression, frames,
all byte lanes) against a LogicPort wired to the ESP32 signal generator
from the libsigrok driver repository. `oxrok-hil -k trigger` runs a
subset, `LOGICPORT_CONN` picks a unit, `SIGGEN_PORT` the serial port.
`oxrok-hil --clk HZ`, `--set MASK`, `--cmd LINE` and `--delay MS` only
drive the generator, for captures by hand. Opening the serial port
resets the ESP32, so the commands of one experiment go in one
invocation. Checks of features not yet seen working print UNVERIFIED
rather than FAIL; `--strict` counts them as failures. The `qualifier`
section needs a wire from GPIO15 to CLK2 and `--clk2`; it is the one
section still unverified, and it also tells which input the qualifier
is.

Verified 2026-10-22 on two units: all 60 bench checks pass, the `bits`
and `csv` outputs match the libsigrok driver through sigrok-cli byte for
byte for the same captures, and a cold start (unconfigured FPGA after a
power cycle) configures and captures in about 4 s: the open sees no
answer, loads the image by bit-bang, resyncs once while the FPGA comes
up, and proceeds.

## Serial multimeters

Handheld meters with a serial cable, one driver id per meter as in
sigrok (`uni-t-ut61e-ser`, `voltcraft-vc820-ser`, `metex-m3640d`, ...;
`oxrok -L` lists them). The byte stream does not say which meter is
talking, so the driver id picks the chipset protocol and the cable's
serial parameters, `conn=` names the port and `serialcomm=` overrides
the parameters:

```
oxrok -d uni-t-ut61e-ser:conn=/dev/ttyUSB0 --samples 10
oxrok -d metex-m3640d:conn=/dev/ttyUSB0:serialcomm=1200/7n2/rts=0/dtr=1 --continuous
```

The scan listens for up to three seconds for a packet the chipset
parser accepts. Readings stream as analog packets, one per display, on
channels `P1` (and `P2` to `P4` for the four display Metex meters);
`--samples` counts packets, `--time` limits the run, `--continuous`
runs until stopped.

Chipsets ported: FS9721, FS9922, DTM0660, Metex 14 byte (single and
four display), VC-96, ASYC-II, M2110, UT71x, ES519xx (11 and 14 byte,
2400 and 19200 baud variants), VC-870, RadioShack 22-812, Brymen BM25x,
BM52x, BM82x, BM85x, BM86x, Digitech QM1578, EEVblog 121GW, Mastech
MS2115B and MS8250D, Meterman 38XR: all 69 meters of libsigrok's
serial-dmm driver. Multi display meters put each display on its own
channel (`P1`/`P2`, or `main`/`sub`/`bar` where sigrok names them so).
Not ported: the recordings a BM52x stores in memory.

The BM85x is the one meter with variable length packets: its protocol
frames the stream itself (`Protocol::frame`), and the driver pulses RTS
after opening the port as the meter requires. Nothing on this list past
the FS9721 and ES519xx families has met a real meter; see below.

### HID cables

Some cables are USB HID devices rather than serial ports: the Brymen
BU-86X infrared adapter (`brymen-bm52x`, `brymen-bm82x`,
`brymen-bm86x`), the Victor 70C and 86C cable (`victor-dmm`), and the
WCH CH9325 and Silicon Labs CP2110 UART bridges that other meters'
cables use. `oxrok-hid` talks to them through `async-hid` (IOHIDManager
on macOS, hidraw on Linux, Win32 HID on Windows, no C code), and
`oxrok-serial` wraps each chip's report framing as a port, so the
meter drivers see a byte stream as with a serial cable. The port name
follows sigrok:

```
oxrok -d brymen-bm86x --samples 5                       # finds the adapter on its own
oxrok -d brymen-bm86x:conn=hid/bu86x/sn=1234 --samples 5
oxrok -d uni-t-ut61e-ser:conn=hid/ch9325 --samples 5    # a serial meter on a HID bridge
```

`hid` takes any cable with a known USB id, `hid/CHIP` one of `bu86x`,
`ch9325`, `cp2110`, `victor`, and `sn=` picks one by serial number. On
Linux the user needs read and write access to the `/dev/hidraw*` node
(a udev rule for the cable's USB id). Meters that come with a HID cable
scan without `conn=`; an unplugged default cable is simply not found.

Verified 2026-10-23 with a Raspberry Pi Pico running the cable emulator
in `pico/hidcable` of the LogicPort driver tree: it enumerates as the
BU-86X, the CH9325 or the Victor cable (`MODE bu86x|ch9325|victor` on
its USB serial port, `PACKET hex`, `PERIOD ms`, `SAVE`; `cable.sh`
sends the commands) and plays a meter packet. `brymen-bm86x` on the
BU-86X, `uni-t-ut61e-ser:conn=hid/ch9325` and `victor-dmm` give the same
readings through oxrok and sigrok-cli; `tools/diffcli.sh --hw` runs
the three cases. The CP2110 has no emulation yet. sigrok-cli needed two
small fixes in libsigrok's `serial_hid.c` to work on this macOS at all
(the `DevSrvsID:` paths of current hidapi, and a double conversion of
found paths); they are applied in the local build only.

### UNI-T meters on the UT-D04 cable, and the UT32x

libsigrok's `uni-t-dmm` driver covers 27 meters whose UT-D04 cable is a
CH9325 HID bridge (`uni-t-ut61e`, `voltcraft-vc820`, `tenma-72-7730`,
`uni-t-ut372`, ...). oxrok registers the same ids as twins of the
serial entries over `hid/ch9325`, so they scan without `conn=` and also
take sigrok's `conn=1a86.e008` form. The UT372 tachometer parser came
with them. The UNI-T UT32x thermometer (`uni-t-ut32x`) has its own
driver: channels `T1`, `T2`, `T1-T2`, `data_source` `Live` or `Memory`,
`conn=` required since the cable cannot be told from any other UT-D04.
Both were exercised with the Pico emulator in CH9325 mode (a UT61E
stream for the meters, a 23.5 C packet for the UT32x); sigrok-cli has
no reference run for them on macOS, where libusb cannot claim a HID.

## USB relay boards and the PICkit2

`dcttech-usbrelay` switches the dcttech relay boards (`R1` to `Rn`
channel groups, `enabled` per group or for all at once):

```
oxrok -d dcttech-usbrelay --show
oxrok -d dcttech-usbrelay -g R2 --set -c enabled=on
oxrok -d dcttech-usbrelay:conn=sn=ABCDE -g R1 --get enabled
```

`microchip-pickit2` uses a PICkit2 as a three channel logic analyzer
(pins 4 to 6, 1024 samples, 5 kHz to 1 MHz, a one stage trigger on
levels and edges, `captureratio` presets 0 to 6 for the trigger
position):

```
oxrok -d microchip-pickit2 -c samplerate=1M --samples 1024 -O bits
oxrok -d microchip-pickit2 -c samplerate=250k:captureratio=2 -t pin5=r --samples 1024 -O csv
```

libsigrok reaches the PICkit2 with raw USB transfers, which macOS does
not allow on a HID; oxrok speaks HID reports instead, so it works on
every platform. Both drivers were verified against the Pico emulator
(`MODE usbrelay`, `MODE pickit2`, which plays a counter on the three
pins); the user's real PICkit2 is the next check.

## Raspberry Pi Pico (sigrok-pico firmware)

A Pico running the sigrok-pico firmware is a 21 channel logic analyzer
with 3 analog inputs over its USB serial port, driver `raspberrypi-pico`.
`conn=` is required; the Pico shows up as a CDC port (`/dev/cu.usbmodem*`
on macOS, `/dev/ttyACM*` on Linux):

```
oxrok -d raspberrypi-pico:conn=/dev/ttyACM0 -c samplerate=1M -C D2,D3,D4 --samples 100 -O bits
oxrok -d raspberrypi-pico:conn=/dev/ttyACM0 -c samplerate=100k -C A0,D2 --samples 50 -O csv
oxrok -d raspberrypi-pico:conn=/dev/ttyACM0 -c samplerate=1M -C D2,D3 -t D2=r --samples 1000 -O bits
```

Digital channels are `D2` to `D22` (the board's GPIO numbers) and must
be enabled from D2 up without gaps; analog `A0` to `A2` are GPIO 26 to
28 scaled to volts. The firmware takes any sample rate and the driver
rounds it to a common divisor when analog and digital run together, as
libsigrok does. Triggers run on the host (one stage, digital channels,
with `captureratio` of pre-trigger samples); the device streams until
told to stop. Rates the firmware cannot do abort the capture, the same
way in both tools (one analog channel at 1 MHz, for example).

Verified 2026-10-23 on a Pico with `pico_baseline.uf2`: `--show`, bits,
hex, ascii, csv with and without trigger marks, analog, mixed and
triggered captures are byte identical between oxrok and sigrok-cli;
two recorded sessions are the replay tests. Not yet exercised: a real
signal on the inputs (a wire from the ESP32 to GPIO 2 would do) and the
larger digital-only firmware variants.

## Rigol and Agilent oscilloscopes

The `rigol-ds` driver covers the same 61 models as libsigrok's: Rigol
VS5000, DS1000 (DS1052E, DS1102E, DS1152E and the D variants with the
logic pod), DS2000 and DS2000A/MSO2000A, DS1000Z and MSO1000Z, DS4000,
MSO5000, and the Agilent DSO1000A/B and MSO7034A. It speaks SCPI over
`oxrok-scpi`, which carries USBTMC (the USB port on the scope), raw TCP,
Rigol's length prefixed TCP and serial ports:

```
oxrok -d rigol-ds --show                           # any USBTMC scope
oxrok -d rigol-ds:conn=usbtmc/1.5 --frames 1 -O analog
oxrok -d rigol-ds:conn=tcp-rigol/192.168.1.20/5555 -c timebase=1ms -C CH1 --frames 2 -O csv
```

Options follow sigrok: `timebase`, `triggersource`, `triggerslope`,
`triggerlevel`, `horiz_triggerpos`, `data_source` (`Live`, `Memory`,
`Segmented` as the series allows) and `limit_frames`; per analog channel
group (`-g CH1`) `vdiv`, `coupling` and `probe_factor`. Every setting is
written to the scope at once and the current values are read back when
the device opens. A capture downloads each enabled channel once per
frame, scaled to volts, between `FRAME-BEGIN` and `FRAME-END`; the
sample rate is the frame length over the screen width for `Live`
and `:WAV:XINC?` for the memory sources.

One difference from libsigrok on purpose: `horiz_triggerpos` is
converted to seconds with the number of horizontal divisions, where
libsigrok multiplies by the number of timebases in its table.

Verification: the DS1000E path (readback, one live frame, key unlock
on close) runs in a replay test against a hand written SCPI fixture
with the exact command sequence; the user's DS1102E is the bench device
for the real run, which has not happened yet. The other series have
no recordings; `OXROK_SCPI_RECORD=FILE` captures one from any run.

## fx2lafw devices

Saleae Logic and its clones, the CWAV USBee models, the bare Cypress FX2
board, Braintechnology USB-LPS and the sigrok FX2 LA boards. The driver
is `fx2lafw`; 8 channels at up to 24 MS/s, 16 channels at up to
12 MS/s on the models that have them, triggers on the host.

Firmware: the sigrok `fx2lafw-*.fw` files (the `sigrok-firmware-fx2lafw`
package, or `/opt/homebrew/share/sigrok-firmware` from Homebrew's
sigrok). The driver uploads the file for the device's USB id when the
device does not report the firmware in its strings, then waits up to
3 s for it to renumerate.

Analog channels (USBee AX) are not supported.

Verified 2026-10-22 on a Saleae Logic clone (`0925:3881`, firmware 1.4):
the cold path after a power cycle (upload, renumeration in about 2 s,
reopen and claim), 60 s at 24 MS/s delivering every byte, and `--show`
identical to sigrok-cli. Trigger and timing checks against the
signal generator need a wire from the ESP32 to the analyzer; they are
not run yet.
