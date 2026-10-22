# Roadmap

| Phase | Deliverable | Exit criterion |
|---|---|---|
| 0 | workspace, CI, core types, demo driver, CLI with sigrok-cli compatible outputs | done when `cargo nextest run` is green on three platforms and the demo outputs match sigrok-cli byte for byte |
| 1 | `.sr` read and write, VCD, CSV, binary | round trips; sigrok-dumps files load; PulseView opens our files |
| 2 | USB transport (nusb, optional libusb), record and replay, FTDI | FT245 bitbang and FIFO work on macOS and Linux; a recorded session replays |
| 3 | Intronix LogicPort driver, bench crate, replay fixture | the bench checks pass against the new CLI; first driver porting guide |
| 4 | EZ-USB upload, software trigger, fx2lafw driver | continuous 8 ch at 24 MS/s for 60 s without drops; bench checks |
| 5 | CLI parity pass and the differential script | documented flows diff clean against sigrok-cli on both devices |
| 6 | decoder runtime, native uart, i2c, spi, Python decoder host, conformance fixtures | annotations identical to libsigrokdecode on the fixtures |
| 7 | packaging: static binaries, Homebrew, AUR, udev rules | a clean machine installs and captures without any sigrok package |
| 8 | release 0.1 | tags, changelog, guides |
