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

Unverified: no HID cable was on the bench. The chip framing, the
Victor unscrambling and both Brymen parsers have tests written from the
C implementations and the Brymen protocol document; the BM86x request
and answer also run through the driver in a replay test. A
microcontroller that enumerates as one of these cables and plays a
meter would close the gap, as the ESP32 does for serial meters.

Verification without a meter: the parsers have tests built from the
packet layouts, the driver has replay tests on hand written serial
fixtures (`OXROK_SERIAL_RECORD=FILE` records a real session into the
same format), and the ESP32 generator plays a meter on its own serial
port (`EMIT <ms> <seconds> <hex>`, kept in flash across the reset that
opening the port causes). Verified 2026-10-23 that way: the UT61E and
UT60A streams give the same readings through oxrok and sigrok-cli
(`tools/diffcli.sh --hw`, cases `serial dmm`). The ESP32 port runs at
921600 baud, so those runs pass `serialcomm=921600/8n1`.

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
