// Counterpart in sigrok:
//   libsigrok/src/scpi/scpi_tcp.c,
//   Copyright (C) 2013 Martin Ling <martin-sigrok@earth.li>,
//   with commits by Bert Vermeulen, Uwe Hermann, Gerhard Sittig,
//   Aurelien Jacobs, Daniel Elstner
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! SCPI over TCP: raw, where a short read ends a response, and Rigol's
//! variant where every chunk carries a four byte little endian length.

use crate::core::{Error, Result};
use crate::scpi::ScpiTransport;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// A TCP connection carrying SCPI.
pub struct Tcp {
    stream: TcpStream,
    resource: String,
    rigol: bool,
    length: Option<usize>,
    length_buf: Vec<u8>,
    response_length: usize,
    response_read: usize,
}

impl Tcp {
    /// Connects to `host:port`; `rigol` selects the length prefixed
    /// protocol.
    pub fn connect(host: &str, port: u16, rigol: bool) -> Result<Tcp> {
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(Error::Io)?
            .next()
            .ok_or_else(|| Error::device(format!("cannot resolve {host}")))?;
        let stream =
            TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(Error::Io)?;
        stream.set_nodelay(true).map_err(Error::Io)?;
        let prefix = if rigol { "tcp-rigol" } else { "tcp-raw" };
        Ok(Tcp {
            stream,
            resource: format!("{prefix}/{host}/{port}"),
            rigol,
            length: None,
            length_buf: Vec::new(),
            response_length: 0,
            response_read: 0,
        })
    }

    fn read_some(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        self.stream
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
            .map_err(Error::Io)?;
        match self.stream.read(buf) {
            Ok(n) => Ok(n),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(0)
            }
            Err(e) => Err(Error::Io(e)),
        }
    }
}

impl ScpiTransport for Tcp {
    fn resource(&self) -> &str {
        &self.resource
    }

    fn send(&mut self, data: &[u8]) -> Result<()> {
        self.stream.write_all(data).map_err(Error::Io)
    }

    fn read_begin(&mut self) -> Result<()> {
        self.length = None;
        self.length_buf.clear();
        self.response_length = 0;
        self.response_read = 0;
        Ok(())
    }

    fn read_data(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        if !self.rigol {
            let n = self.read_some(buf, timeout)?;
            // A short read ends a response, a full one may continue.
            self.length = Some(0);
            self.response_length = if n < buf.len() { n } else { buf.len() + 1 };
            self.response_read = n;
            return Ok(n);
        }
        if self.length.is_none() {
            let mut tmp = [0u8; 4];
            let want = 4 - self.length_buf.len();
            let n = self.read_some(&mut tmp[..want], timeout)?;
            self.length_buf.extend_from_slice(&tmp[..n]);
            if self.length_buf.len() < 4 {
                return Ok(0);
            }
            let len = u32::from_le_bytes([
                self.length_buf[0],
                self.length_buf[1],
                self.length_buf[2],
                self.length_buf[3],
            ]) as usize;
            self.length = Some(len);
            self.response_length = len;
        }
        if self.response_read >= self.response_length {
            return Err(Error::device("tcp-rigol: more data than announced"));
        }
        let room = buf.len().min(self.response_length - self.response_read);
        let n = self.read_some(&mut buf[..room], timeout)?;
        self.response_read += n;
        Ok(n)
    }

    fn read_complete(&self) -> bool {
        self.length.is_some() && self.response_read >= self.response_length
    }
}
