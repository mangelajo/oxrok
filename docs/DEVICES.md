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

Bench: `oxrok-hil` runs the hardware checks (levels, threshold, timebase
up to 500 MHz, triggers, compression, frames, all byte lanes) against a
LogicPort wired to the ESP32 signal generator from the libsigrok driver
repository. `oxrok-hil -k trigger` runs a subset, `LOGICPORT_CONN`
picks a unit, `SIGGEN_PORT` the serial port. `oxrok-hil --clk HZ`,
`--set MASK`, `--cmd LINE` and `--delay MS` only drive the generator,
for captures by hand. Opening the serial port resets the ESP32, so the
commands of one experiment go in one invocation.

Verified 2026-10-22 on two units: all 35 bench checks pass, and the
`bits` and `csv` outputs match the libsigrok driver through sigrok-cli
byte for byte for the same captures.

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
upload and renumeration, 60 s at 24 MS/s delivering every byte, and
`--show` identical to sigrok-cli. Trigger and timing checks against the
signal generator need a wire from the ESP32 to the analyzer; they are
not run yet.
