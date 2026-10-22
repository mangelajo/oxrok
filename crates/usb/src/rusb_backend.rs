//! The libusb backend through `rusb`.

use crate::{ControlRequest, Handle, Result, Transport, UsbDeviceInfo};
use oxrok_core::Error;
use std::time::Duration;

fn map(e: rusb::Error) -> Error {
    match e {
        rusb::Error::Timeout => Error::Timeout,
        rusb::Error::Busy => Error::Busy,
        rusb::Error::Access => Error::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "access denied (permission denied)",
        )),
        rusb::Error::NoDevice => Error::device("device disconnected"),
        rusb::Error::Pipe => Error::device("endpoint stalled"),
        other => Error::Device(other.to_string()),
    }
}

pub(crate) fn list() -> Result<Vec<UsbDeviceInfo>> {
    let mut out = Vec::new();
    for d in rusb::devices().map_err(map)?.iter() {
        let Ok(desc) = d.device_descriptor() else { continue };
        // Strings need an open handle; skip them when the device is not
        // ours to open. Drivers read them again after opening.
        let (manufacturer, product, serial) = match d.open() {
            Ok(h) => (
                h.read_manufacturer_string_ascii(&desc).ok(),
                h.read_product_string_ascii(&desc).ok(),
                h.read_serial_number_string_ascii(&desc).ok(),
            ),
            Err(_) => (None, None, None),
        };
        let v = desc.device_version();
        out.push(UsbDeviceInfo {
            vendor_id: desc.vendor_id(),
            product_id: desc.product_id(),
            device_version: (u16::from(v.major()) << 8)
                | (u16::from(v.minor()) << 4)
                | u16::from(v.sub_minor()),
            bus: d.bus_number(),
            address: d.address(),
            manufacturer,
            product,
            serial,
            handle: Handle::Rusb { bus: d.bus_number(), address: d.address() },
        });
    }
    Ok(out)
}

pub(crate) fn open(info: &UsbDeviceInfo) -> Result<Box<dyn Transport>> {
    let Handle::Rusb { bus, address } = info.handle else {
        return Err(Error::not_supported("not a libusb device"));
    };
    let device = rusb::devices()
        .map_err(map)?
        .iter()
        .find(|d| d.bus_number() == bus && d.address() == address)
        .ok_or_else(|| Error::device("device is gone"))?;
    let handle = device.open().map_err(map)?;
    let _ = handle.set_auto_detach_kernel_driver(true);
    Ok(Box::new(Rusb { info: info.clone(), device, handle, claimed: Vec::new() }))
}

struct Rusb {
    info: UsbDeviceInfo,
    device: rusb::Device<rusb::GlobalContext>,
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
    claimed: Vec<u8>,
}

impl Transport for Rusb {
    fn info(&self) -> &UsbDeviceInfo {
        &self.info
    }

    fn claim_interface(&mut self, interface: u8) -> Result<()> {
        if self.claimed.contains(&interface) {
            return Ok(());
        }
        self.handle.claim_interface(interface).map_err(map)?;
        self.claimed.push(interface);
        Ok(())
    }

    fn release_interface(&mut self, interface: u8) -> Result<()> {
        if let Some(pos) = self.claimed.iter().position(|&i| i == interface) {
            self.claimed.remove(pos);
            self.handle.release_interface(interface).map_err(map)?;
        }
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.claimed.clear();
        self.handle.reset().map_err(map)
    }

    fn max_packet_size(&mut self, endpoint: u8) -> Result<usize> {
        let config = self.device.active_config_descriptor().map_err(map)?;
        for i in config.interfaces() {
            for d in i.descriptors() {
                for e in d.endpoint_descriptors() {
                    if e.address() == endpoint {
                        return Ok(usize::from(e.max_packet_size()));
                    }
                }
            }
        }
        Err(Error::device("no such endpoint"))
    }

    fn control_out(&mut self, req: ControlRequest, data: &[u8], timeout: Duration) -> Result<()> {
        self.handle
            .write_control(
                req.request_type & 0x7f,
                req.request,
                req.value,
                req.index,
                data,
                timeout,
            )
            .map(|_| ())
            .map_err(map)
    }

    fn control_in(
        &mut self,
        req: ControlRequest,
        len: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self
            .handle
            .read_control(
                req.request_type | 0x80,
                req.request,
                req.value,
                req.index,
                &mut buf,
                timeout,
            )
            .map_err(map)?;
        buf.truncate(n);
        Ok(buf)
    }

    fn bulk_out(&mut self, endpoint: u8, data: &[u8], timeout: Duration) -> Result<usize> {
        self.handle.write_bulk(endpoint, data, timeout).map_err(map)
    }

    fn bulk_in(&mut self, endpoint: u8, len: usize, timeout: Duration) -> Result<Vec<u8>> {
        let mps = self.max_packet_size(endpoint)?.max(1);
        let len = len.div_ceil(mps).max(1) * mps;
        let mut buf = vec![0u8; len];
        match self.handle.read_bulk(endpoint, &mut buf, timeout) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) => Err(map(e)),
        }
    }
}
