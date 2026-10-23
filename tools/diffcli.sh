#!/bin/sh
# Differential check: runs the same command lines through sigrok-cli and
# oxrok and diffs the output after the first header line.
#
#   tools/diffcli.sh [-k substring] [--hw]
#
# SIGROK_CLI and OXROK name the binaries (defaults: sigrok-cli on PATH
# and target/debug/oxrok). --hw adds the hardware cases, which need the
# LogicPort and the fx2lafw device attached, and the ESP32 generator for
# the LogicPort cases: HIL names the bench binary that drives it
# (default target/debug/oxrok-hil), which sets idle inputs and a 1 MHz
# clock on CLK1 for the trigger case.

set -u
SIGROK_CLI=${SIGROK_CLI:-sigrok-cli}
OXROK=${OXROK:-$(dirname "$0")/../target/debug/oxrok}
HIL=${HIL:-$(dirname "$0")/../target/debug/oxrok-hil}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
filter=""
hw=0
while [ $# -gt 0 ]; do
    case "$1" in
        -k) filter=$2; shift ;;
        --hw) hw=1 ;;
        *) echo "unknown argument $1" >&2; exit 2 ;;
    esac
    shift
done
pass=0
fail=0

# Drops the lines that name the tool: the version line and the META
# lines sigrok-cli prints for file inputs.
# Frame markers go too: libsigrok's fx2lafw wraps even a single capture
# in a frame and re-arms before stopping, oxrok sends frames only for
# limit_frames other than 1.
strip_header() {
    grep -v -E '^(META |FRAME-)' | sed -E '1{/^(libsigrok|sigrok-cli|oxrok) /d;}' \
        | sed -E 's/^\$(date|version) .*/$\1 X $end/; s/^\$scope module (libsigrok|oxrok) /$scope module X /'
}

# case NAME [--keep-header] [--stderr] [--names] [--filter SED] -- ARGS...
#   --names compares only the words before a colon, for listings whose
#   descriptions are worded differently; --filter applies a sed script
#   to both outputs first.
case_() {
    name=$1; shift
    strip=1; which=stdout; names=0; filter_expr=""
    while [ "$1" != "--" ]; do
        case "$1" in
            --keep-header) strip=0 ;;
            --stderr) which=stderr ;;
            --names) names=1 ;;
            --filter) filter_expr=$2; shift ;;
        esac
        shift
    done
    shift
    case "$name" in *"$filter"*) ;; *) return ;; esac
    "$SIGROK_CLI" "$@" >"$TMP/a.out" 2>"$TMP/a.err"
    "$OXROK" "$@" >"$TMP/b.out" 2>"$TMP/b.err"
    if [ "$which" = stderr ]; then
        cp "$TMP/a.err" "$TMP/a.cmp"; cp "$TMP/b.err" "$TMP/b.cmp"
    elif [ $strip = 1 ]; then
        strip_header <"$TMP/a.out" >"$TMP/a.cmp"; strip_header <"$TMP/b.out" >"$TMP/b.cmp"
    else
        cp "$TMP/a.out" "$TMP/a.cmp"; cp "$TMP/b.out" "$TMP/b.cmp"
    fi
    if [ -n "$filter_expr" ]; then
        sed -E "$filter_expr" "$TMP/a.cmp" >"$TMP/a.f"; mv "$TMP/a.f" "$TMP/a.cmp"
        sed -E "$filter_expr" "$TMP/b.cmp" >"$TMP/b.f"; mv "$TMP/b.f" "$TMP/b.cmp"
    fi
    if [ $names = 1 ]; then
        sed -E 's/^([^:]*):.*/\1/' "$TMP/a.cmp" | grep -v -E '^  (gnuplot|scale)$' >"$TMP/a.n"; mv "$TMP/a.n" "$TMP/a.cmp"
        sed -E 's/^([^:]*):.*/\1/' "$TMP/b.cmp" >"$TMP/b.n"; mv "$TMP/b.n" "$TMP/b.cmp"
    fi
    if cmp -s "$TMP/a.cmp" "$TMP/b.cmp"; then
        pass=$((pass + 1))
        echo "PASS $name"
    else
        fail=$((fail + 1))
        echo "FAIL $name: $*"
        diff "$TMP/a.cmp" "$TMP/b.cmp" | head -${DIFF_LINES:-6}
    fi
}

# Files written by both tools, compared with a reader.
D4="demo:logic_channels=4:analog_channels=0"
D8="demo:logic_channels=8:analog_channels=0"
DA="demo:logic_channels=4:analog_channels=2"

case_ "demo bits walking-one" -- -d $D4 -g Logic -c pattern=walking-one --samples 20 -O bits
case_ "demo bits incremental 8ch" -- -d $D8 -g Logic -c pattern=incremental --samples 300 -O bits
case_ "demo hex graycode" -- -d $D8 -g Logic -c pattern=graycode --samples 100 -O hex
case_ "demo ascii" -- -d $D4 -g Logic -c pattern=walking-zero --samples 50 -O ascii
case_ "demo analog sine" -- -d demo:logic_channels=0:analog_channels=1 -g A0 -c pattern=sine --samples 20 -O analog
case_ "demo analog square amplitude" -- -d demo:logic_channels=0:analog_channels=1 -g A0 -c pattern=square:amplitude=5 --samples 10 -O analog
case_ "demo csv" -- -d $D4 -g Logic -c pattern=walking-one --samples 10 -O csv:header=false:time=true
case_ "demo csv dedup" -- -d $D4 -g Logic -c pattern=walking-one --samples 10 -O csv:header=false:dedup=true:label=channel
case_ "demo vcd" -- -d $D4 -g Logic -c pattern=walking-one --samples 10 -O vcd
case_ "demo binary" -- -d $D8 -g Logic -c pattern=incremental --samples 16 -O binary
case_ "demo time limit" -- -d $D4 -g Logic -c pattern=walking-one --time 1ms -O bits
case_ "demo analog with logic falls back" -- -d $DA -c samplerate=100k -g Logic -c pattern=walking-one --samples 5 -O bits
case_ "demo show" --keep-header -- -d $DA --show
case_ "demo get samplerate" --keep-header -- -d demo --get samplerate
case_ "demo set bad option" --stderr -- -d demo -c nosuch=1 --samples 1
# Disabled channels: sigrok's demo masks by the count of enabled ones,
# oxrok's by position (docs/CLI.md), so the selection stays contiguous.
case_ "demo channels selection" -- -d $D8 -g Logic -c pattern=incremental -C D0,D1,D2 --samples 24 -O bits
case_ "demo rename channel" -- -d $D4 -g Logic -c pattern=walking-one -C D0=clk,D1 --samples 8 -O bits
# Triggers: sigrok's demo ignores them, oxrok's honours them, so no case.
case_ "input module show vcd" --keep-header --names -- -I vcd --show
case_ "input module show binary" --keep-header --names -- -I binary --show
# csv: the gnuplot script options are not available in oxrok.
case_ "output module show csv" --keep-header --names -- -O csv --show
case_ "output module show vcd" --keep-header --names -- -O vcd --show
case_ "output module show bits" --keep-header --names -- -O bits --show

# Files round trips: each tool writes, the other reads.
"$SIGROK_CLI" -d $D8 -g Logic -c pattern=incremental --samples 100 -o "$TMP/sr.sr" 2>/dev/null
"$OXROK" -d $D8 -g Logic -c pattern=incremental --samples 100 -o "$TMP/ox.sr" 2>/dev/null
case_ "sr written by sigrok-cli" -- -i "$TMP/sr.sr" -O bits
case_ "sr written by oxrok" -- -i "$TMP/ox.sr" -O bits
case_ "sr to csv" -- -i "$TMP/sr.sr" -O csv:header=false
"$SIGROK_CLI" -d $D4 -g Logic -c pattern=walking-one --samples 40 -o "$TMP/sr.vcd" -O vcd 2>/dev/null
"$OXROK" -d $D4 -g Logic -c pattern=walking-one --samples 40 -o "$TMP/ox.vcd" -O vcd 2>/dev/null
case_ "vcd written by sigrok-cli" -- -i "$TMP/sr.vcd" -O bits
# A VCD written by oxrok names its scope "oxrok", which libsigrok's
# reader prefixes to the channel names (docs/CLI.md); only the reverse
# direction diffs clean.
case_ "vcd downsample" -- -i "$TMP/sr.vcd" -I vcd:downsample=4 -O bits
"$SIGROK_CLI" -d $D8 -g Logic -c pattern=incremental --samples 64 -o "$TMP/raw.bin" -O binary 2>/dev/null
case_ "binary input" -- -i "$TMP/raw.bin" -I binary:numchannels=8 -O hex
case_ "file show" --keep-header -- -i "$TMP/sr.sr" --show

# Protocol decoders on a synthetic UART capture.
UART="$(dirname "$0")/../crates/cli/tests/fixtures/uart-hi.vcd"
case_ "uart all annotations verbose" --keep-header -- -l 4 -i "$UART" -P uart:baudrate=115200
case_ "uart rx-data" --keep-header -- -i "$UART" -P uart -A uart=rx-data
case_ "uart ascii start and data" --keep-header -- -i "$UART" -P uart:format=ascii -A uart=rx-data:rx-start
case_ "uart samplenum" --keep-header -- -i "$UART" -P uart -A uart=rx-data --protocol-decoder-samplenum
case_ "uart binary" --keep-header -- -i "$UART" -P uart -B uart=rx
I2C="$(dirname "$0")/../crates/cli/tests/fixtures/i2c-write-read.vcd"
SPI="$(dirname "$0")/../crates/cli/tests/fixtures/spi-mode0.vcd"
case_ "i2c all annotations verbose" --keep-header -- -l 4 -i "$I2C" -P i2c
case_ "i2c unshifted addresses" --keep-header -- -i "$I2C" -P i2c:address_format=unshifted -A i2c=address-write:address-read
case_ "i2c meta" --keep-header -- -i "$I2C" -P i2c -M i2c
case_ "i2c binary" --keep-header -- -i "$I2C" -P i2c -B i2c=data-write
case_ "spi all annotations verbose" --keep-header -- -l 4 -i "$SPI" -P spi
case_ "spi lsb first nibbles" --keep-header -- -i "$SPI" -P spi:bitorder=lsb-first:wordsize=4 -A spi=mosi-data
case_ "spi transfers" --keep-header -- -i "$SPI" -P spi -A spi=mosi-transfer:miso-transfer
case_ "spi binary" --keep-header -- -i "$SPI" -P spi -B spi=miso
UARTSR="$(dirname "$0")/../crates/cli/tests/fixtures/uart-esp32.sr"
SPISR="$(dirname "$0")/../crates/cli/tests/fixtures/spi-esp32.sr"
I2CSR="$(dirname "$0")/../crates/cli/tests/fixtures/i2c-esp32.sr"
case_ "uart real capture" --keep-header -- -l 4 -i "$UARTSR" -P uart:baudrate=921600:rx=D21
case_ "i2c real capture" --keep-header -- -l 4 -i "$I2CSR" -P i2c:scl=D11:sda=D12
case_ "spi real capture" --keep-header -- -l 4 -i "$SPISR" -P spi:clk=D28:miso=D26:mosi=D5:cs=D25
# The documentation text is oxrok's own, and this libsigrokdecode build
# predates decoder tags.
case_ "decoder show uart" --keep-header --names --filter '/^Documentation:/,$d; /^Decoder tags:/,/^Annotation classes:/{/^- /d;/^None\.$/d;}' -- -P uart --show

# Python decoders, when oxrok was built with the python feature.
if "$OXROK" -L 2>/dev/null | grep -q '^  timing '; then
    case_ "python timing" --keep-header -- -l 4 -i "$UART" -P timing
    case_ "python timing options" --keep-header -- -i "$UART" -P timing:edge=rising:avg_period=3
    case_ "python chain uart midi" --keep-header -- -l 4 -i "$UART" -P uart,midi
    case_ "python chain i2c eeprom24xx" --keep-header -- -i "$I2C" -P i2c,eeprom24xx
    case_ "python chain i2c rtc8564" --keep-header -- -i "$I2C" -P i2c,rtc8564
    case_ "python chain spi sdcard" --keep-header -- -l 4 -i "$SPI" -P spi,sdcard_spi
    case_ "python parallel" --keep-header -- -l 4 -i "$SPI" -P parallel:clk=top.clk:d0=top.mosi
    case_ "python decoder show eeprom24xx" --keep-header --names --filter '/^Documentation:/,$d; /^Decoder tags:/,/^Annotation classes:/{/^- /d;/^None\.$/d;}' -- -P eeprom24xx --show
    case_ "decoder listing" --keep-header --filter '1,/^Supported protocol decoders:/d' --names -- -L
fi

if [ $hw = 1 ]; then
    LP="intronix-logicport"
    gen() { [ -x "$HIL" ] && "$HIL" "$@" > /dev/null; }
    gen --clk 0 --set 0
    # The options beyond libsigrok's are filtered out, and the threshold
    # list: the C driver accumulates the 0.05 V step, so its values round
    # differently at one decimal and never equal the current value.
    case_ "logicport show" --keep-header --filter '/^    clock_qualifier:/d; /^    sample_point:/d; /^    invert:/d; /^    frequency_channel:/d; /^    frequency:/d; /^    voltage_threshold:/d' -- -d $LP --show
    case_ "logicport bits 1M" -- -d $LP -c samplerate=1M:voltage_threshold=1.5-1.5 -C D0,D1,CLK1 --samples 64 -O bits
    gen --clk 1000000
    case_ "logicport csv 10M trigger" -- -d $LP -c samplerate=10M:captureratio=25 -C D0,CLK1 --samples 200 -t CLK1=r -O csv:trigger=true:header=false
    gen --clk 0
    case_ "logicport rle" -- -d $LP -c samplerate=1M:rle=on -C CLK1 --samples 500 -O bits
    case_ "logicport 500M" -- -d $LP -c samplerate=500M -C CLK1 --samples 100 -O hex
    # Serial multimeters: the ESP32 plays the meter on its own serial
    # port (EMIT keeps emitting across the reset that opening the port
    # causes), at its fixed 921600 baud.
    DMMPORT=${DMMPORT:-/dev/cu.usbserial-0001}
    gen --cmd "EMIT 200 60 3131323334353b3030303a300d0a"
    case_ "serial dmm ut61e" -- -d uni-t-ut61e-ser:conn=$DMMPORT:serialcomm=921600/8n1 --samples 3
    gen --cmd "EMIT 200 60 1720354d5b617f8297a0b0c0d4e0"
    case_ "serial dmm fs9721" -- -d uni-t-ut60a-ser:conn=$DMMPORT:serialcomm=921600/8n1 --samples 2
    gen --cmd "EMIT 0 0"
    # HID cables: the Pico emulator (pico/hidcable in the LogicPort
    # driver tree) switches cable type on its serial port and reboots,
    # so each mode change waits for the new device. Skipped when the
    # emulator's port is absent.
    CABLE=${CABLE:-$(dirname "$0")/../../sigrok-intronix-driver/pico/hidcable/cable.sh}
    if [ -x "$CABLE" ] && ls /dev/cu.usbmodemHIDCABLE* >/dev/null 2>&1; then
        cable() { "$CABLE" "$@" > /dev/null 2>&1; }
        mode() { cable "MODE $1"; sleep 6; }
        mode bu86x
        cable "PACKET 000001a0dbf8e47c01007cbebf0004008686868600000000" "PERIOD 0" "SAVE"
        case_ "hid dmm bm86x" -- -d brymen-bm86x --samples 2
        mode ch9325
        cable "PACKET 3131323334353b3030303a300d0a" "PERIOD 200" "SAVE"
        case_ "hid dmm ut61e on ch9325" -- -d uni-t-ut61e-ser:conn=hid/ch9325 --samples 2
        mode victor
        cable "PACKET 2b31323334203130000080000d0a" "PERIOD 300" "SAVE"
        case_ "hid dmm victor" -- -d victor-dmm --samples 2
        mode bu86x
    fi
    # A Pico running the sigrok-pico firmware, found by asking every CDC
    # port that is not the HID cable emulator. Inputs float, so the
    # trigger is a level that fires at once.
    PICO=${PICO_PORT:-}
    if [ -z "$PICO" ]; then
        for p in /dev/cu.usbmodem* /dev/ttyACM*; do
            case "$p" in *HIDCABLE*|*\**) continue ;; esac
            if "$OXROK" -d raspberrypi-pico:conn=$p --show >/dev/null 2>&1; then PICO=$p; break; fi
        done
    fi
    if [ -n "$PICO" ]; then
        RP="raspberrypi-pico:conn=$PICO"
        case_ "pico show" --keep-header -- -d $RP --show
        case_ "pico bits d4" -- -d $RP -c samplerate=1M -C D2,D3,D4 --samples 12 -O bits
        case_ "pico hex 5 channels" -- -d $RP -c samplerate=1M -C D2,D3,D4,D5,D6 --samples 20 -O hex
        # The analog inputs float, so their values differ between runs:
        # only the shape of the mixed output is compared.
        case_ "pico mixed csv" --filter 's/[0-9]*\.[0-9][0-9]*/V/g' -- -d $RP -c samplerate=100k -C A0,A1,D2,D3 --samples 3 -O csv:header=false
        case_ "pico trigger csv" -- -d $RP -c samplerate=1M -C D2,D3 -t D2=0 --samples 40 -O csv:trigger=true:header=false
    fi
    case_ "fx2lafw show" --keep-header -- -d fx2lafw --show
    case_ "fx2lafw bits 1M" -- -d fx2lafw -c samplerate=1M --samples 64 -O bits
    case_ "fx2lafw hex 24M" -- -d fx2lafw -c samplerate=24M --samples 1024 -O hex
fi

echo "$pass passed, $fail failed"
[ $fail = 0 ]
