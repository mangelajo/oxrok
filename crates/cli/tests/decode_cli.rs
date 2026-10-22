//! Protocol decoding through the binary, compared with annotation
//! output captured from sigrok-cli 0.8.0-git with libsigrokdecode on
//! 2026-10-22. `uart-hi.vcd` is a synthetic 8N1 capture at 115200 baud,
//! 1 MHz, carrying "Hi!\n", 0x00 and 0xff.

use std::process::Command;

fn oxrok(args: &[&str]) -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_oxrok")).args(args).output().expect("run oxrok");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

const VCD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/uart-hi.vcd");
const I2C: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/i2c-write-read.vcd");
const SPI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/spi-mode0.vcd");

#[test]
fn uart_annotations_match_libsigrokdecode() {
    let (out, _, ok) = oxrok(&["-l", "4", "-i", VCD, "-P", "uart:baudrate=115200:rx=top.D0"]);
    assert!(ok);
    assert_eq!(out, include_str!("fixtures/uart-hi.ann"));
    // Default verbosity: only the longest text, no sample numbers.
    let (out, _, _) =
        oxrok(&["-i", VCD, "-P", "uart:baudrate=115200:rx=top.D0", "-A", "uart=rx-data"]);
    assert_eq!(out, "uart-1: 48\nuart-1: 69\nuart-1: 21\nuart-1: 0A\nuart-1: 00\nuart-1: FF\n");
    let (out, _, _) = oxrok(&["-i", VCD, "-P", "uart:format=ascii", "-A", "uart=rx-data"]);
    assert_eq!(out, include_str!("fixtures/uart-hi-ascii.ann"));
    let (out, _, _) = oxrok(&[
        "-i",
        VCD,
        "-P",
        "uart:rx=top.D0",
        "-A",
        "uart=rx-data",
        "--protocol-decoder-samplenum",
    ]);
    assert!(out.starts_with("29-99 uart-1: 48\n"), "{out}");
    let (out, _, _) =
        oxrok(&["-i", VCD, "-P", "uart", "-A", "uart=rx-start", "--protocol-decoder-ann-class"]);
    assert_eq!(out.lines().next(), Some("uart-1: rx-start: Start bit"));
}

#[test]
fn uart_binary_output() {
    let out = Command::new(env!("CARGO_BIN_EXE_oxrok"))
        .args(["-i", VCD, "-P", "uart", "-B", "uart=rx"])
        .output()
        .unwrap();
    assert_eq!(out.stdout, b"Hi!\n\x00\xff");
}

#[test]
fn decoder_listing_and_errors() {
    let (out, _, _) = oxrok(&["-L"]);
    assert!(out.contains(
        "Supported protocol decoders:\n  i2c                  Inter-Integrated Circuit\n"
    ));
    assert!(out.contains("  spi                  Serial Peripheral Interface\n  uart                 Universal Asynchronous Receiver/Transmitter\n"));
    let (out, _, ok) = oxrok(&["-P", "uart", "--show"]);
    assert!(ok);
    assert!(out.starts_with("ID: uart\nName: UART\n"));
    assert!(
        out.contains("- rx-data (RX): rx-data, rx-start, rx-parity-ok, rx-parity-err, rx-stop\n")
    );
    assert!(out.contains("- stop_bits: Stop bits (0.0, 0.5, 1.0, 1.5, default 1.0)\n"));
    let (_, err, ok) = oxrok(&["-i", VCD, "-P", "nosuch"]);
    assert!(!ok);
    assert!(err.contains("Protocol decoder 'nosuch' not found."), "{err}");
    let (_, err, ok) = oxrok(&["-i", VCD, "-P", "uart:parity=maybe"]);
    assert!(!ok);
    assert!(err.contains("Invalid value 'maybe'"), "{err}");
    let (_, err, ok) = oxrok(&["-i", VCD, "-P", "uart", "-A", "uart=nosuch"]);
    assert!(!ok);
    assert!(err.contains("Annotation class 'nosuch' not found"), "{err}");
}

#[test]
fn i2c_annotations_match_libsigrokdecode() {
    // Channels default to scl = first, sda = second, as in libsigrokdecode.
    let (out, _, ok) = oxrok(&["-l", "4", "-i", I2C, "-P", "i2c"]);
    assert!(ok);
    assert_eq!(out, include_str!("fixtures/i2c-write-read.ann"));
    let (out, _, _) =
        oxrok(&["-i", I2C, "-P", "i2c", "-A", "i2c=address-write:data-write:data-read"]);
    assert_eq!(
        out,
        "i2c-1: Write\ni2c-1: Address write: 51\ni2c-1: Data write: 55\ni2c-1: Data write: AA\ni2c-1: Data read: 0F\n"
    );
    let (out, _, _) = oxrok(&["-i", I2C, "-P", "i2c", "-M", "i2c"]);
    assert_eq!(out, "i2c-1: Bitrate: 88028\ni2c-1: Bitrate: 87628\n");
    let out = Command::new(env!("CARGO_BIN_EXE_oxrok"))
        .args(["-i", I2C, "-P", "i2c", "-B", "i2c=data-write"])
        .output()
        .unwrap();
    assert_eq!(out.stdout, [0x55, 0xaa]);
}

#[test]
fn spi_annotations_match_libsigrokdecode() {
    let (out, _, ok) = oxrok(&["-l", "4", "-i", SPI, "-P", "spi"]);
    assert!(ok);
    assert_eq!(out, include_str!("fixtures/spi-mode0.ann"));
    let (out, _, _) = oxrok(&["-i", SPI, "-P", "spi", "-A", "spi=mosi-transfer:miso-transfer"]);
    assert_eq!(out, "spi-1: 3C FF\nspi-1: A5 01\nspi-1: 7E\nspi-1: 80\n");
    let (out, _, _) =
        oxrok(&["-i", SPI, "-P", "spi:bitorder=lsb-first:wordsize=4", "-A", "spi=mosi-data"]);
    assert_eq!(out.lines().next(), Some("spi-1: 05"));
    let out = Command::new(env!("CARGO_BIN_EXE_oxrok"))
        .args(["-i", SPI, "-P", "spi", "-B", "spi=miso"])
        .output()
        .unwrap();
    assert_eq!(out.stdout, [0x3c, 0xff, 0x7e]);
}
