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
| 6a | LogicPort extras: trigger counts, durations, value ranges, or/and combines, clock qualifier, sample point, inversion, frequency counter | done 2026-10-22: `-t` grammar in core, options on the device, 60 bench checks pass; the verification below is still open |
| 6b | serial transport and the serial multimeter family | done 2026-10-23: `oxrok-serial` with record and replay, `oxrok-dmm` with 15 chipset parsers, 64 meters; UT61E and UT60A streams from the ESP32 identical to sigrok-cli; HID cables (`oxrok-hid`, BU-86X, CH9325, CP2110, Victor) added the same day, unverified for want of a cable |
| 6c | SCPI transport (USBTMC, TCP, serial) and the first scope driver (`rigol-ds`) | a Rigol scope captures frames; replay fixtures for the other SCPI drivers |
| 6d | the remaining USB logic analyzers, one per recording | each ships a replay fixture; bench runs where the device is at hand |
| 7 | packaging: static binaries, Homebrew, AUR, udev rules | a clean machine installs and captures without any sigrok package |
| 8 | release 0.1 | tags, changelog, guides |

## Open verification

Hardware checks not yet done, with what each needs:

- Clock qualifier input: which input gates the clock (CLK2, or the other
  clock input, or a data channel). Wire ESP32 GPIO15 to CLK2, run
  `oxrok-hil -k qualifier --clk2`; it reports which of off, active-high
  and active-low complete or stall with CLK2 low and high. Then fix the
  option's description in `docs/DEVICES.md` and the driver.
- Sample point timing: the `setup-hold` section only shows that images
  0 to 4 capture correctly. Whether `before` and `after` move the sample
  by 2.5 ns needs a source phase locked to the clock with an adjustable
  delay, which the ESP32 does not provide.
- Duration delay at other rates: the one sample correction was measured
  at 1 MHz; check 100 MHz and 500 MHz DDR (ticks are two samples there)
  with the `duration` section at those rates.
- Inversion and the clock edge: `invert=CLK1` with state mode should
  sample on the opposite edge; check with `sample_point=before` that
  the image follows (`StateBeforeClk1Fall` for a rising edge selection).
- Counter source 1 (pattern entered) is used for level only counts and
  passed with `D0=1` entered twice in the software trigger model; the
  hardware check (`D0=1*2` on the pulse train) is not in the bench yet.
- Frequency counter above 1 MHz, up to the 300 MHz the vendor states:
  needs a faster source than the ESP32's LEDC output.
- HID cables: no BU-86X, CH9325, CP2110 or Victor cable was on the
  bench, so the chip framing and the Brymen parsers are tested only
  against the C implementations. A microcontroller with native USB
  (an ESP32-S2/S3 or RP2040) enumerating with one of the cables' USB
  ids and replaying a meter would verify them the way the ESP32 does
  for serial meters; the `EMIT` idea carries over.
