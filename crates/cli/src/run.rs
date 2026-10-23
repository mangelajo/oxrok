// Counterpart in sigrok:
//   sigrok-cli/session.c,
//   Copyright (C) 2013 Bert Vermeulen <bert@biot.com>,
//   with commits by Uwe Hermann, Gerhard Sittig, Bartosz Golaszewski,
//   Daniel Elstner, Martin Ling, Soeren Apel, Aleksander Alekseev,
//   Aurelien Jacobs, Devan Lai, Guido Trentalancia, Jens Steinhauser,
//   Peter Zotov, Vincent Palatin, poljar (Damir Jelić)
//   (sigrok-cli sigrok-cli-unreleased-103-gf44dd91).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! Acquisition runs, `--get` and `--set`.

use crate::args::Args;
use crate::parse;
use crate::pipeline::Pipeline;
use oxrok_core::session::Flow;
use oxrok_core::util::{parse_sizestring, parse_timestring};
use oxrok_core::{Device, DeviceClass, Error, Key, Registry, Result, Session, Value, ValueList};
use std::sync::atomic::Ordering;

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
    let title = registry.find(device.info().driver).map_or("unknown", |d| d.long_name()).to_owned();
    let mut pipeline = Pipeline::new(args, &channels, samplerate, &title)?;

    let mut session = Session::new();
    session.add_device(device);
    session.set_trigger(trigger);
    let stop = session.stop_flag();
    let _ = ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst));

    let result =
        session.run(|_, packet| if pipeline.handle(packet) { Flow::Continue } else { Flow::Stop });
    let devices = session.into_devices();
    for mut d in devices {
        let _ = d.close();
    }
    result?;
    pipeline.check_limit(limit_samples);
    Ok(())
}
