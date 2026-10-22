//! Acquisition runs, `--get` and `--set`.

use crate::args::Args;
use crate::parse;
use oxrok_core::session::Flow;
use oxrok_core::util::{parse_sizestring, parse_timestring};
use oxrok_core::{
    Device, DeviceClass, Error, Key, Packet, Registry, Result, Session, Value, ValueList,
};
use oxrok_formats::output::{self, Options, Output, OutputContext};
use std::io::Write;
use std::sync::atomic::Ordering;

const DEFAULT_OUTPUT_NOFILE: &str = "bits:width=64";
const DEFAULT_OUTPUT_FILE: &str = "srzip";

/// Scans and picks the one device to work with, the way sigrok-cli does:
/// with several devices found, the demo device is dropped and exactly one
/// real device must remain.
fn pick_device(registry: &Registry, args: &Args) -> Result<Box<dyn Device>> {
    let devices = parse::device_scan(registry, args)?;
    if devices.is_empty() {
        return Err(Error::arg("No devices found."));
    }
    let mut devices = devices;
    if devices.len() > 1 {
        let real: Vec<Box<dyn Device>> =
            devices.into_iter().filter(|d| !d.classes().contains(&DeviceClass::Demo)).collect();
        if real.len() != 1 {
            return Err(Error::arg("oxrok only supports one device for capturing."));
        }
        devices = real;
    }
    let mut device = devices.remove(0);
    device.open().map_err(|e| Error::Arg(format!("Failed to open device: {e}.")))?;
    Ok(device)
}

/// `--get`.
pub fn get_options(registry: &Registry, args: &Args) -> Result<()> {
    let mut device = pick_device(registry, args)?;
    parse::apply_configs(device.as_mut(), args)?;
    for get in &args.gets {
        let parsed = parse::generic_args(get, false);
        let group = parsed.scope.or_else(|| args.channel_group.clone());
        for (name, _) in parsed.items {
            let key = parse::resolve_key(device.as_ref(), &name, group.as_deref())?;
            let value = device
                .get(&key, group.as_deref())
                .map_err(|e| Error::Arg(format!("Failed to get '{name}': {e}")))?;
            println!("{value}");
        }
    }
    device.close()
}

/// `--set`.
pub fn set_options(registry: &Registry, args: &Args) -> Result<()> {
    if args.configs.is_empty() {
        return Err(Error::arg("No setting specified."));
    }
    let mut device = pick_device(registry, args)?;
    parse::apply_configs(device.as_mut(), args)?;
    device.close()
}

fn has_option(device: &dyn Device, key: &Key) -> bool {
    device.option(key, None).is_some()
}

fn set_limit_time(device: &mut dyn Device, text: &str) -> Result<u64> {
    let msec = parse_timestring(text)
        .filter(|t| *t > 0)
        .ok_or_else(|| Error::Arg(format!("Invalid time '{text}'")))?;
    if device.option(&Key::LimitMsec, None).is_some_and(|o| o.access.set) {
        device
            .set(&Key::LimitMsec, &Value::U64(msec), None)
            .map_err(|_| Error::arg("Failed to configure time limit."))?;
        return Ok(0);
    }
    if device.option(&Key::Samplerate, None).is_some_and(|o| o.access.get) {
        let rate = device.get(&Key::Samplerate, None)?.as_u64().unwrap_or(0);
        let samples = rate * msec / 1000;
        if samples == 0 {
            return Err(Error::arg("Not enough time at this samplerate."));
        }
        device
            .set(&Key::LimitSamples, &Value::U64(samples), None)
            .map_err(|_| Error::arg("Failed to configure time-based sample limit."))?;
        return Ok(samples);
    }
    Err(Error::arg("This device does not support time limits."))
}

/// Builds the output module named by `-O`, or the default.
type Outputs = (Box<dyn Output>, Option<Box<dyn Output>>);

fn setup_output(args: &Args, ctx: &OutputContext) -> Result<Outputs> {
    let spec = args.output_format.clone().unwrap_or_else(|| {
        if args.output_file.is_some() { DEFAULT_OUTPUT_FILE } else { DEFAULT_OUTPUT_NOFILE }
            .to_owned()
    });
    let parsed = parse::generic_args(&spec, true);
    let id = parsed.scope.ok_or_else(|| Error::arg("Invalid output format."))?;
    let module =
        output::find(&id).ok_or_else(|| Error::Arg(format!("Unknown output module '{id}'.")))?;
    let options: Options =
        parsed.items.into_iter().map(|(k, v)| (k, v.unwrap_or_default())).collect();
    let main = module.instantiate(ctx, &options)?;
    // Analog data that the main output does not print goes to the
    // analog output, as in sigrok-cli.
    let analog = if module.id == "analog" {
        None
    } else {
        output::find("analog").map(|m| m.instantiate(ctx, &Options::new())).transpose()?
    };
    Ok((main, analog))
}

/// A capture run: `--samples`, `--time`, `--frames` or `--continuous`.
#[allow(clippy::too_many_lines)]
pub fn session(registry: &Registry, args: &Args) -> Result<()> {
    let mut device = pick_device(registry, args)?;
    parse::apply_configs(device.as_mut(), args)?;
    if let Some(spec) = &args.channels {
        parse::select_channels(device.as_mut(), spec)
            .map_err(|e| Error::Arg(format!("Failed to set channels: {e}")))?;
    }
    let trigger = match &args.triggers {
        Some(spec) => Some(parse::parse_triggers(device.as_ref(), spec)?),
        None => None,
    };
    if args.continuous && !has_option(device.as_ref(), &Key::Continuous) {
        return Err(Error::arg("This device does not support continuous sampling."));
    }
    let mut limit_samples = 0u64;
    if let Some(t) = &args.time {
        limit_samples = set_limit_time(device.as_mut(), t)?;
    }
    if let Some(s) = &args.samples {
        limit_samples =
            parse_sizestring(s).map_err(|_| Error::Arg(format!("Invalid sample limit '{s}'.")))?;
        if let Ok(ValueList::MinMaxU64(min, max)) = device.list(&Key::LimitSamples, None) {
            if limit_samples < min {
                return Err(Error::Arg(format!(
                    "The device stores at least {min} samples with the current settings."
                )));
            }
            if limit_samples > max {
                return Err(Error::Arg(format!(
                    "The device can store only {max} samples with the current settings."
                )));
            }
        }
        device
            .set(&Key::LimitSamples, &Value::U64(limit_samples), None)
            .map_err(|_| Error::arg("Failed to configure sample limit."))?;
    }
    if let Some(f) = &args.frames {
        let frames =
            parse_sizestring(f).map_err(|_| Error::Arg(format!("Invalid frame limit '{f}'.")))?;
        device
            .set(&Key::LimitFrames, &Value::U64(frames), None)
            .map_err(|_| Error::arg("Failed to configure frame limit."))?;
    }

    let samplerate = device.get(&Key::Samplerate, None).ok().and_then(|v| v.as_u64());
    let channels = device.channels().to_vec();
    let ctx = OutputContext { channels: &channels, samplerate };
    let (mut main, mut analog) = setup_output(args, &ctx)?;

    let mut sink: Box<dyn Write> = match &args.output_file {
        Some(path) => {
            Box::new(std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| {
                Error::Arg(format!("Cannot write to output file '{}': {e}", path.display()))
            })?))
        }
        None => Box::new(std::io::stdout()),
    };

    let mut session = Session::new();
    session.add_device(device);
    session.set_trigger(trigger);
    let stop = session.stop_flag();
    let _ = ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst));

    let mut received_logic = 0u64;
    let mut received_analog = 0u64;
    let mut triggered = false;
    let mut buf = Vec::new();
    let wait_trigger = args.wait_trigger;
    let result = session.run(|_, packet| {
        match packet {
            Packet::Trigger => triggered = true,
            Packet::Logic(l) => received_logic += l.len() as u64,
            Packet::Analog(a) => received_analog += a.len() as u64,
            _ => {}
        }
        if wait_trigger && !triggered && matches!(packet, Packet::Logic(_)) {
            return Flow::Continue;
        }
        buf.clear();
        if main.receive(packet, &mut buf).is_ok() {
            if buf.is_empty()
                && let Some(a) = &mut analog
            {
                let _ = a.receive(packet, &mut buf);
            }
            if !buf.is_empty() && (sink.write_all(&buf).is_err() || sink.flush().is_err()) {
                return Flow::Stop;
            }
        }
        Flow::Continue
    });
    let devices = session.into_devices();
    for mut d in devices {
        let _ = d.close();
    }
    result?;
    if limit_samples > 0 {
        if received_logic > 0 && received_logic < limit_samples {
            log::warn!("Device only sent {received_logic} samples.");
        } else if received_analog > 0 && received_analog < limit_samples {
            log::warn!("Device only sent {received_analog} samples.");
        }
    }
    Ok(())
}
