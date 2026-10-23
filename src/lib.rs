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
//! The command line tool lives behind the `cli` feature.
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
