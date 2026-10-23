// Counterpart in sigrok:
//   libsigrok/src/hardware/intronix-logicport/protocol.c (the ccf loader),
//   Copyright (C) 2026 Miguel Angel Ajo Pelayo <miguelangel@ajo.es>
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port by the same author, GPL-3.0-or-later like it.

//! The FPGA configuration images.
//!
//! `LogicPort.ccf` from the vendor software is used unmodified, installed
//! as `intronix-logicport.ccf`: a text line of nine XOR obfuscated
//! decimal numbers (eight image sizes in 16 byte units and their XOR)
//! followed by the images, each a ready bit-bang stream (two bytes per
//! bit, data in bit 0, nCONFIG in bit 2, DCLK in bit 1) whose first two
//! 4 KiB blocks are swapped. Per image `.rbf` files, as
//! `sigrok-fwextract-intronix-logicport` writes them, are accepted too.

use oxrok_core::{Error, Result};

/// The file name looked up in the firmware directories.
pub const FILE: &str = "intronix-logicport.ccf";
const CCF_KEY: u32 = 0x0091_6662;
const SWAP_BLOCK: usize = 4096;

/// The eight images, one per sampling mode. In state mode the image
/// fixes where data is sampled relative to the external clock edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Image {
    /// State mode, sampled 2.5 ns after the edge.
    StateAfter = 0,
    /// State mode, 2.5 ns before a CLK1 rising edge.
    StateBeforeClk1Rise = 1,
    /// State mode, 2.5 ns before a CLK2 rising edge.
    StateBeforeClk2Rise = 2,
    /// State mode, 2.5 ns before a CLK1 falling edge.
    StateBeforeClk1Fall = 3,
    /// State mode, 2.5 ns before a CLK2 falling edge.
    StateBeforeClk2Fall = 4,
    /// State mode, sampled on the edge.
    StateOn = 5,
    /// Timing mode, 500 MHz double data rate.
    TimingDdr = 6,
    /// Timing mode, up to 250 MHz.
    TimingSdr = 7,
}

/// How many images the file holds.
pub const IMAGE_COUNT: usize = 8;

impl Image {
    /// All images, in file order.
    pub const ALL: [Image; IMAGE_COUNT] = [
        Image::StateAfter,
        Image::StateBeforeClk1Rise,
        Image::StateBeforeClk2Rise,
        Image::StateBeforeClk1Fall,
        Image::StateBeforeClk2Fall,
        Image::StateOn,
        Image::TimingDdr,
        Image::TimingSdr,
    ];

    /// The image with index `i`, as the FPGA reports it.
    pub fn from_index(i: u8) -> Option<Image> {
        Image::ALL.get(usize::from(i)).copied()
    }

    /// The index in the file and in the FPGA's id register.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The name used in `.rbf` file names and logs.
    pub fn name(self) -> &'static str {
        match self {
            Image::StateAfter => "state-after",
            Image::StateBeforeClk1Rise => "state-before-clk1-rise",
            Image::StateBeforeClk2Rise => "state-before-clk2-rise",
            Image::StateBeforeClk1Fall => "state-before-clk1-fall",
            Image::StateBeforeClk2Fall => "state-before-clk2-fall",
            Image::StateOn => "state-on",
            Image::TimingDdr => "timing-ddr",
            Image::TimingSdr => "timing-sdr",
        }
    }
}

pub use oxrok_core::firmware::{find_file, search_dirs};

/// Parses the header line: nine comma separated numbers, XOR obfuscated.
/// Returns the eight image sizes in 16 byte units.
pub fn parse_header(line: &str) -> Result<[u32; IMAGE_COUNT]> {
    let bad = || Error::device(format!("'{FILE}' is not a LogicPort.ccf file"));
    let fields: Vec<&str> = line.trim_end_matches(['\r', '\n']).split(',').collect();
    if fields.len() != IMAGE_COUNT + 1 {
        return Err(bad());
    }
    let mut units = [0u32; IMAGE_COUNT];
    let mut check = 0u32;
    for (i, f) in fields.iter().enumerate() {
        let v = f.trim().parse::<u32>().map_err(|_| bad())? ^ CCF_KEY;
        if i < IMAGE_COUNT {
            units[i] = v;
            check ^= v;
        } else if v != check {
            return Err(bad());
        }
    }
    Ok(units)
}

/// True when the bit-bang stream starts with an Altera RBF header:
/// 16 bytes of 0xff then 0x6a, least significant bit first.
pub fn stream_is_rbf(stream: &[u8]) -> bool {
    if stream.len() < 17 * 16 {
        return false;
    }
    (0..17).all(|i| {
        let byte = (0..8).fold(0u8, |b, k| b | ((stream[(i * 8 + k) * 2] & 1) << k));
        byte == if i < 16 { 0xff } else { 0x6a }
    })
}

/// Extracts `image` from the contents of a `.ccf` file as a ready
/// bit-bang stream.
pub fn image_from_ccf(data: &[u8], image: Image) -> Result<Vec<u8>> {
    let eol = data
        .iter()
        .position(|&b| b == b'\n')
        .filter(|&p| p < 128)
        .ok_or_else(|| Error::device(format!("'{FILE}' is not a LogicPort.ccf file")))?;
    let line = std::str::from_utf8(&data[..eol])
        .map_err(|_| Error::device(format!("'{FILE}' is not a LogicPort.ccf file")))?;
    let units = parse_header(line)?;
    let sizes: Vec<usize> = units.iter().map(|&u| u as usize * 16).collect();
    let total: usize = sizes.iter().sum();
    let size = sizes[image.index()];
    if eol + 1 + total != data.len() || size < 2 * SWAP_BLOCK {
        return Err(Error::device(format!("'{FILE}' has an unexpected size")));
    }
    let offset = eol + 1 + sizes[..image.index()].iter().sum::<usize>();
    let mut stream = data[offset..offset + size].to_vec();
    let (a, b) = stream.split_at_mut(SWAP_BLOCK);
    a.swap_with_slice(&mut b[..SWAP_BLOCK]);
    if !stream_is_rbf(&stream) {
        return Err(Error::device(format!(
            "image {} in '{FILE}' is not a valid bitstream",
            image.index()
        )));
    }
    Ok(stream)
}

/// Turns a raw `.rbf` bitstream into the bit-bang stream: two bytes per
/// bit, least significant bit first, nCONFIG held high, DCLK low then
/// high.
pub fn stream_from_rbf(rbf: &[u8]) -> Vec<u8> {
    const NCONFIG: u8 = 1 << 2;
    const DCLK: u8 = 1 << 1;
    let mut out = Vec::with_capacity(rbf.len() * 16);
    for &byte in rbf {
        for k in 0..8 {
            let bit = (byte >> k) & 1;
            out.push(NCONFIG | bit);
            out.push(NCONFIG | DCLK | bit);
        }
    }
    out
}

/// Loads `image` from the firmware directories: the `.ccf` file, or
/// `intronix-logicport-<name>.rbf` when that is what is installed.
pub fn load_image(image: Image) -> Result<Vec<u8>> {
    let rbf_name = format!("intronix-logicport-{}.rbf", image.name());
    if find_file(FILE).is_none() && find_file(&rbf_name).is_some() {
        let stream = stream_from_rbf(&oxrok_core::firmware::read(&rbf_name, "")?);
        if !stream_is_rbf(&stream) {
            return Err(Error::device(format!("'{rbf_name}' is not a valid bitstream")));
        }
        return Ok(stream);
    }
    let data = oxrok_core::firmware::read(
        FILE,
        &format!(
            "or {rbf_name}; get it with sigrok-fwextract-intronix-logicport, see docs/DEVICES.md"
        ),
    )?;
    image_from_ccf(&data, image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(sizes: [u32; 8]) -> String {
        let check = sizes.iter().fold(0, |a, b| a ^ b);
        let mut v: Vec<String> = sizes.iter().map(|s| (s ^ CCF_KEY).to_string()).collect();
        v.push((check ^ CCF_KEY).to_string());
        v.join(",")
    }

    #[test]
    fn header_round_trip() {
        let sizes = [66115, 65282, 65431, 65458, 65407, 65937, 64452, 66149];
        assert_eq!(parse_header(&(header(sizes) + "\r\n")).unwrap(), sizes);
        assert!(parse_header("1,2,3").is_err());
        let mut bad = header(sizes);
        bad.push_str(",1");
        assert!(parse_header(&bad).is_err());
    }

    #[test]
    fn rbf_conversion_and_check() {
        let mut rbf = vec![0xff; 16];
        rbf.push(0x6a);
        rbf.extend(std::iter::repeat_n(0x55, 600));
        let stream = stream_from_rbf(&rbf);
        assert_eq!(stream.len(), rbf.len() * 16);
        assert_eq!(&stream[..4], [0x05, 0x07, 0x05, 0x07]);
        assert!(stream_is_rbf(&stream));
        let mut broken = stream.clone();
        broken[16 * 16] ^= 1;
        assert!(!stream_is_rbf(&broken));
    }

    #[test]
    fn ccf_extraction_swaps_the_first_blocks() {
        // Two images of 8 KiB (512 units) each.
        let mut rbf = vec![0xff; 16];
        rbf.push(0x6a);
        rbf.extend(std::iter::repeat_n(0xa5, 512 - 17));
        let image = stream_from_rbf(&rbf);
        assert_eq!(image.len(), 8192);
        let mut swapped = image.clone();
        let (a, b) = swapped.split_at_mut(SWAP_BLOCK);
        a.swap_with_slice(b);
        let mut file = (header([512, 512, 512, 512, 512, 512, 512, 512]) + "\r\n").into_bytes();
        for _ in 0..8 {
            file.extend_from_slice(&swapped);
        }
        assert_eq!(image_from_ccf(&file, Image::TimingSdr).unwrap(), image);
        assert_eq!(image_from_ccf(&file, Image::StateAfter).unwrap(), image);
        file.pop();
        assert!(image_from_ccf(&file, Image::StateOn).is_err());
    }
}
