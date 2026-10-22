//! The driver against a hand written session of a Saleae Logic clone
//! with the firmware running: open, version checks, a start command
//! for 1 MHz and three transfers of samples.

use oxrok_core::session::Flow;
use oxrok_core::{Device, Key, Packet, Session, Trigger, TriggerMatch, Value};
use oxrok_driver_fx2lafw::Fx2lafwDevice;
use oxrok_usb::record::Replay;

const SESSION: &str = "# oxrok usb fixture 1\n\
    device vid=0925 pid=3881 version=0001 bus=1 addr=4 manufacturer=\"sigrok\" product=\"fx2lafw\"\n\
    config 1 -> ok\n\
    claim 0 -> ok\n\
    ctl_in 40 b0 0000 0000 2 -> 0104\n\
    ctl_in 40 b2 0000 0000 1 -> 01\n\
    ctl_out 40 b1 0000 0000 40002f -> ok\n\
    bulk_in 82 10240 -> 00{5000}01{5240}\n\
    bulk_in 82 10240 -> ff{10240}\n\
    bulk_in 82 10240 -> aa{10240}\n";

fn capture(trigger: Option<Trigger>, limit: u64, ratio: u64) -> (Vec<u8>, Option<usize>) {
    let replay = Replay::from_text(SESSION).unwrap();
    let mut dev = Fx2lafwDevice::from_transport(Box::new(replay)).unwrap();
    dev.open().unwrap();
    assert_eq!(dev.info().version, "1.4");
    dev.set(&Key::Samplerate, &Value::U64(1_000_000), None).unwrap();
    dev.set(&Key::LimitSamples, &Value::U64(limit), None).unwrap();
    dev.set(&Key::CaptureRatio, &Value::U64(ratio), None).unwrap();
    let mut session = Session::new();
    session.add_device(Box::new(dev));
    session.set_trigger(trigger);
    let mut data = Vec::new();
    let mut at = None;
    session
        .run(|_, p| {
            match p {
                Packet::Trigger => at = Some(data.len()),
                Packet::Logic(l) => {
                    assert_eq!(l.unit_size, 1);
                    data.extend_from_slice(&l.data);
                    if data.len() as u64 >= limit {
                        return Flow::Stop;
                    }
                }
                _ => {}
            }
            Flow::Continue
        })
        .unwrap();
    for mut d in session.into_devices() {
        d.close().unwrap();
    }
    (data, at)
}

#[test]
fn samples_stream_until_the_limit() {
    let (data, at) = capture(None, 20_000, 0);
    assert_eq!(data.len(), 20_000);
    assert_eq!(at, None);
    assert_eq!(&data[4_999..5_001], [0, 1]);
    assert_eq!(data[10_240], 0xff);
}

#[test]
fn software_trigger_keeps_the_pre_trigger_window() {
    let mut t = Trigger::new();
    t.add(0, 0, TriggerMatch::Rising);
    let (data, at) = capture(Some(t), 12_000, 50);
    // 6000 samples of pre-trigger window were asked for, 5000 exist.
    assert_eq!(at, Some(5_000));
    assert_eq!(data.len(), 12_000);
    assert!(data[..5_000].iter().all(|&b| b == 0));
    assert!(data[5_000..10_240].iter().all(|&b| b == 1));
    assert!(data[10_240..].iter().all(|&b| b == 0xff));
    // A smaller pre-trigger window drops the oldest samples.
    let mut t = Trigger::new();
    t.add(0, 0, TriggerMatch::Rising);
    let (data, at) = capture(Some(t), 1_000, 10);
    assert_eq!(at, Some(100));
    assert_eq!(data.len(), 1_000);
}

#[test]
fn a_wrong_rate_fails_before_the_device_is_touched() {
    let replay = Replay::from_text(SESSION).unwrap();
    let mut dev = Fx2lafwDevice::from_transport(Box::new(replay)).unwrap();
    assert!(dev.set(&Key::Samplerate, &Value::U64(7_000_000), None).is_err());
    dev.set(&Key::Samplerate, &Value::U64(48_000_000), None).unwrap();
    dev.open().unwrap();
    let (tx, _rx) = std::sync::mpsc::sync_channel(4);
    let e = dev.start(oxrok_core::PacketSink::new(tx, 0)).unwrap_err().to_string();
    assert!(e.contains("maximum sample rate"), "{e}");
}

/// A session recorded on a Saleae Logic clone (firmware 1.4) with
/// `OXROK_USB_RECORD`, 2026-10-22, inputs floating high.
#[test]
fn recorded_capture_replays() {
    let replay = Replay::from_text(include_str!("fixtures/capture-1m.txt")).unwrap();
    let mut dev = Fx2lafwDevice::from_transport(Box::new(replay)).unwrap();
    dev.open().unwrap();
    dev.set(&Key::Samplerate, &Value::U64(1_000_000), None).unwrap();
    dev.set(&Key::LimitSamples, &Value::U64(2000), None).unwrap();
    let mut session = Session::new();
    session.add_device(Box::new(dev));
    let mut data = Vec::new();
    session
        .run(|_, p| {
            if let Packet::Logic(l) = p {
                data.extend_from_slice(&l.data);
                if data.len() >= 2000 {
                    return Flow::Stop;
                }
            }
            Flow::Continue
        })
        .unwrap();
    assert_eq!(data.len(), 2000);
    assert!(data.iter().all(|&b| b == 0xff));
    // The bits output printed during the recording agrees.
    let bits = include_str!("fixtures/capture-1m.bits");
    let d0 = bits.lines().find(|l| l.starts_with("D0:")).unwrap();
    assert!(d0.chars().skip(3).all(|c| c == '1' || c == ' '), "{d0}");
}
