//! Lists the HID devices the current user can open.

fn main() {
    match oxrok::hid::list() {
        Ok(devices) => {
            for d in devices {
                println!(
                    "{:04x}:{:04x} {} {} {}",
                    d.vendor_id,
                    d.product_id,
                    d.manufacturer.as_deref().unwrap_or("-"),
                    d.product,
                    d.serial.as_deref().unwrap_or("-")
                );
            }
        }
        Err(e) => eprintln!("{e}"),
    }
}
