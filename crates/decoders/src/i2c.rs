// Counterpart in sigrok:
//   libsigrokdecode/decoders/i2c/pd.py,
//   Copyright (C) 2010-2016 Uwe Hermann <uwe@hermann-uwe.de>,
//   with commits by Gerhard Sittig and others (see the libsigrokdecode
//   history).
// This file is an independent implementation written for oxrok from the
//   I2C bus protocol and the decoder's documented classes and texts.

//! I2C: start and stop conditions, 7 bit addresses, data bytes, acks.

use oxrok_core::{Error, Result};
use oxrok_decode::{
    AnnClass, AnnRow, BinClass, ChannelDesc, Cond, CondKind, Config, Context, Decoder, DecoderDef,
    DecoderInfo, Item, OptionDesc, OptionValue,
};

const SCL: usize = 0;
const SDA: usize = 1;

/// The decoder description.
pub static INFO: DecoderInfo = DecoderInfo {
    id: "i2c",
    name: "I2C",
    longname: "Inter-Integrated Circuit",
    desc: "Two-wire, multi-master, serial bus.",
    license: "MIT OR Apache-2.0",
    inputs: &["logic"],
    outputs: &["i2c"],
    tags: &["Embedded/industrial"],
    channels: &[
        ChannelDesc { id: "scl", name: "SCL", desc: "Serial clock line" },
        ChannelDesc { id: "sda", name: "SDA", desc: "Serial data line" },
    ],
    optional_channels: &[],
    options: &[OptionDesc {
        id: "address_format",
        desc: "Displayed slave address format",
        default: OptionValue::s("shifted"),
        values: &[OptionValue::s("shifted"), OptionValue::s("unshifted")],
    }],
    annotations: &[
        AnnClass { id: "start", desc: "Start condition" },
        AnnClass { id: "repeat-start", desc: "Repeat start condition" },
        AnnClass { id: "stop", desc: "Stop condition" },
        AnnClass { id: "ack", desc: "ACK" },
        AnnClass { id: "nack", desc: "NACK" },
        AnnClass { id: "bit", desc: "Data/address bit" },
        AnnClass { id: "address-read", desc: "Address read" },
        AnnClass { id: "address-write", desc: "Address write" },
        AnnClass { id: "data-read", desc: "Data read" },
        AnnClass { id: "data-write", desc: "Data write" },
        AnnClass { id: "warnings", desc: "Human-readable warnings" },
    ],
    annotation_rows: &[
        AnnRow { id: "bits", desc: "Bits", classes: &[5] },
        AnnRow { id: "addr-data", desc: "Address/Data", classes: &[0, 1, 2, 3, 4, 6, 7, 8, 9] },
        AnnRow { id: "warnings", desc: "Warnings", classes: &[10] },
    ],
    binary: &[
        BinClass { id: "address-read", desc: "Address read" },
        BinClass { id: "address-write", desc: "Address write" },
        BinClass { id: "data-read", desc: "Data read" },
        BinClass { id: "data-write", desc: "Data write" },
    ],
    doc: "I2C (Inter-Integrated Circuit) is a two wire serial bus with one or \
more masters and addressed slaves. The decoder marks start and stop \
conditions, 7 bit addresses with the read or write bit, data bytes and \
the ACK or NACK after each byte.",
};

/// The decoder definition for the registry.
pub static DEF: DecoderDef = DecoderDef { info: &INFO, new: || Box::new(I2c::default()) };

/// Protocol events: class index, long and short text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Proto {
    Start,
    StartRepeat,
    Stop,
    Ack,
    Nack,
    AddressRead,
    AddressWrite,
    DataRead,
    DataWrite,
}

impl Proto {
    fn class(self) -> usize {
        match self {
            Proto::Start => 0,
            Proto::StartRepeat => 1,
            Proto::Stop => 2,
            Proto::Ack => 3,
            Proto::Nack => 4,
            Proto::AddressRead => 6,
            Proto::AddressWrite => 7,
            Proto::DataRead => 8,
            Proto::DataWrite => 9,
        }
    }

    fn texts(self) -> (&'static str, &'static str) {
        match self {
            Proto::Start => ("Start", "S"),
            Proto::StartRepeat => ("Start repeat", "Sr"),
            Proto::Stop => ("Stop", "P"),
            Proto::Ack => ("ACK", "A"),
            Proto::Nack => ("NACK", "N"),
            Proto::AddressRead => ("Address read", "AR"),
            Proto::AddressWrite => ("Address write", "AW"),
            Proto::DataRead => ("Data read", "DR"),
            Proto::DataWrite => ("Data write", "DW"),
        }
    }

    fn python_name(self) -> &'static str {
        match self {
            Proto::Start => "START",
            Proto::StartRepeat => "START REPEAT",
            Proto::Stop => "STOP",
            Proto::Ack => "ACK",
            Proto::Nack => "NACK",
            Proto::AddressRead => "ADDRESS READ",
            Proto::AddressWrite => "ADDRESS WRITE",
            Proto::DataRead => "DATA READ",
            Proto::DataWrite => "DATA WRITE",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum State {
    FindStart,
    FindAddress,
    FindData,
    FindAck,
}

/// The I2C decoder.
pub struct I2c {
    shifted: bool,
    samplerate: u64,
    state: State,
    ss: u64,
    es: u64,
    ss_byte: u64,
    bitcount: u32,
    databyte: u32,
    /// -1 unknown, 0 read, 1 write.
    wr: i8,
    is_repeat_start: bool,
    pdu_start: u64,
    pdu_bits: u64,
    bitwidth: u64,
    /// Bits most recent first: value, ss, es.
    bits: Vec<(u8, u64, u64)>,
}

impl Default for I2c {
    fn default() -> Self {
        I2c {
            shifted: true,
            samplerate: 0,
            state: State::FindStart,
            ss: 0,
            es: 0,
            ss_byte: 0,
            bitcount: 0,
            databyte: 0,
            wr: -1,
            is_repeat_start: false,
            pdu_start: 0,
            pdu_bits: 0,
            bitwidth: 0,
            bits: Vec::new(),
        }
    }
}

impl I2c {
    fn putx(&self, ctx: &mut dyn Context, p: Proto) {
        let (long, short) = p.texts();
        ctx.put_ann(self.ss, self.es, p.class(), vec![long.to_owned(), short.to_owned()]);
    }

    fn putp(&self, ctx: &mut dyn Context, p: Proto, data: Item) {
        ctx.put_python(self.ss, self.es, Item::List(vec![Item::Str(p.python_name().into()), data]));
    }

    fn handle_start(&mut self, ctx: &mut dyn Context) {
        let s = ctx.samplenum();
        self.ss = s;
        self.es = s;
        self.pdu_start = s;
        self.pdu_bits = 0;
        let p = if self.is_repeat_start { Proto::StartRepeat } else { Proto::Start };
        self.putp(ctx, p, Item::None);
        self.putx(ctx, p);
        self.state = State::FindAddress;
        self.bitcount = 0;
        self.databyte = 0;
        self.is_repeat_start = true;
        self.wr = -1;
        self.bits.clear();
    }

    fn handle_address_or_data(&mut self, ctx: &mut dyn Context, sda: bool) {
        let s = ctx.samplenum();
        self.pdu_bits += 1;
        self.databyte = (self.databyte << 1) | u32::from(sda);
        if self.bitcount == 0 {
            self.ss_byte = s;
        }
        self.bits.insert(0, (u8::from(sda), s, s));
        if self.bitcount > 0 {
            self.bits[1].2 = s;
        }
        if self.bitcount == 7 {
            self.bitwidth = self.bits[1].2 - self.bits[2].2;
            self.bits[0].2 += self.bitwidth;
        }
        if self.bitcount < 7 {
            self.bitcount += 1;
            return;
        }
        let mut d = self.databyte;
        if self.state == State::FindAddress {
            self.wr = i8::from(self.databyte & 1 == 0);
            if self.shifted {
                d >>= 1;
            }
        }
        let (p, bin_class) = match (self.state, self.wr) {
            (State::FindAddress, 1) => (Proto::AddressWrite, 1),
            (State::FindAddress, _) => (Proto::AddressRead, 0),
            (_, 1) => (Proto::DataWrite, 3),
            _ => (Proto::DataRead, 2),
        };
        self.ss = self.ss_byte;
        self.es = s + self.bitwidth;
        let bit_items: Vec<Item> = self
            .bits
            .iter()
            .map(|&(b, ss, es)| {
                Item::List(vec![
                    Item::Int(i64::from(b)),
                    Item::Int(ss as i64),
                    Item::Int(es as i64),
                ])
            })
            .collect();
        ctx.put_python(
            self.ss,
            self.es,
            Item::List(vec![Item::Str("BITS".into()), Item::List(bit_items)]),
        );
        self.putp(ctx, p, Item::Int(i64::from(d)));
        ctx.put_binary(self.ss, self.es, bin_class, vec![d as u8]);
        for &(b, ss, es) in &self.bits {
            ctx.put_ann(ss, es, 5, vec![b.to_string()]);
        }
        if matches!(p, Proto::AddressRead | Proto::AddressWrite) {
            self.ss = s;
            self.es = s + self.bitwidth;
            let w: &[&str] =
                if self.wr == 1 { &["Write", "Wr", "W"] } else { &["Read", "Rd", "R"] };
            ctx.put_ann(self.ss, self.es, p.class(), w.iter().map(|t| (*t).to_owned()).collect());
            self.ss = self.ss_byte;
            self.es = s;
        }
        let (long, short) = p.texts();
        ctx.put_ann(
            self.ss,
            self.es,
            p.class(),
            vec![format!("{long}: {d:02X}"), format!("{short}: {d:02X}"), format!("{d:02X}")],
        );
        self.bitcount = 0;
        self.databyte = 0;
        self.bits.clear();
        self.state = State::FindAck;
    }

    fn get_ack(&mut self, ctx: &mut dyn Context, sda: bool) {
        let s = ctx.samplenum();
        self.ss = s;
        self.es = s + self.bitwidth;
        let p = if sda { Proto::Nack } else { Proto::Ack };
        self.putp(ctx, p, Item::None);
        self.putx(ctx, p);
        self.state = State::FindData;
    }

    fn handle_stop(&mut self, ctx: &mut dyn Context) {
        let s = ctx.samplenum();
        if self.samplerate > 0 {
            let elapsed = (s - self.pdu_start + 1) as f64 / self.samplerate as f64;
            let bitrate = (1.0 / elapsed * self.pdu_bits as f64) as i64;
            ctx.put_meta(self.ss_byte, s, "Bitrate", Item::Int(bitrate));
        }
        self.ss = s;
        self.es = s;
        self.putp(ctx, Proto::Stop, Item::None);
        self.putx(ctx, Proto::Stop);
        self.state = State::FindStart;
        self.is_repeat_start = false;
        self.wr = -1;
        self.bits.clear();
    }
}

impl Decoder for I2c {
    fn info(&self) -> &'static DecoderInfo {
        &INFO
    }

    fn start(&mut self, config: &Config) -> Result<()> {
        *self =
            I2c { shifted: config.str("address_format", "shifted") == "shifted", ..I2c::default() };
        Ok(())
    }

    fn metadata(&mut self, samplerate: u64) {
        self.samplerate = samplerate;
    }

    fn decode(&mut self, ctx: &mut dyn Context) -> Result<()> {
        if !ctx.has_channel(SCL) || !ctx.has_channel(SDA) {
            return Err(Error::arg("Both SCL and SDA pins are required."));
        }
        let start: &[Cond] = &[Cond::pin(SCL, CondKind::High), Cond::pin(SDA, CondKind::Falling)];
        let stop: &[Cond] = &[Cond::pin(SCL, CondKind::High), Cond::pin(SDA, CondKind::Rising)];
        let rising: &[Cond] = &[Cond::pin(SCL, CondKind::Rising)];
        loop {
            match self.state {
                State::FindStart => {
                    ctx.wait(&[start])?;
                    self.handle_start(ctx);
                }
                State::FindAddress => {
                    let pins = ctx.wait(&[rising])?;
                    self.handle_address_or_data(ctx, pins.get(SDA));
                }
                State::FindData => {
                    let pins = ctx.wait(&[rising, start, stop])?;
                    let m = ctx.matched().to_vec();
                    if m[0] {
                        self.handle_address_or_data(ctx, pins.get(SDA));
                    } else if m[1] {
                        self.handle_start(ctx);
                    } else if m[2] {
                        self.handle_stop(ctx);
                    }
                }
                State::FindAck => {
                    let pins = ctx.wait(&[rising])?;
                    self.get_ack(ctx, pins.get(SDA));
                }
            }
        }
    }
}
