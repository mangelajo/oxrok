//! Prints the interfaces and endpoints of a device: `interfaces VID:PID`.

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let (v, p) = arg.split_once(':').unwrap_or(("0", "0"));
    let vid = u16::from_str_radix(v, 16).unwrap_or(0);
    let pid = u16::from_str_radix(p, 16).unwrap_or(0);
    let devices = oxrok_usb::list_devices().expect("list");
    for d in devices.iter().filter(|d| d.vendor_id == vid && d.product_id == pid) {
        println!("{} {:?}", d.id(), d.product);
        for i in &d.interfaces {
            println!(
                "  listed interface {} class {:02x}/{:02x}/{:02x}",
                i.number, i.class, i.subclass, i.protocol
            );
        }
        match oxrok_usb::open(d).and_then(|mut t| t.interfaces()) {
            Ok(list) => {
                for i in list {
                    println!(
                        "  interface {} class {:02x}/{:02x}/{:02x}",
                        i.class.number, i.class.class, i.class.subclass, i.class.protocol
                    );
                    for e in i.endpoints {
                        println!(
                            "    endpoint {:02x} type {} size {}",
                            e.address, e.transfer_type, e.max_packet_size
                        );
                    }
                }
            }
            Err(e) => println!("  cannot read interfaces: {e}"),
        }
    }
}
