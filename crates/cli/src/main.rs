//! `oxrok`: a sigrok-cli compatible command line for oxrok.
//!
//! The flags and the output are the ones sigrok-cli users know, so
//! existing scripts keep working. Where sigrok-cli had rough edges (one
//! trigger stage, rounded option listings) this tool does better while
//! staying a superset.

#![forbid(unsafe_code)]

mod args;
mod logger;
mod parse;
mod run;
mod show;

use clap::Parser;
use oxrok_core::Registry;
use std::process::ExitCode;

/// Every driver this binary ships.
fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(oxrok_driver_demo::DemoDriver));
    r
}

fn main() -> ExitCode {
    let args = args::Args::parse();
    logger::init(args.loglevel);
    let registry = registry();
    let result = if args.version {
        show::version(&registry);
        Ok(())
    } else if args.list_supported {
        show::supported(&registry);
        Ok(())
    } else if args.scan {
        show::device_list(&registry, &args)
    } else if args.show {
        show::device_detail(&registry, &args)
    } else if !args.gets.is_empty() {
        run::get_options(&registry, &args)
    } else if args.set {
        run::set_options(&registry, &args)
    } else if args.samples.is_some()
        || args.time.is_some()
        || args.frames.is_some()
        || args.continuous
    {
        run::session(&registry, &args)
    } else {
        show::help();
        Ok(())
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(oxrok_core::Error::Arg(msg)) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
