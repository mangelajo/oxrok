// Counterpart in sigrok:
//   libsigrok/src/scpi/scpi_usbtmc_libusb.c,
//   Copyright (C) 2014 Aurelien Jacobs <aurel@gnuage.org>,
//   with commits by Martin Ling, Bert Vermeulen, Uwe Hermann, Gerhard Sittig,
//   Soeren Apel, Daniel Elstner, Wolfram Sang, Timo Kokkonen
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! USBTMC with the USB488 subclass: SCPI messages in bulk transfers
//! with a twelve byte header.

use crate::ScpiTransport;
use oxrok_core::{Error, Result};
use oxrok_usb::{ControlRequest, Transport, UsbDeviceInfo};
use std::time::Duration;

const CLASS_APPLICATION: u8 = 0xfe;
const SUBCLASS_USBTMC: u8 = 0x03;
const PROTOCOL_USB488: u8 = 0x01;
const TRANSFER_TYPE_BULK: u8 = 2;

const MAX_TRANSFER: usize = 2048;
const TIMEOUT: Duration = Duration::from_secs(1);
const HEADER: usize = 12;

const DEV_DEP_MSG_OUT: u8 = 1;
const REQUEST_DEV_DEP_MSG_IN: u8 = 2;
const DEV_DEP_MSG_IN: u8 = 2;
const EOM: u8 = 0x01;

const GET_CAPABILITIES: u8 = 7;
const REN_CONTROL: u8 = 160;
const GO_TO_LOCAL: u8 = 161;
const LOCAL_LOCKOUT: u8 = 162;
const STATUS_SUCCESS: u8 = 0x01;
const USB488_CAP_RL1: u8 = 0x02;

/// Devices that misbehave when put into remote mode.
const NO_REMOTE: [(u16, u16); 7] = [
    (0x1ab1, 0x0588), // Rigol DS1000
    (0x1ab1, 0x04b0), // Rigol DS2000
    (0x1ab1, 0x04b1), // Rigol DS4000
    (0x1ab1, 0x0515), // Rigol MSO5000
    (0x0957, 0x0588), // Agilent DSO1000 (a Rigol DS1000)
    (0x0b21, 0xffff), // all Yokogawa
    (0xf4ec, 0xffff), // all Siglent SDS
];
/// Devices that want a USB reset after the interface is claimed.
const RESET_ON_OPEN: [(u16, u16); 1] = [(0xf4ec, 0xffff)];

fn listed(table: &[(u16, u16)], vid: u16, pid: u16) -> bool {
    table.iter().any(|(v, p)| *v == vid && (*p == 0xffff || *p == pid))
}

/// The USBTMC devices on the machine.
pub fn list() -> Result<Vec<UsbDeviceInfo>> {
    Ok(oxrok_usb::list_devices()?
        .into_iter()
        .filter(|d| {
            d.interfaces.iter().any(|i| {
                i.class == CLASS_APPLICATION
                    && i.subclass == SUBCLASS_USBTMC
                    && i.protocol == PROTOCOL_USB488
            })
        })
        .collect())
}

/// An open USBTMC device.
pub struct Usbtmc {
    usb: Box<dyn Transport>,
    resource: String,
    interface: u8,
    bulk_in: u8,
    bulk_out: u8,
    usb488_caps: u8,
    tag: u8,
    remote: bool,
    buffer: Vec<u8>,
    response_length: usize,
    response_read: usize,
    remaining: usize,
    attributes: u8,
}

fn class_in(request: u8, value: u16, index: u16) -> ControlRequest {
    ControlRequest { request_type: 0xa1, request, value, index }
}

impl Usbtmc {
    /// Opens `info`, claims its USBTMC interface and reads its
    /// capabilities.
    pub fn open(info: &UsbDeviceInfo) -> Result<Usbtmc> {
        let usb = oxrok_usb::open(info)?;
        Self::from_transport(usb, format!("usbtmc/{}", info.conn()))
    }

    /// Sets up USBTMC on an open transport, e.g. a replay.
    pub fn from_transport(mut usb: Box<dyn Transport>, resource: String) -> Result<Usbtmc> {
        let interfaces = usb.interfaces()?;
        let iface = interfaces
            .iter()
            .find(|i| {
                i.class.class == CLASS_APPLICATION
                    && i.class.subclass == SUBCLASS_USBTMC
                    && i.class.protocol == PROTOCOL_USB488
            })
            .ok_or_else(|| Error::device("Failed to find USBTMC interface."))?;
        let bulk_in = iface
            .endpoints
            .iter()
            .find(|e| e.transfer_type == TRANSFER_TYPE_BULK && e.address & 0x80 != 0)
            .map(|e| e.address)
            .ok_or_else(|| Error::device("USBTMC: no bulk IN endpoint"))?;
        let bulk_out = iface
            .endpoints
            .iter()
            .find(|e| e.transfer_type == TRANSFER_TYPE_BULK && e.address & 0x80 == 0)
            .map(|e| e.address)
            .ok_or_else(|| Error::device("USBTMC: no bulk OUT endpoint"))?;
        let interface = iface.class.number;
        usb.claim_interface(interface)?;
        let (vid, pid) = (usb.info().vendor_id, usb.info().product_id);
        if listed(&RESET_ON_OPEN, vid, pid) {
            usb.reset()?;
        }
        let mut usb488_caps = 0;
        if let Ok(caps) =
            usb.control_in(class_in(GET_CAPABILITIES, 0, u16::from(interface)), 24, TIMEOUT)
            && caps.len() == 24
        {
            usb488_caps = caps[15];
            log::debug!(
                "USBTMC capabilities: int {:#04x} dev {:#04x} usb488 {:#04x}",
                caps[4],
                caps[5],
                caps[15]
            );
        }
        let mut dev = Usbtmc {
            usb,
            resource,
            interface,
            bulk_in,
            bulk_out,
            usb488_caps,
            tag: 0,
            remote: false,
            buffer: Vec::new(),
            response_length: 0,
            response_read: 0,
            remaining: 0,
            attributes: 0,
        };
        dev.remote = dev.usb488_caps & USB488_CAP_RL1 != 0 && !listed(&NO_REMOTE, vid, pid);
        if dev.remote {
            dev.control(REN_CONTROL, 1)?;
            dev.control(LOCAL_LOCKOUT, 0)?;
        }
        Ok(dev)
    }

    fn control(&mut self, request: u8, value: u16) -> Result<()> {
        let status =
            self.usb.control_in(class_in(request, value, u16::from(self.interface)), 1, TIMEOUT)?;
        if status.first() != Some(&STATUS_SUCCESS) {
            return Err(Error::device(format!("USBTMC request {request} failed: {status:?}")));
        }
        Ok(())
    }

    fn next_tag(&mut self) -> u8 {
        self.tag = self.tag.wrapping_add(1);
        if self.tag == 0 {
            self.tag = 1;
        }
        self.tag
    }

    fn bulk_out(&mut self, msg_id: u8, data: &[u8], size: u32, attributes: u8) -> Result<()> {
        if data.len() + HEADER + 3 > MAX_TRANSFER {
            return Err(Error::device("USBTMC bulk out transfer is too big."));
        }
        let tag = self.next_tag();
        let mut msg = Vec::with_capacity(HEADER + data.len() + 3);
        msg.extend([msg_id, tag, !tag, 0]);
        msg.extend(size.to_le_bytes());
        msg.extend([attributes, 0, 0, 0]);
        msg.extend_from_slice(data);
        while !msg.len().is_multiple_of(4) {
            msg.push(0);
        }
        let sent = self.usb.bulk_out(self.bulk_out, &msg, TIMEOUT)?;
        if sent < msg.len() {
            return Err(Error::device(format!("USBTMC short bulk out: {sent}/{}", msg.len())));
        }
        Ok(())
    }

    fn bulk_in_start(&mut self, timeout: Duration) -> Result<()> {
        let mut data = Vec::new();
        for attempt in 0..2 {
            data = self.usb.bulk_in(self.bulk_in, MAX_TRANSFER, timeout)?;
            if data.is_empty() && attempt == 0 {
                // A Rigol DS1054Z sends an empty transfer at times and
                // the real message right after.
                log::warn!("USBTMC bulk in start was empty; retrying");
                continue;
            }
            break;
        }
        if data.len() < HEADER {
            return Err(Error::device(format!(
                "USBTMC bulk in returned too little data: {} bytes",
                data.len()
            )));
        }
        if data[0] != DEV_DEP_MSG_IN || data[1] != self.tag || data[2] != !self.tag {
            return Err(Error::device("USBTMC invalid bulk in header."));
        }
        let message_size =
            u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize + HEADER;
        self.attributes = data[8];
        self.response_length = data.len().min(message_size);
        self.response_read = HEADER;
        self.remaining = message_size - self.response_length;
        self.buffer = data;
        Ok(())
    }

    fn bulk_in_continue(&mut self, timeout: Duration) -> Result<()> {
        let data = self.usb.bulk_in(self.bulk_in, MAX_TRANSFER, timeout)?;
        self.response_length = data.len().min(self.remaining);
        self.response_read = 0;
        self.remaining -= self.response_length;
        self.buffer = data;
        Ok(())
    }

    fn begin(&mut self, timeout: Duration) -> Result<()> {
        self.remaining = 0;
        self.bulk_out(REQUEST_DEV_DEP_MSG_IN, &[], u32::MAX, 0)?;
        self.bulk_in_start(timeout)
    }
}

impl Drop for Usbtmc {
    fn drop(&mut self) {
        if self.remote {
            let _ = self.control(GO_TO_LOCAL, 0);
        }
        let _ = self.usb.release_interface(self.interface);
    }
}

impl ScpiTransport for Usbtmc {
    fn resource(&self) -> &str {
        &self.resource
    }

    fn send(&mut self, data: &[u8]) -> Result<()> {
        self.bulk_out(DEV_DEP_MSG_OUT, data, data.len() as u32, EOM)
    }

    fn read_begin(&mut self) -> Result<()> {
        self.begin(TIMEOUT)
    }

    fn read_data(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        if self.response_read >= self.response_length {
            if self.remaining > 0 {
                self.bulk_in_continue(timeout)?;
            } else if self.attributes & EOM != 0 {
                return Err(Error::device("USBTMC: read past the end of the message"));
            } else {
                self.begin(timeout)?;
            }
        }
        let n = buf.len().min(self.response_length - self.response_read);
        buf[..n].copy_from_slice(&self.buffer[self.response_read..self.response_read + n]);
        self.response_read += n;
        Ok(n)
    }

    fn read_complete(&self) -> bool {
        self.response_read >= self.response_length
            && self.remaining == 0
            && self.attributes & EOM != 0
    }
}
