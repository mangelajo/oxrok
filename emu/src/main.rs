//! `oxrok-emu`: serves a device model over USB/IP and, on Linux with
//! root, attaches it to the machine through `vhci-hcd`.
//!
//! ```text
//! oxrok-emu list
//! sudo oxrok-emu serve usbrelay --attach --open-devices --log tx.log
//! sudo oxrok-emu cleanup
//! ```
//!
//! While serving, commands on standard input drive the run, each
//! acknowledged with an `OK` line on standard output once done:
//! `section NAME` starts a section of the transaction log, `reset`
//! puts a fresh model behind the attached device (its initial state,
//! for a second run), `detach` (or end of input, or a termination
//! signal) detaches the device and exits. `READY port=N busid=B
//! usb=BUS.ADDR usbfs=/dev/bus/usb/BBB/DDD nodes=...` is printed once
//! the kernel has configured the device. The exit status is 2 when any
//! violation was logged.
//!
//! `cleanup` detaches the `vhci-hcd` ports a crashed run left attached:
//! those whose device announced an oxrok-emu `devid` (bus `0x6f78`), or
//! with `--all` every attached port.
//!
//! Each server announces a device number no attached oxrok-emu port
//! has, so its `devid` names its own port: on the way out it detaches
//! the port only while the port still holds that `devid`. A server
//! whose port someone else detached (a `cleanup`, say) thus leaves the
//! port alone even after another server has been given it, and exits
//! with status 1, as its device went away during the run.

use std::io::{self, BufRead};
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::{Parser, Subcommand};
use oxrok_emu::device::Device;
use oxrok_emu::txlog::TxLog;
use oxrok_emu::usbip::Server;
use oxrok_emu::{Result, models, usbip, vhci};

#[derive(Parser)]
#[command(name = "oxrok-emu", about = "USB/IP device emulator for driver checks without hardware")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lists the device models.
    List,
    /// Serves a model, and attaches it with --attach.
    Serve(Serve),
    /// Detaches the ports left attached by an oxrok-emu that died (root).
    Cleanup {
        /// Every attached port, whoever attached it.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Parser)]
struct Serve {
    /// The model (see `list`).
    model: String,
    /// A model option, KEY=VALUE.
    #[arg(short = 'o', long = "option", value_name = "KEY=VALUE")]
    options: Vec<String>,
    /// The address to listen on; port 0 picks a free one.
    #[arg(long, default_value = "127.0.0.1:3240")]
    listen: String,
    /// Attaches the device through vhci-hcd (Linux, root).
    #[arg(long)]
    attach: bool,
    /// After attaching, makes the device nodes world accessible.
    #[arg(long)]
    open_devices: bool,
    /// The transaction log file.
    #[arg(long)]
    log: Option<PathBuf>,
    /// Logs events (connections, held transfers, unlinks) too.
    #[arg(short, long)]
    verbose: bool,
}

/// The busid the model is exported under; the client asks for it.
const BUSID: &str = "1-1";

/// The bus number the server announces; with the device number it
/// forms the `devid` in the controller's `status`, by which `cleanup`
/// recognises the ports oxrok-emu attached (`0x6f78` is "ox").
const BUSNUM: u32 = 0x6f78;

/// A device number, 1 to 0xffff, that no attached oxrok-emu port has;
/// derived from the process id, so that two servers started one after
/// the other differ even when the first one's port is already gone.
fn pick_devnum() -> u32 {
    let used: Vec<u32> = vhci::attached_ports(None)
        .map(|ps| ps.iter().filter(|p| p.devid >> 16 == BUSNUM).map(|p| p.devid & 0xffff).collect())
        .unwrap_or_default();
    let start = std::process::id() % 0xffff;
    (0..0xffff).map(|i| (start + i) % 0xffff + 1).find(|d| !used.contains(d)).unwrap_or(1)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::List => {
            let width = models::NAMES.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
            for (name, about) in models::NAMES {
                println!("{name:<width$}  {about}");
            }
            ExitCode::SUCCESS
        }
        Command::Cleanup { all } => match cleanup(all) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("oxrok-emu: {e}");
                ExitCode::FAILURE
            }
        },
        Command::Serve(args) => match serve(&args) {
            Ok(violations) => {
                if violations == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(2)
                }
            }
            Err(e) => {
                eprintln!("oxrok-emu: {e}");
                ExitCode::FAILURE
            }
        },
    }
}

/// The state a termination signal needs to clean up.
struct Attached {
    port: Mutex<Option<u8>>,
    /// The `devid` the port holds while it is this server's.
    devid: u32,
    /// Set when the port was found detached by someone else.
    lost: AtomicBool,
    log: Arc<Mutex<TxLog>>,
    stop: AtomicBool,
}

impl Attached {
    /// Detaches (once) and reports the violation count. A port that no
    /// longer holds this server's `devid` is someone else's by now and
    /// is left alone.
    fn finish(&self) -> usize {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(port) = self.port.lock().expect("port lock").take() {
            let ours =
                vhci::attached_ports(Some(self.devid)).map(|ps| ps.iter().any(|p| p.port == port));
            match ours {
                Ok(true) => match vhci::detach(port) {
                    Ok(()) => eprintln!("oxrok-emu: detached port {port}"),
                    Err(e) => eprintln!("oxrok-emu: {e}"),
                },
                Ok(false) => {
                    self.lost.store(true, Ordering::Relaxed);
                    eprintln!(
                        "oxrok-emu: port {port} no longer holds devid {:#010x}: the device was \
                         detached by someone else during the run; the port is left alone",
                        self.devid
                    );
                }
                Err(e) => eprintln!("oxrok-emu: port {port} left attached: {e}"),
            }
        }
        let n = self.log.lock().expect("log lock").violations();
        println!("violations: {n}");
        n
    }
}

/// Detaches the stale ports and waits for the controller to free them.
fn cleanup(all: bool) -> Result<()> {
    if !vhci::available() {
        return Ok(());
    }
    let stale: Vec<vhci::PortStatus> = vhci::attached_ports(None)?
        .into_iter()
        .filter(|p| all || p.devid >> 16 == BUSNUM)
        .collect();
    for p in &stale {
        vhci::detach(p.port)?;
        println!("detached port {} (busid {}, devid {:#010x})", p.port, p.busid, p.devid);
    }
    // The port is free once the controller has shut the connection
    // down, which a server that is stopped rather than dead delays.
    let start = std::time::Instant::now();
    loop {
        let busy: Vec<u8> = vhci::attached_ports(None)?
            .iter()
            .filter(|p| stale.iter().any(|s| s.port == p.port))
            .map(|p| p.port)
            .collect();
        if busy.is_empty() {
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(5) {
            return Err(format!(
                "ports {busy:?} still in use after detach; is an oxrok-emu process stopped?"
            )
            .into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Waits for the device on `port` to be configured, opens its nodes,
/// and returns the `READY` line.
fn ready_line(port: u8, open: bool) -> Result<String> {
    let busid = vhci::busid(port, Duration::from_secs(10))?;
    let dir = vhci::wait_configured(&busid, Duration::from_secs(10))?;
    let found = vhci::open_device(&dir, open, Duration::from_secs(5))?;
    let names: Vec<String> = found.nodes.iter().map(|n| n.display().to_string()).collect();
    let usbfs = found.usbfs.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    Ok(format!(
        "READY port={port} busid={busid} usb={} usbfs={usbfs} nodes={}",
        found.bus_address.as_deref().unwrap_or(""),
        names.join(",")
    ))
}

fn serve(args: &Serve) -> Result<usize> {
    let options = args
        .options
        .iter()
        .map(|o| {
            o.split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .ok_or_else(|| format!("option '{o}' is not KEY=VALUE"))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let model = models::build(&args.model, &options)?;
    let speed = model.descriptors().speed;
    let log = Arc::new(Mutex::new(TxLog::new(args.log.as_deref(), args.verbose)?));
    log.lock().expect("log lock").section("attach");
    let device = Arc::new(Mutex::new(Device::new(model)));
    let server = Server::new(Arc::clone(&device), Arc::clone(&log), BUSID, BUSNUM, pick_devnum());
    let devid = server.devid();
    let listener = TcpListener::bind(&args.listen)?;
    let addr = listener.local_addr()?;
    println!("listening on {addr} busid {BUSID}");

    let state = Arc::new(Attached {
        port: Mutex::new(None),
        devid,
        lost: AtomicBool::new(false),
        log: Arc::clone(&log),
        stop: AtomicBool::new(false),
    });
    let server_state = Arc::clone(&state);
    let server_thread = thread::spawn(move || server.run(&listener, &server_state.stop));
    let signal_state = Arc::clone(&state);
    ctrlc::set_handler(move || {
        let n = signal_state.finish();
        let lost = signal_state.lost.load(Ordering::Relaxed);
        std::process::exit(if lost {
            1
        } else if n == 0 {
            0
        } else {
            2
        });
    })
    .map_err(|e| format!("signal handler: {e}"))?;

    let mut attached_socket = None;
    if args.attach {
        if !vhci::available() {
            return Err("vhci-hcd is not loaded (sudo modprobe vhci-hcd)".into());
        }
        let (socket, _record) = usbip::import(addr, BUSID)?;
        let port = vhci::free_port(speed)?;
        vhci::attach(port, socket.as_raw_fd(), devid, speed)?;
        // The controller took its own reference to the socket; ours is
        // kept until the end of the run anyway.
        attached_socket = Some(socket);
        *state.port.lock().expect("port lock") = Some(port);
        // A device that never configures is detached again, not left
        // on the bus for the next run to trip over.
        let ready = ready_line(port, args.open_devices);
        match ready {
            Ok(line) => println!("{line}"),
            Err(e) => {
                state.finish();
                return Err(e);
            }
        }
    }

    for line in io::stdin().lock().lines() {
        let line = line?;
        let line = line.trim();
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "" => {}
            "section" => {
                log.lock().expect("log lock").section(rest.trim());
                println!("OK {line}");
            }
            "reset" => {
                // A fresh model, the bus state kept: the next run starts
                // from the device's initial state.
                let fresh = models::build(&args.model, &options)?;
                device.lock().expect("device lock").replace_model(fresh);
                log.lock().expect("log lock").event("model reset");
                println!("OK {line}");
            }
            "detach" | "quit" => {
                println!("OK {line}");
                break;
            }
            _ => println!("ERR unknown command: {line}"),
        }
    }
    let n = state.finish();
    drop(attached_socket);
    match server_thread.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("oxrok-emu: server: {e}"),
        Err(_) => eprintln!("oxrok-emu: server thread panicked"),
    }
    if state.lost.load(Ordering::Relaxed) {
        return Err("the device was detached by someone else during the run".into());
    }
    Ok(n)
}
