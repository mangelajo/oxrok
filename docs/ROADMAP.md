# Roadmap

| Phase | Deliverable | Exit criterion |
|---|---|---|
| 0 | workspace, CI, core types, demo driver, CLI with sigrok-cli compatible outputs | done when `cargo nextest run` is green on three platforms and the demo outputs match sigrok-cli byte for byte |
| 1 | `.sr` read and write, VCD, CSV, binary | round trips; sigrok-dumps files load; PulseView opens our files |
| 2 | USB transport (nusb, optional libusb), record and replay, FTDI | done 2026-10-22: FT245 requests, bit-bang entry and FIFO reads work on macOS with both backends; a recorded session replays in CI. Linux is checked with the first driver |
| 3 | Intronix LogicPort driver, bench crate, replay fixture | done 2026-10-22: 35 bench checks pass, `bits` and `csv` match libsigrok byte for byte, two recordings replay in CI, `docs/DRIVER_PORTING.md` |
| 4 | EZ-USB upload, software trigger, fx2lafw driver | done 2026-10-22 on a Saleae Logic clone: firmware upload and renumeration, 60 s at 24 MS/s without a missing byte (1.6 s CPU, 25 MB), `--show` identical to sigrok-cli, continuous mode stopped by Ctrl-C, a recording replays in CI |
| 5 | CLI parity pass and the differential script | done 2026-10-22: `tools/diffcli.sh` runs 29 software and 8 hardware cases against sigrok-cli 0.8.0-git, all clean; module listings compared by option name |
| 6 | decoder runtime, native uart, i2c, spi, Python decoder host, conformance fixtures | done 2026-10-22: runtime, native uart, i2c, spi, stacking and the PyO3 host (all 112 upstream decoders load, checked ones identical to sigrok-cli); UART, I2C and SPI fixtures from the bench |
| 7 | packaging: static binaries, Homebrew, AUR, udev rules | a clean machine installs and captures without any sigrok package |
| 8 | release 0.1 | tags, changelog, guides |
