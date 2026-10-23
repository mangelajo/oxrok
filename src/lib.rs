//! oxrok: a sigrok compatible stack in Rust.
//!
//! One crate, organised as the layers a device goes through:
//!
//! - [`core`]: keys, values, packets, the device and driver contracts,
//!   the session and the trigger model;
//! - transports: [`usb`], [`serial`], [`hid`], [`scpi`], [`ftdi`] and
//!   [`ezusb`], each with record and replay for tests;
//! - [`drivers`]: one module per device family;
//! - [`dmm`]: the multimeter packet parsers;
//! - [`formats`]: input and output modules byte compatible with
//!   sigrok-cli;
//! - [`decode`] and [`decoders`]: the protocol decoder runtime and the
//!   native decoders; [`pydecode`] hosts libsigrokdecode's Python
//!   decoders when built with the `python` feature.
//!
//! # Features
//!
//! - `cli` (default): the `oxrok` command line tool.
//! - `nusb` (default) and `libusb`: the USB backends; with both,
//!   `OXROK_USB_BACKEND` picks one at run time.
//! - `python`: [`pydecode`], which runs libsigrokdecode's Python
//!   decoders unchanged.
//!
//! # Installing
//!
//! ```text
//! cargo install oxrok --version 0.1.0-alpha.1 --locked
//! cargo install oxrok --version 0.1.0-alpha.1 --locked --features python
//! ```
//!
//! The `python` feature needs Python 3.9 or newer with its shared
//! library at build and at run time (Homebrew or python.org Pythons,
//! `python3-dev` on Debian and Ubuntu, `python3-devel` on Fedora), and
//! the decoders of a libsigrokdecode installation (`brew install
//! libsigrokdecode`, the distribution package, or a checkout). The build
//! takes the interpreter from `PYO3_PYTHON`, the active virtualenv, or
//! `python` and `python3` on `PATH`. Decoders are found in
//! `OXROK_DECODERS_DIR`, `SIGROKDECODE_DIR`,
//! `~/.local/share/libsigrokdecode/decoders` and the usual system
//! directories.
//!
//! As a library dependency:
//!
//! ```toml
//! [dependencies]
//! oxrok = { version = "0.1.0-alpha.1", default-features = false, features = ["nusb"] }
//! ```
//!
//! Every module forbids unsafe code except `pydecode`, which crosses
//! into Python.

pub mod core;
pub mod decode;
pub mod decoders;
pub mod dmm;
pub mod ezusb;
pub mod formats;
pub mod ftdi;
pub mod hid;
#[cfg(feature = "python")]
pub mod pydecode;
pub mod scpi;
pub mod serial;
pub mod usb;

/// The device drivers, one module per family.
pub mod drivers {
    pub mod dcttech_usbrelay;
    pub mod demo;
    pub mod fx2lafw;
    pub mod logicport;
    pub mod microchip_pickit2;
    pub mod raspberrypi_pico;
    pub mod rigol_ds;
    pub mod serial_dmm;
}

/// Registers every driver.
pub fn register_all(registry: &mut core::Registry) {
    registry.register(Box::new(drivers::demo::DemoDriver));
    registry.register(Box::new(drivers::logicport::LogicPortDriver));
    drivers::serial_dmm::register_all(registry);
    registry.register(Box::new(drivers::rigol_ds::RigolDsDriver));
    registry.register(Box::new(drivers::raspberrypi_pico::RaspberryPiPicoDriver));
    registry.register(Box::new(drivers::dcttech_usbrelay::UsbRelayDriver));
    registry.register(Box::new(drivers::microchip_pickit2::Pickit2Driver));
    registry.register(Box::new(drivers::fx2lafw::Fx2lafwDriver));
}
