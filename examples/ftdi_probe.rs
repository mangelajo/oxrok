//! Opens an FTDI device and exercises the vendor requests that do not
//! drive any pin. Use it to check that the USB backend can claim the
//! device on this machine.
//!
//! ```text
//! cargo run -p oxrok-ftdi --example probe -- [conn] [--record FILE]
//! ```
//!
//! `conn` is `bus.address` or `vid.pid` in hex; without it the first
//! FTDI device is used.

use oxrok::ftdi::{BitMode, Ftdi, Port, VENDOR_ID};
use oxrok::usb::record::Recorder;
use std::time::Duration;

fn main() {
    let mut conn = None;
    let mut record = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--record" {
            record = args.next();
        } else {
            conn = Some(a);
        }
    }
    if let Err(e) = run(conn.as_deref(), record.as_deref()) {
        eprintln!("probe failed: {e}");
        std::process::exit(1);
    }
}

fn run(conn: Option<&str>, record: Option<&str>) -> oxrok::core::Result<()> {
    let backend = oxrok::usb::Backend::current().map_or("none", |b| b.name());
    let devices = oxrok::usb::list_devices()?;
    let info = devices
        .iter()
        .find(|d| {
            conn.is_none_or(|c| d.matches_conn(c)) && (conn.is_some() || d.vendor_id == VENDOR_ID)
        })
        .ok_or_else(|| oxrok::core::Error::device("no matching device"))?;
    println!(
        "backend {backend}: {} at {} {} {} {}",
        info.id(),
        info.conn(),
        info.manufacturer.as_deref().unwrap_or("-"),
        info.product.as_deref().unwrap_or("-"),
        info.serial.as_deref().unwrap_or("-")
    );
    let mut transport = oxrok::usb::open(info)?;
    let mut recording = None;
    if record.is_some() {
        let (rec, handle) = Recorder::with_handle(transport);
        transport = Box::new(rec);
        recording = Some(handle);
    }
    let mut f = Ftdi::open(transport, Port::A)?;
    println!("chip {}, max packet {}", f.chip().name(), f.max_packet_size());
    f.reset()?;
    f.set_latency_timer(4)?;
    println!("latency timer {} ms", f.latency_timer()?);
    println!("baud 460800 -> {}", f.set_baudrate(460_800)?);
    println!("pins 0x{:02x}", f.read_pins()?);
    println!("modem status 0x{:04x}", f.modem_status()?);
    let words = f.chip().eeprom_words();
    let eeprom = f.read_eeprom(words)?;
    for (i, row) in eeprom.chunks(16).enumerate() {
        let hex: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
        let text: String =
            row.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
        println!("eeprom {:03x}: {} {text}", i * 16, hex.join(" "));
    }
    // Bit-bang with no outputs drives nothing; it only proves the
    // request works and shows the pin levels through that path.
    f.set_bitmode(0, BitMode::BitBang)?;
    println!("pins in bit-bang 0x{:02x}", f.read_pins()?);
    f.set_bitmode(0, BitMode::Reset)?;
    f.read_timeout = Duration::from_millis(200);
    let mut buf = [0u8; 4096];
    let n = f.read(&mut buf)?;
    println!("fifo read: {n} bytes");
    f.purge_read()?;
    if let (Some(path), Some(handle)) = (record, recording) {
        drop(f);
        std::fs::write(path, handle.fixture().to_text())?;
        println!("recorded to {path}");
    }
    Ok(())
}
