# Firmware for hardware in the loop checks

Two microcontroller projects that stand in for instruments on the
bench. Both build with PlatformIO (`pio run` in the directory).

- `esp32-siggen`: an ESP32 signal generator driven by `oxrok-hil` over
  its serial port: clocks and pin patterns for the LogicPort, and
  `EMIT` to play a multimeter's serial stream. Commands are listed at
  the top of `src/main.cpp`.
- `pico-hidcable`: a Raspberry Pi Pico that enumerates as a USB device
  under test: the Brymen BU-86X, WCH CH9325 and Victor HID cables, a
  dcttech relay board, a PICkit2 and a Rigol DS1102E on USBTMC. `MODE`
  on its serial port picks the role, `cable.sh` sends the commands,
  `BOOTSEL` drops it into the bootloader for `picotool load -x`. The
  same Pico runs the sigrok-pico firmware for the `raspberrypi-pico`
  driver when flashed with it.

`tools/diffcli.sh --hw` detects which of these is attached and runs
the matching comparisons against sigrok-cli.
