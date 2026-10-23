//! The driver against hand written serial fixtures: packets built from
//! the chipset layouts, with the byte stream starting mid packet as a
//! real cable does.

use oxrok_core::session::Flow;
use oxrok_core::{Device, Key, Mq, MqFlags, Packet, Session, Unit, Value};
use oxrok_driver_serial_dmm::{DmmDevice, detect, models};
use oxrok_serial::record::Replay;
use std::time::Duration;

/// Readings (channel, value, mq, unit, flags, digits) from one run.
type Readings = Vec<(usize, f32, Mq, Unit, MqFlags, i32)>;

fn run(model: &str, fixture: &str, limit: u64) -> Readings {
    let model = models::find(model).unwrap();
    let replay = Replay::from_text(fixture).unwrap();
    let mut dev = DmmDevice::from_port(model, Box::new(replay));
    dev.open().unwrap();
    dev.set(&Key::LimitSamples, &Value::U64(limit), None).unwrap();
    let mut session = Session::new();
    session.add_device(Box::new(dev));
    let mut out = Readings::new();
    session
        .run(|_, p| {
            if let Packet::Analog(a) = p {
                out.push((a.channels[0], a.samples[0], a.mq, a.unit, a.flags, a.digits));
            }
            Flow::Continue
        })
        .unwrap();
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A UT61E (ES519xx, 19200 baud, 14 bytes) packet: range, 5 digits,
/// function, status, four option bytes, CR LF.
fn ut61e(range: u8, digits: &str, function: u8, status: u8, opt3: u8) -> Vec<u8> {
    let mut p = vec![b'0' + range];
    p.extend_from_slice(digits.as_bytes());
    p.extend([function, 0x30 | status, 0x30, 0x30, 0x30 | opt3, 0x30, b'\r', b'\n']);
    p
}

#[test]
fn ut61e_stream_mid_packet() {
    // 12.345 V DC auto, then -0.100 V, with the stream joining half way
    // through a packet and the bytes arriving in odd chunks.
    let a = ut61e(1, "12345", 0x3b, 0, 8 | 2);
    let b = ut61e(1, "00100", 0x3b, 4, 8 | 2);
    let mut stream = a[7..].to_vec();
    stream.extend(&a);
    stream.extend(&b);
    stream.extend(&a);
    let fixture = format!(
        "# oxrok serial fixture 1\nport /dev/ttyUSB0 19200/7o1/rts=0/dtr=1\nread 128 -> {}\nread 128 -> -\nread 128 -> {}\nread 128 -> {}\n",
        hex(&stream[..10]),
        hex(&stream[10..30]),
        hex(&stream[30..])
    );
    let out = run("uni-t-ut61e-ser", &fixture, 3);
    assert_eq!(out.len(), 3);
    let (ch, v, mq, unit, flags, digits) = out[0];
    assert_eq!((ch, mq, unit, digits), (0, Mq::Voltage, Unit::Volt, 3));
    assert!((v - 12.345).abs() < 1e-4);
    assert!(flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
    assert!((out[1].1 + 0.1).abs() < 1e-5);
    assert!((out[2].1 - 12.345).abs() < 1e-4);
}

#[test]
fn metex_requests_and_four_displays() {
    // A four display Metex: the driver sends 'D', the meter answers
    // four 14 byte packets. The time limit ends the run.
    let mut four = Vec::new();
    for line in ["DC  1.000 V  \r", "AC  0.500 V  \r", "OH  1.234KOhm\r", "FR 12.34  KHz\r"] {
        four.extend_from_slice(line.as_bytes());
    }
    let fixture = format!(
        "# oxrok serial fixture 1\nport p 9600/7n2/rts=0/dtr=1\nwrite 44\nread 128 -> {}\nwrite 44\nread 128 -> -\n",
        hex(&four)
    );
    let out = run("metex-m3860m", &fixture, 1);
    assert_eq!(out.len(), 4);
    assert_eq!(out[2].0, 2);
    assert_eq!(out[2].2, Mq::Resistance);
    assert!((out[2].1 - 1234.0).abs() < 1e-2);
    assert_eq!(out[3].2, Mq::Frequency);
}

#[test]
fn detection_finds_a_packet_in_noise() {
    let p = ut61e(0, "00000", 0x33, 0, 2);
    let mut stream = vec![0x00, 0xff, 0x12];
    stream.extend(&p);
    let fixture = format!(
        "# oxrok serial fixture 1\nport p 19200/7o1\n{}read 1 -> -\n",
        stream.iter().map(|b| format!("read 1 -> {b:02x}\n")).collect::<String>()
    );
    let mut port = Replay::from_text(&fixture).unwrap();
    let model = models::find("uni-t-ut61e-ser").unwrap();
    assert!(detect(&mut port, &model.protocol, Duration::from_secs(1)).unwrap());
    let fixture = "# oxrok serial fixture 1\nport p 19200/7o1\nread 1 -> 00\nread 1 -> -\n";
    let mut port = Replay::from_text(fixture).unwrap();
    // The replay runs out of data: that is an error, not a detection.
    assert!(detect(&mut port, &model.protocol, Duration::from_millis(300)).is_err());
}
