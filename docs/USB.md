# USB transport

Drivers talk to USB through the `Transport` trait in `oxrok-usb`:
control and bulk transfers with timeouts, interface claims, resets and
the packet size of an endpoint. Two backends implement it, selected by
cargo feature. The drivers never see which one is in use.

| Feature | Crate | Platforms | Notes |
|---|---|---|---|
| `nusb` (default) | nusb 0.2 | Linux, macOS, Windows, Android | pure Rust, no system library |
| `libusb` | rusb 0.9 | the above plus the BSDs, Solaris, Haiku | builds libusb from source |

With both compiled in, `OXROK_USB_BACKEND=libusb` or `=nusb` picks one
at run time; nusb is the default.

`cargo run -p oxrok-usb --example list` prints what the backend sees.

## Verified

2026-10-22, macOS 26 on an Intronix LogicPort LA1034 (FT245BL,
`0403:dc48`): both backends open the device, claim interface 0, run the
FTDI vendor requests (reset, latency timer, baud rate, bit-bang mode,
pin readback, modem status, EEPROM read) and read the FIFO. No kernel
driver claimed the device; Apple ships no FTDI driver since Monterey
and the LogicPort's product id is not one a third party FTDI driver
matches by default.

## Streams

`Transport::bulk_in_stream` opens a queue of IN transfers for devices
that stream faster than one blocking read after another can drain
them: the nusb backend keeps `depth` transfers submitted in the kernel,
the libusb backend runs a thread reading back to back. A recording
logs every completed transfer as a `bulk_in` of the transfer size, and
a replay answers a stream from those steps.

## Record and replay

`record::Recorder` wraps a transport and logs every call with its
result. `record::Replay` answers from such a log and fails on the first
call that differs from the recording: a different request, different
data, or a call past the end. Every driver ships at least one recorded
session so its logic runs in CI without hardware.

A fixture is a text file, one call per line, hand editable:

```text
# oxrok usb fixture 1
device vid=0403 pid=dc48 version=0400 bus=0 addr=1 product="LogicPort Logic Analyzer"
claim 0 -> ok
mps 81 -> 64
ctl_out 40 09 0004 0000 - -> ok
ctl_in 40 0a 0000 0000 1 -> 04
bulk_out 02 00{4096} -> 4096
bulk_in 81 4096 -> 3160aabb
bulk_in 81 4096 -> err timeout
```

- `device`: what enumeration reported; strings are quoted with `\"`
  and `\\` escapes.
- `config N`, `claim N`, `release N`, `reset`, `mps EP`: result `ok`, or
  the packet size for `mps`.
- `ctl_out TYPE REQ VALUE INDEX DATA`, `ctl_in TYPE REQ VALUE INDEX LEN`:
  the setup fields in hex, `TYPE` without the direction bit. Results
  are `ok` and the data read.
- `bulk_out EP DATA`, `bulk_in EP LEN`: results are the byte count and
  the data read.
- Data is hex pairs; `-` is empty and `xx{N}` repeats a byte, so a zero
  flood stays one short token. Data sent to the device that is longer
  than 1 KiB is recorded as `@LEN:SHA256` and compared by digest, so a
  1 MB bitstream is one line.
- Errors are `err KIND ["message"]` with kind `timeout`, `busy`,
  `device`, `io` or `unsupported`.

Timeouts are not recorded; a replay answers at once. Polling loops run
on time, so a replay may poll more or fewer times than the recording:
a read identical to the one just answered is answered again, and
leftover identical reads in the recording are skipped.

`OXROK_USB_RECORD=FILE` makes `oxrok_usb::open` record every session
and write the fixture when the device is closed, with any program
that uses the drivers:

```sh
OXROK_USB_RECORD=capture.txt oxrok -d intronix-logicport -c samplerate=10M --samples 64 -O bits
```

To record, wrap the transport before handing it to the driver:

```rust
let (recorder, handle) = Recorder::with_handle(oxrok_usb::open(&info)?);
let ftdi = Ftdi::open(Box::new(recorder), Port::A)?;
// ... drive it ...
std::fs::write("session.txt", handle.fixture().to_text())?;
```

`cargo run -p oxrok-ftdi --example probe -- [conn] [--record FILE]`
does this for the FTDI layer: it runs every request that drives no pin
and writes the fixture.

## FTDI

`oxrok-ftdi` speaks the FT232/FT245 vendor protocol over a transport:
reset and purge, latency timer, baud rate (which also clocks the
bit-bang modes), line settings, modem control and status, the bit modes,
pin readback and EEPROM reads. Bulk reads strip the two status bytes
that begin every USB packet. The EEPROM write request is not
implemented and will not be: a wrong write bricks the device.

The baud rate encoding follows FTDI's application note AN_120: a 3 MHz
clock (12 MHz on the H chips above 1.2 MBaud), divisors 1 and 1.5 with
their own codes, then 14 integer bits and a 3 bit code for the eighths.

## Troubleshooting

- Linux: without a udev rule the device is root only. Phase 7 ships
  rules; until then `sudo` or a rule with `MODE="0666"` for the vendor
  and product id. A kernel driver (`ftdi_sio` for FTDI devices with a
  known product id) is detached when the interface is claimed, with
  both backends.
- macOS: a third party FTDI VCP driver that matches the device would
  block the claim with a busy error. `kextstat | grep -i ftdi` shows
  whether one is loaded. After a firmware upload and renumeration the
  system can hand out a device whose configuration is active but whose
  interface cannot be claimed ("interface not found"); the nusb backend
  then sets the configuration again and reopens the device, which is
  what libusb does on every open.
- Windows: nusb needs the WinUSB driver bound to the device, as libusb
  does; use Zadig once. The control transfers go through the claimed
  interface, so drivers claim before any request (the FTDI layer does).
- The `bus.address` in `conn=` comes from the OS: on Linux the real
  numbers, on macOS the ones libusb derives from the location id (the
  same sigrok shows), on Windows always `0.0`.
