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
picks a unit, `SIGGEN_PORT` the serial port.

Verified 2026-10-22 on two units: all 35 bench checks pass, and the
`bits` and `csv` outputs match the libsigrok driver through sigrok-cli
byte for byte for the same captures.
