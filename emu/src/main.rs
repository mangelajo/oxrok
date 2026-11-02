//! `oxrok-emu`: serves a device model over USB/IP and, on Linux with
//! root, attaches it to the machine through `vhci-hcd`.
//!
//! ```text
//! oxrok-emu list
//! sudo oxrok-emu serve usbrelay --attach --open-devices --log tx.log
//! ```
//!
//! While serving, commands on standard input drive the run, each
//! acknowledged with an `OK` line on standard output once done:
//! `section NAME` starts a section of the transaction log, `reset`
//! puts a fresh model behind the attached device (its initial state,
//! for a second run), `detach` (or end of input, or a termination
//! signal) detaches the device and exits. `READY port=N busid=B nodes=...` is printed once the kernel
//! has configured the device. The exit status is 2 when any violation
//! was logged.

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

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::List => {
            for (name, about) in models::NAMES {
                println!("{name:<12} {about}");
            }
            ExitCode::SUCCESS
        }
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
    log: Arc<Mutex<TxLog>>,
    stop: AtomicBool,
}

impl Attached {
    /// Detaches (once) and reports the violation count.
    fn finish(&self) -> usize {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(port) = self.port.lock().expect("port lock").take() {
            match vhci::detach(port) {
                Ok(()) => eprintln!("oxrok-emu: detached port {port}"),
                Err(e) => eprintln!("oxrok-emu: {e}"),
            }
        }
        let n = self.log.lock().expect("log lock").violations();
        println!("violations: {n}");
        n
    }
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
    let server = Server::new(Arc::clone(&device), Arc::clone(&log), BUSID, 1, 2);
    let devid = server.devid();
    let listener = TcpListener::bind(&args.listen)?;
    let addr = listener.local_addr()?;
    println!("listening on {addr} busid {BUSID}");

    let state = Arc::new(Attached {
        port: Mutex::new(None),
        log: Arc::clone(&log),
        stop: AtomicBool::new(false),
    });
    let server_state = Arc::clone(&state);
    let server_thread = thread::spawn(move || server.run(&listener, &server_state.stop));
    let signal_state = Arc::clone(&state);
    ctrlc::set_handler(move || {
        let n = signal_state.finish();
        std::process::exit(if n == 0 { 0 } else { 2 });
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
        let busid = vhci::busid(port, Duration::from_secs(10))?;
        let dir = vhci::wait_configured(&busid, Duration::from_secs(10))?;
        let nodes = vhci::device_nodes(&dir, Duration::from_secs(5));
        if args.open_devices {
            vhci::open_nodes(&nodes)?;
        }
        let names: Vec<String> = nodes.iter().map(|n| n.display().to_string()).collect();
        println!("READY port={port} busid={busid} nodes={}", names.join(","));
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
    Ok(n)
}
