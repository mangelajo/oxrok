# Intronix LogicPort LA1034

Driver: `intronix-logicport`, crate `oxrok-driver-logicport`. 34 channels
(D0-D31, CLK1, CLK2), 2048 sample entries, 1 kHz to 500 MHz timing mode,
state mode on CLK1 or CLK2 with a qualifier and a selectable sample
point, run length compression up to 200 MHz, two hardware trigger levels
with occurrence counts, durations and value ranges, per input inversion,
a frequency counter, an input threshold of -6 to 6 V in 50 mV steps.

The protocol was worked out from USB captures of the vendor software for
the libsigrok driver by the same author; this crate is a port of that
driver with the same register map. What follows is the summary a reader
of the code needs.

## Hardware

An FT245BL USB FIFO bridge (`0403:dc48`) in front of an SRAM based Altera
Cyclone FPGA. The FPGA holds no configuration at power up. The host loads
one of eight images from the vendor's `LogicPort.ccf` through the FTDI
chip in bit-bang mode (passive serial: DATA0 bit 0, DCLK bit 1, nCONFIG
bit 2 as outputs, nSTATUS bit 3 and CONF_DONE bit 4 read back), then
talks to the FPGA in FIFO mode. The bit-bang clock is set with the FTDI
baud rate request (460800). Which image is loaded can be read back from
register `0x10000f`: `0x10 | index`.

| Index | Image | Used for |
|---|---|---|
| 0 | state-after | state mode, sample 2.5 ns after the edge (unused) |
| 1-4 | state-before-clk{1,2}-{rise,fall} | state mode, 2.5 ns before the edge (unused) |
| 5 | state-on | state mode, `external_clock=on` |
| 6 | timing-ddr | 500 MHz |
| 7 | timing-sdr | 1 kHz to 250 MHz |

## Command protocol

Every command is five bytes: opcode, 16 bit offset (LE), 16 bit count
minus one (LE), followed by the write data. The device answers with a
three byte ack: the opcode and a 16 bit sequence number (BE) that counts
commands since the last resync, followed by read data. A 32 bit register
address is a segment (upper 16 bits, selected once with `0xc3`) and an
offset.

| Opcode | Meaning |
|---|---|
| `0xc1` | write `count` bytes at `offset` |
| `0xc2` | read `count` bytes at `offset` |
| `0xc3` | select segment `offset` |
| `0x01` | resync: reset the parser and the sequence counter |

The device keeps its sequence counter across host sessions, so every open
starts with a resync: purge, 95535 zero bytes to drain any partial
command, `0x01`, purge. A bad ack marks the link out of step; nothing is
sent until the next resync.

## Registers

Control block at `0x100000`: status (read; bit 0 done, bit 1 memory
wrapped, bit 5 triggered, bit 6 pre-filled), configure control (write;
**only ever written with 0**, which releases the FPGA configuration
before a reload), arm `+2`, run `+3`, pre and post trigger depth `+4`
and `+6` (16 bit), threshold DAC `+8` (16 bit), end pre-fill `+a`
(write) and unfilled count (read), end post-fill `+b`, force trigger
`+c` (write 1; unused), trigger combine `+d` (0: none, 1: level A, 2: B,
3: A or B, 4: A and B, 5: A then B), image id `+f`. Nine bytes read from
`+1` are the wire status (seen high, seen low per input; unused).

Trigger levels A at `0x200000` and B at `0x400000`: pattern mask `+00`,
pattern value `+05`, count high `+0a`, count low `+0e` (20 bit), value
bit order `+12` (1: channel k weighs 2^k), condition `+14`, counter
source `+16`, counter mode `+17`, consecutive `+18`, prequalify `+19`
(unused), value mask `+1a`, value low `+1f`, value high `+24`, rising
mask `+29`, falling mask `+2e`. Masks are 5 bytes, one bit per channel.
The condition is a pattern code (1: pattern true, 2: false) plus a
value code (0x0c: value within the bounds, 0x10: outside; the bounds
are scattered onto the group's channel bits). Counter source 3 counts
edges, 1 the pattern becoming true, 0 the sample clock. Counter mode 1
fires when the count reaches count high plus one; mode 2 fires once the
condition has held for more than count low ticks (a tick is a sample,
or two at 500 MHz). Mode 2 needs the consecutive flag, else the counter
keeps counting across gaps in the condition, and marks the trigger one
sample after the pipeline delay below. Mode 3 (a duration window) is
not used. The value registers are written only when a value term is in
use, so the default register stream is the one the replay fixtures
hold.

Clocking at `0x1000000`: clock mode (write; bit 2 CLK1, bit 1 CLK2, bit 0
falling edge, bit 4 qualifier off, bit 3 qualifier active low, neither
for active high) or trigger position (read), channel config `+1`,
compression on `+2`, compression mask `+3` (5 bytes), invert mask `+8`
(5 bytes; the device inverts those inputs for the trigger and in the
stored data). Timebase: prescaler `0x20000000` and 1-2-5 step
`0x20000001`, from the rate table in `protocol.rs`. Frequency counter at
`0x40000000`: write the input (D0..D31 by index, CLK1 0x20, CLK2 0x21),
read 0 there when a measurement is ready, then the 32 bit value at
`+1` in units of 10 Hz. The gate free runs, so the first ready value
after selecting an input can still be the previous input's; the driver
takes the second. Init counter `0x40000005` increments for about 250 ms
after configuration.

The FPGA image follows the settings: 7 for timing mode up to 250 MHz,
6 for 500 MHz, and in state mode 5 to sample on the clock edge, 0 for
2.5 ns after it, 1 to 4 for 2.5 ns before a CLK1 rising, CLK2 rising,
CLK1 falling or CLK2 falling edge, taking an inverted clock input into
account.

Sample memory at `0x2000000`: write pointer `+0` (bank B `+100`), four
data lanes from `+1000` with a stride of `0x1000`, bank B lanes from
`+5000`, flags lane at `+9000`. 2048 entries of 40 bits: D0-D31 in the
lanes, CLK1 and CLK2 in flag bits 1 and 2 (bank B shifted by 4). With
compression an entry with flag bit 3 set is a count record: the lanes
and the low three flag bits form a 35 bit count of extra sample periods
without a transition.

## Acquisition

1. Load the image the settings need, if it is not loaded already.
2. Write the settings in the vendor software's order (`program_regs`).
   When the threshold DAC changed, wait 200 ms for it to settle.
3. Write run=1, arm=1, then poll status every 10 ms until done. With
   compression and a sample limit, end the pre-fill and post-fill phases
   by time, since an idle input never fills the memory.
4. Read the window from the write pointer: the whole ring when it
   wrapped, else what was filled. In SDR mode a leading count record is
   dropped. The trigger position register counts entries after the
   trigger record; the pipeline delay is 7 samples in SDR and 21 in DDR
   mode.
5. Expand count records, apply the sample limit around the trigger
   (honouring the capture ratio) and send the samples, 5 bytes each.

## Threshold

The DAC code is `round((4.559 - v) / 5.875 * 1023) + cal` with
`v = (volts - 1.315) * -0.4020 + 1.315`, clamped to 0..1023. `cal` is a
signed three digit string in the FTDI EEPROM user area, right after the
string descriptors and the PnP block, e.g. `-004`. The EEPROM is read
with the FTDI vendor request and never written.
