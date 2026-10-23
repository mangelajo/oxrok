// Counterpart in sigrok:
//   libsigrok/src/scpi/scpi_serial.c,
//   Copyright (C) 2013 poljar (Damir Jelic) <poljarinho@gmail.com>,
//   Copyright (C) 2013 Martin Ling <martin-sigrok@earth.li>,
//   with commits by Bert Vermeulen, Uwe Hermann, Gerhard Sittig,
//   Aurelien Jacobs, Guido Trentalancia, Frank Stettner
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! SCPI over a serial port: responses end at a newline.

use crate::ScpiTransport;
use oxrok_core::Result;
use oxrok_serial::{Params, Port};
use std::time::Duration;

/// A serial port carrying SCPI.
pub struct Serial {
    port: Box<dyn Port>,
    resource: String,
    got_newline: bool,
}

impl Serial {
    /// Opens `path` with `params`.
    pub fn open(path: &str, params: Params) -> Result<Serial> {
        let port = oxrok_serial::open(path, params)?;
        Ok(Serial { port, resource: path.to_owned(), got_newline: false })
    }

    /// Wraps an open port, e.g. a replay.
    pub fn from_port(port: Box<dyn Port>) -> Serial {
        let resource = port.name().to_owned();
        Serial { port, resource, got_newline: false }
    }
}

impl ScpiTransport for Serial {
    fn resource(&self) -> &str {
        &self.resource
    }

    fn send(&mut self, data: &[u8]) -> Result<()> {
        self.port.write(data)
    }

    fn read_begin(&mut self) -> Result<()> {
        self.got_newline = false;
        Ok(())
    }

    fn read_data(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        let mut n = self.port.read(buf, timeout)?;
        // The usual NL, or the NL CR some meters send.
        self.got_newline = false;
        if n >= 1 && buf[n - 1] == b'\n' {
            self.got_newline = true;
        } else if n >= 2 && buf[n - 2] == b'\n' && buf[n - 1] == b'\r' {
            n -= 1;
            self.got_newline = true;
        }
        Ok(n)
    }

    fn read_complete(&self) -> bool {
        self.got_newline
    }
}
