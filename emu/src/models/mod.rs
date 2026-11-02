//! The device models, by name.

pub mod usbrelay;

use crate::model::DeviceModel;
use crate::{Error, Result};

/// Name and description of each model, with its options.
pub const NAMES: &[(&str, &str)] = &[(
    "usbrelay",
    "dcttech USBRelay board (HID, 16c0:05df), the Pico emulator's role; relays=N (1 to 8, default 2)",
)];

/// Builds a model from its name and `key=value` options.
pub fn build(name: &str, options: &[(String, String)]) -> Result<Box<dyn DeviceModel>> {
    match name {
        "usbrelay" => {
            let mut relays = 2;
            for (k, v) in options {
                match k.as_str() {
                    "relays" => {
                        relays = v
                            .parse()
                            .ok()
                            .filter(|n| (1..=8).contains(n))
                            .ok_or_else(|| Error(format!("relays={v}: 1 to 8")))?;
                    }
                    other => return Err(Error(format!("usbrelay has no option {other}"))),
                }
            }
            Ok(Box::new(usbrelay::UsbRelay::new(relays)))
        }
        other => Err(Error(format!("no model named {other}; oxrok-emu list names them"))),
    }
}
