// Counterpart in sigrok:
//   libsigrok/src/session.c,
//   Copyright (C) 2010-2012 Bert Vermeulen <bert@biot.com>,
//   Copyright (C) 2015 Daniel Elstner <daniel.kitta@gmail.com>,
//   with commits by Uwe Hermann, Martin Ling, Gerhard Sittig, Stefan Brüns,
//   Aurelien Jacobs, Daniel Glöckner, Frank Stettner, Soeren Apel,
//   Wolfram Sang
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! A session runs devices and hands their packets to a consumer.

use crate::device::{Device, PacketSink};
use crate::error::{Error, Result};
use crate::packet::Packet;
use crate::trigger::Trigger;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::Duration;

/// How many packets may wait between the devices and the consumer.
const QUEUE_DEPTH: usize = 64;

/// What the consumer wants after a packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Keep going.
    Continue,
    /// Stop every device; the session ends once they sent their last
    /// packet.
    Stop,
}

/// One or more devices acquiring together.
pub struct Session {
    devices: Vec<Box<dyn Device>>,
    trigger: Option<Trigger>,
    stop_flag: Arc<AtomicBool>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    /// An empty session.
    pub fn new() -> Self {
        Session { devices: Vec::new(), trigger: None, stop_flag: Arc::new(AtomicBool::new(false)) }
    }

    /// Adds a device. It must be open.
    pub fn add_device(&mut self, device: Box<dyn Device>) -> usize {
        self.devices.push(device);
        self.devices.len() - 1
    }

    /// The devices, in the order they were added.
    pub fn devices(&self) -> &[Box<dyn Device>] {
        &self.devices
    }

    /// Mutable access to the devices.
    pub fn devices_mut(&mut self) -> &mut [Box<dyn Device>] {
        &mut self.devices
    }

    /// Takes the devices out of the session, e.g. to close them.
    pub fn into_devices(self) -> Vec<Box<dyn Device>> {
        self.devices
    }

    /// The trigger for the next run, applied to every device.
    pub fn set_trigger(&mut self, trigger: Option<Trigger>) {
        self.trigger = trigger;
    }

    /// A flag that stops a running session when set, e.g. from a signal
    /// handler.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop_flag)
    }

    /// Runs one acquisition on every device and calls `consumer` with
    /// each packet and the index of the device it came from. Returns when
    /// every device sent [`Packet::End`], or after the consumer asked to
    /// stop and the devices ended.
    pub fn run(&mut self, mut consumer: impl FnMut(usize, &Packet) -> Flow) -> Result<()> {
        if self.devices.is_empty() {
            return Err(Error::arg("a session needs at least one device"));
        }
        self.stop_flag.store(false, Ordering::SeqCst);
        for dev in &mut self.devices {
            if !dev.channels().iter().any(|c| c.enabled) {
                return Err(Error::arg(format!(
                    "{} device has no enabled channels",
                    dev.info().driver
                )));
            }
            dev.set_trigger(self.trigger.clone())?;
        }
        let (tx, rx) = sync_channel(QUEUE_DEPTH);
        let mut started = 0;
        let mut start_err = None;
        for (i, dev) in self.devices.iter_mut().enumerate() {
            match dev.start(PacketSink::new(tx.clone(), i)) {
                Ok(()) => started += 1,
                Err(e) => {
                    start_err = Some(e);
                    break;
                }
            }
        }
        drop(tx);
        if let Some(e) = start_err {
            for dev in self.devices.iter_mut().take(started) {
                let _ = dev.stop();
            }
            // Drain what the started devices send, so their threads end.
            while rx.recv().is_ok() {}
            return Err(e);
        }
        let mut ended = 0;
        let mut stopping = false;
        while ended < started {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok((dev, packet)) => {
                    let is_end = matches!(packet, Packet::End);
                    if consumer(dev, &packet) == Flow::Stop {
                        self.stop_flag.store(true, Ordering::SeqCst);
                    }
                    if is_end {
                        ended += 1;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if !stopping && self.stop_flag.load(Ordering::SeqCst) {
                stopping = true;
                for dev in &mut self.devices {
                    dev.stop()?;
                }
            }
        }
        Ok(())
    }
}
