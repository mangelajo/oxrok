//! Hardware in the loop checks for the LogicPort driver.
//!
//! Runs captures through the library against a LogicPort while the ESP32
//! signal generator (`esp32/siggen` in the sigrok driver repository)
//! drives the inputs. Wiring: GPIO25 -> D0, GPIO26 -> D1, GPIO27 -> D2,
//! GPIO14 -> CLK1, GND -> GND, plus the wide map listed in `wide`.
//!
//! ```text
//! oxrok-hil [-k substring] [--conn bus.addr] [--port /dev/cu.usbserial-0001]
//! ```
//!
//! Prints one PASS/FAIL line per check and exits non-zero on a failure.

#![forbid(unsafe_code)]
#![allow(clippy::doc_markdown, clippy::many_single_char_names, clippy::too_many_lines)]

mod siggen;

use oxrok_core::session::Flow;
use oxrok_core::{Device, Driver, Key, Packet, Session, Value};
use oxrok_driver_logicport::LogicPortDriver;
use siggen::SigGen;
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::Duration;

const D0: u32 = 1;
const D1: u32 = 2;
/// CLK1 in the default selection D0, D1, D2, CLK1.
const CLK1: u64 = 8;
const ALL_CHANNELS: &str = "all";

struct Args {
    filter: String,
    conn: Option<String>,
    port: Option<String>,
    /// Only drive the generator (`--clk HZ`, `--set MASK`, `--cmd LINE`,
    /// `--delay MS` between them) and exit. Opening the serial port
    /// resets the ESP32, so commands that belong together go in one
    /// invocation.
    setup: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        filter: String::new(),
        conn: std::env::var("LOGICPORT_CONN").ok(),
        port: None,
        setup: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "-k" => a.filter = it.next().unwrap_or_default(),
            "--conn" => a.conn = it.next(),
            "--port" => a.port = it.next(),
            "--clk" | "--set" | "--cmd" | "--delay" => {
                a.setup.push(x.clone());
                a.setup.push(it.next().unwrap_or_default());
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    a
}

/// One capture's result: sample values (bit i = channel i) and the
/// trigger index, per frame.
#[derive(Clone, Debug, Default)]
struct Frame {
    values: Vec<u64>,
    trigger: Option<usize>,
}

struct Capture {
    conn: Option<String>,
    frames: Vec<Frame>,
}

/// What one capture asks for.
#[derive(Clone, Default)]
struct Req<'a> {
    config: &'a str,
    samples: Option<u64>,
    time_ms: Option<u64>,
    frames: Option<u64>,
    triggers: Option<&'a str>,
    channels: Option<&'a str>,
}

impl Capture {
    fn device(&self) -> Result<Box<dyn Device>, String> {
        let mut opts = Vec::new();
        if let Some(c) = &self.conn {
            opts.push((Key::Conn, Value::Str(c.clone())));
        }
        let mut devices = LogicPortDriver.scan(&opts).map_err(|e| e.to_string())?;
        if devices.is_empty() {
            return Err("no LogicPort found".into());
        }
        Ok(devices.remove(0))
    }

    /// Runs one acquisition. `during` runs once the device is armed.
    /// Returns the first frame.
    fn run(&mut self, req: &Req<'_>, during: Option<&mut dyn FnMut()>) -> Result<Frame, String> {
        let mut dev = self.device()?;
        dev.open().map_err(|e| format!("open: {e}"))?;
        for item in req.config.split(':').filter(|s| !s.is_empty()) {
            let (k, v) = item.split_once('=').ok_or_else(|| format!("bad config {item}"))?;
            let key = oxrok_core::resolve_key(dev.as_ref(), k, None).map_err(|e| e.to_string())?;
            let value = Value::parse(key.kind(), v).map_err(|e| e.to_string())?;
            dev.set(&key, &value, None).map_err(|e| format!("set {k}: {e}"))?;
        }
        let channels: Vec<String> = dev.channels().iter().map(|c| c.name.clone()).collect();
        let wanted: Vec<&str> = match req.channels {
            None => vec!["D0", "D1", "D2", "CLK1"],
            Some(ALL_CHANNELS) => Vec::new(),
            Some(list) => list.split(',').collect(),
        };
        if !wanted.is_empty() {
            for (pos, name) in channels.iter().enumerate() {
                dev.enable_channel(pos, wanted.contains(&name.as_str()))
                    .map_err(|e| e.to_string())?;
            }
        }
        // Values are compacted to the enabled channels, bit i being the
        // i-th enabled one, as a CSV of those channels would show them.
        let selected: Vec<usize> =
            dev.channels().iter().filter(|c| c.enabled).map(|c| c.index).collect();
        let samples = match (req.time_ms, req.samples) {
            (Some(ms), _) => {
                let rate =
                    dev.get(&Key::Samplerate, None).ok().and_then(|v| v.as_u64()).unwrap_or(0);
                ms * rate / 1000
            }
            (None, s) => s.unwrap_or(2048),
        };
        dev.set(&Key::LimitSamples, &Value::U64(samples), None).map_err(|e| e.to_string())?;
        if let Some(f) = req.frames {
            dev.set(&Key::LimitFrames, &Value::U64(f), None).map_err(|e| e.to_string())?;
        }
        let trigger = match req.triggers {
            Some(spec) => Some(
                oxrok_core::trigger_syntax::parse(dev.as_ref(), spec).map_err(|e| e.to_string())?,
            ),
            None => None,
        };
        let mut session = Session::new();
        session.add_device(dev);
        session.set_trigger(trigger);
        let stop = session.stop_flag();
        let watchdog = std::thread::spawn({
            let stop = stop.clone();
            move || {
                for _ in 0..200 {
                    std::thread::sleep(Duration::from_millis(100));
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                }
                stop.store(true, Ordering::SeqCst);
            }
        });
        let mut frames: Vec<Frame> = Vec::new();
        let mut cur = Frame::default();
        let mut sent = 0u64;
        let limit = samples;
        let mut during = during;
        let result = session.run(|_, packet| {
            match packet {
                Packet::Header { .. } => {
                    if let Some(d) = during.as_mut() {
                        std::thread::sleep(Duration::from_millis(20));
                        d();
                    }
                }
                Packet::Trigger => {
                    if cur.trigger.is_none() {
                        cur.trigger = Some(cur.values.len());
                    }
                }
                Packet::Logic(l) => {
                    for s in l.data.chunks(l.unit_size) {
                        let mut raw = 0u64;
                        for (i, b) in s.iter().enumerate() {
                            raw |= u64::from(*b) << (8 * i);
                        }
                        let v = selected
                            .iter()
                            .enumerate()
                            .fold(0u64, |v, (i, &idx)| v | ((raw >> idx) & 1) << i);
                        cur.values.push(v);
                        sent += 1;
                    }
                    if req.frames.is_none() && limit > 0 && sent >= limit {
                        return Flow::Stop;
                    }
                }
                Packet::FrameEnd => {
                    frames.push(std::mem::take(&mut cur));
                }
                _ => {}
            }
            Flow::Continue
        });
        let timed_out = stop.load(Ordering::SeqCst)
            && result.is_ok()
            && frames.is_empty()
            && cur.values.is_empty();
        stop.store(true, Ordering::SeqCst);
        let _ = watchdog.join();
        for mut d in session.into_devices() {
            let _ = d.close();
        }
        result.map_err(|e| format!("session: {e}"))?;
        if timed_out {
            return Err("capture did not complete in 20 s".into());
        }
        if !cur.values.is_empty() || frames.is_empty() {
            frames.push(cur);
        }
        self.frames = frames;
        Ok(self.frames[0].clone())
    }
}

fn edges(v: &[u64], mask: u64, kind: &str) -> Vec<usize> {
    (1..v.len())
        .filter(|&i| {
            let a = v[i - 1] & mask != 0;
            let b = v[i] & mask != 0;
            match kind {
                "rise" => !a && b,
                "fall" => a && !b,
                _ => a != b,
            }
        })
        .collect()
}

/// (value, run length) of `v & mask`.
fn runs(v: &[u64], mask: u64) -> Vec<(u64, usize)> {
    let mut out: Vec<(u64, usize)> = Vec::new();
    for x in v {
        let x = x & mask;
        match out.last_mut() {
            Some((last, n)) if *last == x => *n += 1,
            _ => out.push((x, 1)),
        }
    }
    out
}

struct Report {
    results: Vec<(String, bool)>,
}

impl Report {
    fn ok(&mut self, name: &str, cond: bool, detail: &str) {
        println!("[{}] {name}  {detail}", if cond { "PASS" } else { "FAIL" });
        self.results.push((name.to_owned(), cond));
    }
}

fn periods(e: &[usize]) -> Vec<usize> {
    e.windows(2).map(|w| w[1] - w[0]).collect()
}

fn fmt_err(e: &Result<Frame, String>) -> String {
    match e {
        Ok(_) => String::new(),
        Err(e) => e.clone(),
    }
}

#[allow(clippy::too_many_lines)]
fn main() -> ExitCode {
    let args = parse_args();
    let want = |name: &str| name.contains(&args.filter);
    let mut g = match SigGen::open(args.port.as_deref()) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if !args.setup.is_empty() {
        for pair in args.setup.chunks(2) {
            let r = if pair[0] == "--cmd" {
                g.cmd(&pair[1], Duration::from_secs(30)).map(|reply| println!("{reply}"))
            } else if pair[0] == "--delay" {
                // Keeps the connection open, so the ESP32 is not reset
                // between the commands around it.
                std::thread::sleep(Duration::from_millis(pair[1].parse().unwrap_or(0)));
                Ok(())
            } else {
                let v: u32 = pair[1].parse().unwrap_or(0);
                if pair[0] == "--clk" { g.clk(v) } else { g.set(v) }
            };
            if let Err(e) = r {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }
    let mut c = Capture { conn: args.conn.clone(), frames: Vec::new() };
    let mut r = Report { results: Vec::new() };
    let thr = "voltage_threshold=1.5";
    let _ = g.clk(0);
    let _ = g.set(0);

    if want("static") {
        let mut bad = Vec::new();
        for m in 0..16u32 {
            let _ = g.set(m);
            let res = c.run(
                &Req {
                    config: &format!("samplerate=1M:{thr}"),
                    samples: Some(64),
                    ..Default::default()
                },
                None,
            );
            match &res {
                Ok(f) if f.values.len() == 64 && f.values.iter().all(|&x| x == u64::from(m)) => {}
                Ok(f) => {
                    bad.push(format!("{m}: {:?}", f.values.iter().take(4).collect::<Vec<_>>()));
                }
                Err(e) => bad.push(format!("{m}: {e}")),
            }
        }
        r.ok("static levels D0,D1,D2,CLK1 (16 combinations)", bad.is_empty(), &bad.join("; "));
        let _ = g.set(0);
    }

    if want("threshold") {
        let _ = g.set(15);
        let hi = c.run(
            &Req {
                config: "samplerate=1M:voltage_threshold=4.0",
                samples: Some(16),
                ..Default::default()
            },
            None,
        );
        let lo = c.run(
            &Req {
                config: "samplerate=1M:voltage_threshold=1.5",
                samples: Some(16),
                ..Default::default()
            },
            None,
        );
        let all_eq = |f: &Result<Frame, String>, v: u64| {
            f.as_ref().is_ok_and(|f| f.values.iter().all(|&x| x == v))
        };
        r.ok(
            "threshold 4.0 V reads 3.3 V logic as 0, 1.5 V as 1",
            all_eq(&hi, 0) && all_eq(&lo, 15),
            &format!("{}{}", fmt_err(&hi), fmt_err(&lo)),
        );
        let _ = g.set(0);
        let z = c.run(
            &Req {
                config: "samplerate=1M:voltage_threshold=-1.0",
                samples: Some(16),
                ..Default::default()
            },
            None,
        );
        r.ok("threshold -1.0 V reads 0 V as 1", all_eq(&z, 15), &fmt_err(&z));
    }

    if want("timebase") {
        let _ = g.clk(1_000_000);
        for (rate, per) in [("100M", 100usize), ("10M", 10), ("200M", 200), ("250M", 250)] {
            let res = c.run(
                &Req { config: &format!("samplerate={rate}:{thr}"), ..Default::default() },
                None,
            );
            let (ok, detail) = match &res {
                Ok(f) => {
                    let p = periods(&edges(&f.values, CLK1, "rise"));
                    let good = f.values.len() == 2048
                        && !p.is_empty()
                        && p.iter().all(|&x| x + 1 >= per && x <= per + 1);
                    (
                        good,
                        format!(
                            "{} samples, periods {:?}",
                            f.values.len(),
                            p.iter().min().zip(p.iter().max())
                        ),
                    )
                }
                Err(e) => (false, e.clone()),
            };
            r.ok(
                &format!("{rate} vs 1 MHz clock: {per} samples/period, 2048 samples"),
                ok,
                &detail,
            );
        }
        let _ = g.clk(10_000_000);
        let res =
            c.run(&Req { config: &format!("samplerate=500M:{thr}"), ..Default::default() }, None);
        match &res {
            Ok(f) => {
                let p = periods(&edges(&f.values, CLK1, "rise"));
                r.ok(
                    "500 MHz DDR vs 10 MHz clock: 50 +-1 samples/period",
                    f.values.len() >= 2040
                        && !p.is_empty()
                        && p.iter().all(|&x| (49..=51).contains(&x)),
                    &format!(
                        "{} samples, periods {:?}",
                        f.values.len(),
                        p.iter().min().zip(p.iter().max())
                    ),
                );
                let rr = runs(&f.values, CLK1);
                let hi: Vec<usize> = rr[1..rr.len().saturating_sub(1)]
                    .iter()
                    .filter(|(x, _)| *x != 0)
                    .map(|(_, n)| *n)
                    .collect();
                r.ok(
                    "500 MHz DDR: high time 25 +-1 samples (no bank misalignment)",
                    !hi.is_empty() && hi.iter().all(|&x| (24..=26).contains(&x)),
                    &format!("{:?}", hi.iter().min().zip(hi.iter().max())),
                );
            }
            Err(e) => {
                r.ok("500 MHz DDR vs 10 MHz clock", false, e);
                r.ok("500 MHz DDR: high time", false, e);
            }
        }
        let _ = g.clk(0);
        let _ = g.set(0);
    }

    // 10 us period on D0.
    let pulses: Vec<(u32, u32)> = [(D0, 5000), (0, 5000)].repeat(30);

    if want("trigger") {
        for (rate, ratio) in [("1M", 50u64), ("10M", 20), ("100M", 80)] {
            let mut during = || {
                let _ = g.seq(&pulses, 1);
            };
            let res = c.run(
                &Req {
                    config: &format!("samplerate={rate}:captureratio={ratio}:{thr}"),
                    triggers: Some("D0=r"),
                    ..Default::default()
                },
                Some(&mut during),
            );
            match &res {
                Ok(f) => {
                    let e = edges(&f.values, D0.into(), "rise");
                    let t = f.trigger;
                    let before: Vec<usize> =
                        e.iter().copied().filter(|&x| x <= t.unwrap_or(0)).collect();
                    let off = t.and_then(|t| before.last().map(|&b| t as i64 - b as i64));
                    r.ok(
                        &format!("rising edge trigger @ {rate}: marker on the edge"),
                        off == Some(0),
                        &format!("trigger {t:?}, offset {off:?}"),
                    );
                    r.ok(
                        &format!(
                            "capture ratio {ratio}% @ {rate}: trigger near {ratio}% of buffer"
                        ),
                        t.is_some_and(|t| {
                            (t as i64 - (f.values.len() as u64 * ratio / 100) as i64).abs() < 16
                        }),
                        &format!("trigger {t:?} of {}", f.values.len()),
                    );
                }
                Err(e) => {
                    r.ok(&format!("rising edge trigger @ {rate}"), false, e);
                    r.ok(&format!("capture ratio {ratio}% @ {rate}"), false, e);
                }
            }
        }
        let mut during = || {
            let _ = g.seq(&pulses, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D0=f"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.trigger.is_some_and(|t| edges(&f.values, D0.into(), "fall").contains(&t))
        });
        r.ok(
            "falling edge trigger: marker on the edge",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );

        let mut seq = vec![(0, 20000)];
        seq.extend_from_slice(&pulses);
        let mut during = || {
            let _ = g.seq(&seq, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D0=e"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.trigger.is_some_and(|t| edges(&f.values, D0.into(), "any").contains(&t))
        });
        r.ok(
            "either-edge trigger: marker on an edge",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );

        // Pattern: D0..D2 count 0..7 in 10 us steps, trigger on D2=1,D1=1,D0=0 (value 6).
        let count: Vec<(u32, u32)> = (0..8u32).map(|m| (m, 10000)).collect::<Vec<_>>().repeat(3);
        let mut during = || {
            let _ = g.seq(&count, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D0=0,D1=1,D2=1"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.trigger.is_some_and(|t| t > 0 && f.values[t] & 7 == 6 && f.values[t - 1] & 7 != 6)
        });
        r.ok(
            "pattern trigger D2..D0=110: marker where the pattern starts",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );

        // Edge qualified by a pattern: D0 rising while D1=1.
        let mut seq = vec![(0u32, 20000u32)];
        seq.extend([(D0, 5000), (0, 5000)].repeat(4));
        seq.push((D1, 5000));
        seq.extend([(D1 | D0, 5000), (D1, 5000)].repeat(4));
        seq.push((0, 20000));
        let mut during = || {
            let _ = g.seq(&seq, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D0=r,D1=1"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.trigger.is_some_and(|t| {
                t > 0 && f.values[t] & 2 != 0 && f.values[t] & 1 != 0 && f.values[t - 1] & 1 == 0
            })
        });
        r.ok(
            "edge + pattern trigger (D0 rising while D1=1)",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );

        // Two stages: level A (D0 rising) then level B (D1 rising).
        let mut seq = vec![(0u32, 20000u32)];
        seq.extend([(D1, 5000), (0, 5000)].repeat(3));
        seq.extend([(D0, 5000), (0, 5000)].repeat(3));
        seq.extend([(D1, 5000), (0, 5000)].repeat(3));
        seq.push((0, 20000));
        let mut during = || {
            let _ = g.seq(&seq, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D0=r;D1=r"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.trigger.is_some_and(|t| {
                edges(&f.values, D1.into(), "rise").contains(&t)
                    && edges(&f.values[..t], D0.into(), "rise").len() >= 3
            })
        });
        r.ok(
            "two stage trigger: D0 rising then D1 rising",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );

        let _ = g.clk(100_000);
        let res = c.run(
            &Req {
                config: &format!("samplerate=10M:{thr}"),
                triggers: Some("CLK1=r"),
                ..Default::default()
            },
            None,
        );
        let ok = res
            .as_ref()
            .is_ok_and(|f| f.trigger.is_some_and(|t| edges(&f.values, CLK1, "rise").contains(&t)));
        r.ok(
            "trigger on CLK1 rising edge",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );
        let _ = g.clk(0);

        let mut during = || {
            let _ = g.seq(&pulses, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:captureratio=25:{thr}"),
                samples: Some(200),
                triggers: Some("D0=r"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.values.len() == 200
                && f.trigger == Some(50)
                && edges(&f.values, D0.into(), "rise").contains(&50)
        });
        r.ok(
            "limit 200 samples, ratio 25%: trigger at sample 50",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| {
                format!("{} samples, trigger {:?}", f.values.len(), f.trigger)
            }),
        );

        let _ = g.clk(10_000_000);
        let res = c.run(
            &Req {
                config: &format!("samplerate=500M:{thr}"),
                triggers: Some("CLK1=r"),
                ..Default::default()
            },
            None,
        );
        let near = res.as_ref().ok().and_then(|f| {
            f.trigger.and_then(|t| {
                edges(&f.values, CLK1, "rise").iter().map(|&x| (x as i64 - t as i64).abs()).min()
            })
        });
        r.ok(
            "500 MHz rising edge trigger within 2 samples of the edge",
            near.is_some_and(|n| n <= 2),
            &format!("distance {near:?}"),
        );
        let _ = g.clk(0);
    }

    if want("rle") {
        // 8 steps of 1 ms at 1 MHz = 8000 samples, more than the 2048 entry memory.
        let steps: Vec<(u32, u32)> =
            (0..8u32).map(|m| (m, 1_000_000)).collect::<Vec<_>>().repeat(2);
        let _ = g.set(0);
        let mut during = || {
            let _ = g.seq(&steps, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:rle=on:{thr}"),
                samples: Some(12000),
                triggers: Some("D0=1,D1=1,D2=1"),
                ..Default::default()
            },
            Some(&mut during),
        );
        match &res {
            Ok(f) => {
                let rr: Vec<(u64, usize)> =
                    runs(&f.values, 7).into_iter().filter(|x| x.1 > 1).collect();
                let inner: Vec<usize> =
                    rr[1..rr.len().saturating_sub(1)].iter().map(|x| x.1).collect();
                r.ok(
                    "RLE @ 1 MHz: 1 ms steps expand to 1000 samples",
                    f.values.len() > 2048
                        && !inner.is_empty()
                        && inner.iter().all(|&n| (995..=1005).contains(&n)),
                    &format!("{} samples, runs {:?}", f.values.len(), &rr[..rr.len().min(6)]),
                );
                r.ok(
                    "RLE: trigger at the start of value 7",
                    f.trigger
                        .is_some_and(|t| t > 0 && f.values[t] & 7 == 7 && f.values[t - 1] & 7 != 7),
                    &format!("trigger {:?}", f.trigger),
                );
            }
            Err(e) => {
                r.ok("RLE @ 1 MHz", false, e);
                r.ok("RLE: trigger", false, e);
            }
        }
        for (rate, step) in [("10M", 50usize), ("1M", 5)] {
            let mut during = || {
                let _ = g.seq(&pulses, 1);
            };
            let res = c.run(
                &Req {
                    config: &format!("samplerate={rate}:rle=on:{thr}"),
                    samples: Some(4000),
                    triggers: Some("D0=r"),
                    ..Default::default()
                },
                Some(&mut during),
            );
            let ok = res.as_ref().is_ok_and(|f| {
                f.trigger.is_some_and(|t| edges(&f.values, D0.into(), "rise").contains(&t))
            });
            r.ok(
                &format!("RLE edge trigger @ {rate} ({step}-sample runs): marker on a rising edge"),
                ok,
                &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
            );
        }
        let res = c.run(
            &Req { config: "samplerate=250M:rle=on", samples: Some(100), ..Default::default() },
            None,
        );
        r.ok("RLE rejected above 200 MHz", res.is_err(), &fmt_err(&res));
    }

    if want("frames") {
        let _ = g.set(0);
        let train = [(D0, 500_000), (0, 500_000)];
        let mut during = || {
            let _ = g.seq(&train, 300);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                samples: Some(1000),
                frames: Some(3),
                triggers: Some("D0=r"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let good = c.frames.iter().all(|f| {
            f.values.len() == 1000
                && f.trigger.is_some_and(|t| edges(&f.values, D0.into(), "rise").contains(&t))
        });
        r.ok(
            "3 frames, each triggered on a D0 rising edge",
            res.is_ok() && c.frames.len() == 3 && good,
            &format!(
                "{}{} frames, {:?}",
                fmt_err(&res),
                c.frames.len(),
                c.frames.iter().map(|f| (f.values.len(), f.trigger)).collect::<Vec<_>>()
            ),
        );
        let steps: Vec<(u32, u32)> =
            (0..8u32).map(|m| (m, 1_000_000)).collect::<Vec<_>>().repeat(2);
        let mut during = || {
            let _ = g.seq(&steps, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:rle=on:{thr}"),
                time_ms: Some(10),
                triggers: Some("D0=1,D1=1,D2=1"),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.values.len() == 10000
                && f.trigger
                    .is_some_and(|t| t > 0 && f.values[t] & 7 == 7 && f.values[t - 1] & 7 != 7)
        });
        r.ok(
            "time 10 ms with RLE @ 1 MHz: 10000 samples, trigger on value 7",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| {
                format!("{} samples, trigger {:?}", f.values.len(), f.trigger)
            }),
        );
        let _ = g.set(0);
    }

    if want("wide") {
        // Extra wiring: GPIO -> channel index.
        let wide: [(u32, usize); 18] = [
            (25, 0),
            (26, 1),
            (27, 2),
            (32, 3),
            (33, 4),
            (23, 5),
            (22, 11),
            (21, 12),
            (16, 17),
            (15, 19),
            (13, 22),
            (17, 23),
            (4, 24),
            (5, 25),
            (19, 26),
            (18, 28),
            (12, 30),
            (2, 31),
        ];
        let chan_mask: u64 = wide.iter().map(|&(_, ch)| 1u64 << ch).sum();
        let _ = g.map(&wide.iter().map(|&(g, _)| g).collect::<Vec<_>>());
        let to_chan = |m: u32| -> u64 {
            wide.iter()
                .enumerate()
                .filter(|(i, _)| m >> i & 1 != 0)
                .map(|(_, &(_, ch))| 1u64 << ch)
                .sum()
        };
        let bit =
            |ch: usize| -> u32 { 1 << wide.iter().position(|&(_, c)| c == ch).expect("wired") };
        let distinct = |v: &[u64], min_len: usize| -> Vec<u64> {
            let mut out: Vec<u64> = Vec::new();
            for (x, n) in runs(v, chan_mask) {
                if n >= min_len && out.last() != Some(&x) {
                    out.push(x);
                }
            }
            out
        };
        let walk: Vec<(u32, u32)> = (0..wide.len() as u32).map(|i| (1 << i, 20000)).collect();
        let mut seq = vec![(0, 50000)];
        seq.extend_from_slice(&walk);
        seq.push((0, 50000));
        let mut during = || {
            let _ = g.seq(&seq, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D0=r"),
                channels: Some(ALL_CHANNELS),
                ..Default::default()
            },
            Some(&mut during),
        );
        let (ok, detail) = match &res {
            Ok(f) => {
                let got = distinct(&f.values[f.trigger.unwrap_or(0)..], 3);
                let exp: Vec<u64> = (0..wide.len() as u32).map(|i| to_chan(1 << i)).collect();
                (
                    got.len() >= exp.len() && got[..exp.len()] == exp[..],
                    format!("{} distinct values", got.len()),
                )
            }
            Err(e) => (false, e.clone()),
        };
        r.ok("walking one over 18 channels in all 4 byte lanes @ 1 MHz", ok, &detail);

        let mut x: u32 = 0x2d5a1;
        let pats: Vec<u32> = (0..60)
            .map(|_| {
                x = ((x >> 1) ^ (0u32.wrapping_sub(x & 1) & 0x40023)) & 0x3ffff;
                x
            })
            .collect();
        for (rate, ns, cfg, step) in
            [("100M", 1000u32, "", 100usize), ("500M", 600, "", 300), ("1M", 50000, ":rle=on", 50)]
        {
            let mut seq = vec![(0u32, 3000u32)];
            seq.extend(pats.iter().map(|&p| (p, ns)));
            seq.push((0, 3000));
            let mut during = || {
                let _ = g.seq(&seq, 1);
            };
            let res = c.run(
                &Req {
                    config: &format!("samplerate={rate}{cfg}:captureratio=2:{thr}"),
                    samples: if cfg.is_empty() { None } else { Some(4000) },
                    triggers: Some("D0=r"),
                    channels: Some(ALL_CHANNELS),
                    ..Default::default()
                },
                Some(&mut during),
            );
            let (ok, detail) = match &res {
                Ok(f) => {
                    let mut got = distinct(&f.values[f.trigger.unwrap_or(0)..], step / 4);
                    got.pop();
                    let k = pats
                        .iter()
                        .enumerate()
                        .position(|(i, &p)| p & 1 != 0 && (i == 0 || pats[i - 1] & 1 == 0))
                        .unwrap_or(0);
                    let exp: Vec<u64> =
                        pats[k..].iter().map(|&p| to_chan(p)).take(got.len()).collect();
                    let n = got.len().min(exp.len());
                    let first_bad = (0..n).find(|&i| got[i] != exp[i]);
                    (
                        n >= 4 && first_bad.is_none(),
                        format!("{n} compared, first mismatch {first_bad:?}"),
                    )
                }
                Err(e) => (false, e.clone()),
            };
            r.ok(&format!("random 18 bit patterns @ {rate}{cfg}"), ok, &detail);
        }
        let (b30, b24, b31, b12) = (bit(30), bit(24), bit(31), bit(12));
        let seq = [
            (0, 20000),
            (b24, 10000),
            (0, 10000),
            (b30, 10000),
            (b30 | b24, 10000),
            (b24, 10000),
            (0, 10000),
        ];
        let mut during = || {
            let _ = g.seq(&seq, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D30=1,D24=r"),
                channels: Some(ALL_CHANNELS),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.trigger.is_some_and(|t| {
                t > 0
                    && f.values[t] >> 24 & 1 != 0
                    && f.values[t - 1] >> 24 & 1 == 0
                    && f.values[t] >> 30 & 1 != 0
            })
        });
        r.ok(
            "trigger D24 rising while D30=1 (upper lanes)",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );
        let seq =
            [(0, 20000), (b31 | b12, 10000), (b12, 10000), (0, 10000), (b31, 10000), (0, 10000)];
        let mut during = || {
            let _ = g.seq(&seq, 1);
        };
        let res = c.run(
            &Req {
                config: &format!("samplerate=1M:{thr}"),
                triggers: Some("D31=f,D12=0"),
                channels: Some(ALL_CHANNELS),
                ..Default::default()
            },
            Some(&mut during),
        );
        let ok = res.as_ref().is_ok_and(|f| {
            f.trigger.is_some_and(|t| {
                t > 0
                    && f.values[t] >> 31 & 1 == 0
                    && f.values[t - 1] >> 31 & 1 != 0
                    && f.values[t] >> 12 & 1 == 0
            })
        });
        r.ok(
            "trigger D31 falling while D12=0",
            ok,
            &res.as_ref().map_or_else(Clone::clone, |f| format!("trigger {:?}", f.trigger)),
        );
        let _ = g.map(&[]);
        let _ = g.set(0);
    }

    let failed = r.results.iter().filter(|(_, ok)| !ok).count();
    println!("{} checks, {failed} failed", r.results.len());
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
