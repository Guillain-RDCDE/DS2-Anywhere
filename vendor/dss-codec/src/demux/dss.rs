/// DSS block walk: the frames of DSS files, and of DS2 files recorded in SP,
/// variable rate SP or LP, whose audio blocks are the same as in DSS.
///
/// This follows Patrick Domack's libavformat/dss.c (FFmpeg fork, n9.0.2b;
/// the file is Copyright (c) 2014 Oleksij Rempel, LGPL-2.1-or-later, and so is
/// this port of it), which reads the block headers the way Olympus's R8
/// decoder does:
///
/// - each block states where its first frame starts, the byte-swap parity of
///   that frame, and how many frames start in it;
/// - a block written at a pause or stop leaves byte 2 at 0 and puts the frame
///   count in byte 5, and those frames are played;
/// - the byte a swapped frame takes from the frame before it is read from the
///   file, two bytes before the frame, rather than carried over, so a cut file
///   or a block that restarts the framing still decodes;
/// - in variable rate SP (format 1) a frame is 41 or 9 bytes, from its type
///   bit;
/// - LP frames are G.723.1, sized from their first byte.
///
/// A frame that the file ends inside is dropped, as FFmpeg and R8 do.
use crate::error::{DecodeError, Result};

const DSS_BLOCK_SIZE: usize = 512;
const DSS_BLOCK_HEADER_SIZE: usize = 6;
/// Size of an SP packet handed to the decoder.
pub const DSS_SP_FRAME_SIZE: usize = 42;
const DSS_SP_SHORT_FRAME_SIZE: usize = 9;
/// Offset of the codec byte in a DSS file header.
const DSS_HEAD_OFFSET_ACODEC: usize = 0x2a4;
/// G.723.1 frame sizes, by the low two bits of the frame's first byte.
const G723_1_FRAME_SIZE: [usize; 4] = [24, 20, 4, 1];

/// Codec of the audio blocks, the format byte of each block header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DssCodec {
    /// Format 0, SP: 41-byte frames.
    Sp,
    /// Format 1, variable rate SP: 41-byte frames and 9-byte noise frames.
    SpVariableRate,
    /// Format 2, LP: G.723.1 frames.
    Lp,
}

impl DssCodec {
    pub fn from_format_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(DssCodec::Sp),
            1 => Some(DssCodec::SpVariableRate),
            2 => Some(DssCodec::Lp),
            _ => None,
        }
    }

    /// The format byte of an audio block, as DssParser.dll reads it: modes 3
    /// and 5 are LP too, with SID frames among the G.723.1 frames.
    fn from_block_format_byte(byte: u8) -> Option<Self> {
        match byte {
            3 | 5 => Some(DssCodec::Lp),
            _ => Self::from_format_byte(byte),
        }
    }
}

/// Header size from the first byte of the file, in 512-byte blocks. Philips
/// SpeechExec writes 1 in the DS2 files it splits out of a recording, over the
/// usual 3 blocks.
pub fn dss_header_size(first_byte: u8) -> usize {
    (if first_byte == 1 {
        3
    } else {
        first_byte as usize
    }) * DSS_BLOCK_SIZE
}

/// Where the audio starts and which codec it is, for a file read by this walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DssLayout {
    pub header_size: usize,
    pub codec: DssCodec,
}

/// Layout of a DSS file, a DS2 file in SP or LP, or a DSS file without its
/// header. `None` when the file is none of these.
pub fn dss_layout(data: &[u8]) -> Option<DssLayout> {
    if data.len() >= 4 && data[1..4] == *b"dss" && data[0] > 0 && data[0] <= 32 && data[0] != 6 {
        let header_size = dss_header_size(data[0]);
        // Some recorders leave the header's codec field at 0xff (seen on real
        // SP dictations with 3, 8 and 19-block headers); the format byte of
        // the first audio block still says what the frames are.
        let codec = DssCodec::from_format_byte(*data.get(DSS_HEAD_OFFSET_ACODEC)?)
            .or_else(|| DssCodec::from_block_format_byte(*data.get(header_size + 4)?))?;
        return Some(DssLayout { header_size, codec });
    }
    let ds2 = data.len() >= 4
        && (data[1..4] == *b"ds2" || data[..4] == crate::crypto::ds2_encrypted::ENCRYPTED_MAGIC)
        && (1..=16).contains(&data[0]);
    if ds2 {
        // DS2 leaves the DSS codec field unset; each block header states it.
        let header_size = dss_header_size(data[0]);
        let codec = DssCodec::from_format_byte(*data.get(header_size + 4)?)?;
        return Some(DssLayout { header_size, codec });
    }
    if looks_like_headerless_dss(data) {
        let codec = DssCodec::from_format_byte(data[4])?;
        return Some(DssLayout {
            header_size: 0,
            codec,
        });
    }
    None
}

/// Frames that start in an audio block: byte 2 when it is not 0, otherwise
/// byte 5 unless that is 0xff. A recorder writes the second form where it
/// pauses or stops, and Olympus's decoder plays those frames.
pub(crate) fn dss_block_frames(header: &[u8]) -> usize {
    if header[2] != 0 || header[5] == 0xff {
        header[2] as usize
    } else {
        header[5] as usize
    }
}

/// Un fichier DSS dont l'en-tete a ete perdu commence directement par ses blocs
/// audio. Le seul moyen sur de le reconnaitre est la structure : sur chaque
/// bloc, l'octet 3 vaut 0xFF, l'octet 4 donne le mode, et surtout la
/// continuation qu'un bloc laisse doit etre exactement celle que le suivant
/// annonce. Cette chaine ne tient pas par hasard : dix blocs de suite suffisent.
pub fn looks_like_headerless_dss(data: &[u8]) -> bool {
    const A_VERIFIER: usize = 24;
    let blocs = data.len() / DSS_BLOCK_SIZE;
    if blocs < A_VERIFIER {
        return false;
    }
    let payload = DSS_BLOCK_SIZE - DSS_BLOCK_HEADER_SIZE;
    let mut report = 0usize;
    let mut liens = 0usize;
    let mut tenus = 0usize;
    for bi in 0..A_VERIFIER {
        let h = &data[bi * DSS_BLOCK_SIZE..bi * DSS_BLOCK_SIZE + DSS_BLOCK_HEADER_SIZE];
        if h[3] != 0xff || h[4] != 0 || h[2] as usize > 13 {
            return false;
        }
        let swap = ((h[0] >> 7) & 1) as usize;
        let cont = (2 * h[1] as usize + 2 * swap).saturating_sub(DSS_BLOCK_HEADER_SIZE);
        if h[2] == 0 {
            report = 0;
            continue;
        }
        if bi > 0 {
            liens += 1;
            if cont == report {
                tenus += 1;
            }
        }
        let mut p = cont;
        let mut sw = swap;
        for _ in 0..h[2] {
            p += if sw != 0 { 40 } else { DSS_SP_FRAME_SIZE };
            sw ^= 1;
        }
        report = p.saturating_sub(payload);
    }
    // Une coupure de prise rompt legitimement la chaine ; du hasard ne la tient
    // jamais. Quatre cinquiemes des liens suffisent a trancher.
    liens >= 12 && tenus * 5 >= liens * 4
}

/// The reader. Offsets are file offsets; `pos` is where the next byte is read.
struct Walk<'a> {
    data: &'a [u8],
    header_size: usize,
    pos: usize,
    /// Offset of the header of the current block, one block before the first
    /// audio block until a block is read.
    block_pos: isize,
    frames_left: usize,
    swap: bool,
    /// The byte a swapped frame takes from the frame before it, if known.
    swap_byte: Option<u8>,
}

enum Frame {
    Packet(Vec<u8>),
    /// A swapped frame whose borrowed byte is not in the file: left out.
    Dropped,
}

impl<'a> Walk<'a> {
    fn new(data: &'a [u8], header_size: usize) -> Self {
        Self {
            data,
            header_size,
            pos: header_size,
            block_pos: header_size as isize - DSS_BLOCK_SIZE as isize,
            frames_left: 0,
            swap: false,
            swap_byte: None,
        }
    }

    /// Read `buf.len()` bytes of frame payload, stepping over any block header
    /// met on the way. False when the file ends first.
    fn read_payload(&mut self, buf: &mut [u8]) -> bool {
        let mut done = 0;
        while done < buf.len() {
            let off = (self.pos - self.header_size) % DSS_BLOCK_SIZE;
            if off < DSS_BLOCK_HEADER_SIZE {
                self.pos += DSS_BLOCK_HEADER_SIZE - off;
                continue;
            }
            let chunk = (buf.len() - done).min(DSS_BLOCK_SIZE - off);
            let Some(src) = self.data.get(self.pos..self.pos + chunk) else {
                return false;
            };
            buf[done..done + chunk].copy_from_slice(src);
            self.pos += chunk;
            done += chunk;
        }
        true
    }

    /// Move to the next audio block that holds frames and take the framing
    /// from its header. False at the end of the file.
    fn next_block(&mut self) -> bool {
        loop {
            self.block_pos += DSS_BLOCK_SIZE as isize;
            let start = self.block_pos as usize;
            let Some(header) = self.data.get(start..start + DSS_BLOCK_HEADER_SIZE) else {
                return false;
            };

            self.swap = header[0] & 0x80 != 0;
            let mut cont =
                2 * header[1] as isize + 2 * self.swap as isize - DSS_BLOCK_HEADER_SIZE as isize;
            if cont < 0 || cont > (DSS_BLOCK_SIZE - DSS_BLOCK_HEADER_SIZE) as isize {
                cont = 0;
            }
            let cont = cont as usize;

            self.frames_left = dss_block_frames(header);
            if self.frames_left == 0 {
                continue;
            }

            // A swapped frame takes one byte from the frame before it, two
            // bytes before its own start, in this block or at the end of the
            // previous one.
            if self.swap {
                let mut at = start as isize + cont as isize - 2;
                if cont >= 2 {
                    at += DSS_BLOCK_HEADER_SIZE as isize;
                }
                if at >= (self.header_size + DSS_BLOCK_HEADER_SIZE) as isize {
                    // avio_r8 past the end gives 0.
                    self.swap_byte = Some(self.data.get(at as usize).copied().unwrap_or(0));
                }
            }

            self.pos = start + DSS_BLOCK_HEADER_SIZE + cont;
            return true;
        }
    }

    fn sp_frame(&mut self, variable_rate: bool) -> Option<Frame> {
        if self.frames_left == 0 && !self.next_block() {
            return None;
        }

        // The packet keeps one byte of padding past the frame, which a swapped
        // long frame reads into and the decoder never sees.
        let mut pkt = [0u8; DSS_SP_FRAME_SIZE + 1];
        let mut size = DSS_SP_FRAME_SIZE - 1;
        let mut done = 0;

        // In variable rate SP the top bit of the frame's first byte, in 16-bit
        // word order, is clear for a short frame. A swapped frame starts with
        // the byte carried over from the previous frame; otherwise it is the
        // second byte read.
        if variable_rate {
            if self.swap {
                if matches!(self.swap_byte, Some(b) if b & 0x80 == 0) {
                    size = DSS_SP_SHORT_FRAME_SIZE;
                }
            } else {
                if !self.read_payload(&mut pkt[..2]) {
                    return None;
                }
                done = 2;
                if pkt[1] & 0x80 == 0 {
                    size = DSS_SP_SHORT_FRAME_SIZE;
                }
            }
        }

        // A swapped frame reads one byte less than its size, starting at
        // offset 3; an unswapped one reads one byte more.
        let ok = if self.swap {
            self.read_payload(&mut pkt[3..3 + size - 1])
        } else {
            self.read_payload(&mut pkt[done..size + 1])
        };
        if !ok {
            return None;
        }
        self.frames_left -= 1;

        if self.swap {
            let mut i = 0;
            while i < size - 1 {
                pkt[i] = pkt[i + 4];
                i += 2;
            }
            pkt[1] = self.swap_byte.unwrap_or(0);
        } else {
            self.swap_byte = Some(pkt[size - 1]);
        }
        for b in &mut pkt[size + 1..] {
            *b = 0;
        }
        pkt[size - 1] = 0;
        self.swap = !self.swap;

        if self.swap_byte.is_none() {
            return Some(Frame::Dropped);
        }
        Some(Frame::Packet(pkt[..DSS_SP_FRAME_SIZE].to_vec()))
    }

    fn lp_frame(&mut self) -> Option<Frame> {
        if self.frames_left == 0 && !self.next_block() {
            return None;
        }
        let mut first = [0u8; 1];
        if !self.read_payload(&mut first) || first[0] == 0xff {
            return None;
        }
        let size = G723_1_FRAME_SIZE[(first[0] & 3) as usize];
        let mut pkt = vec![0u8; size];
        pkt[0] = first[0];
        if !self.read_payload(&mut pkt[1..]) {
            return None;
        }
        self.frames_left -= 1;
        Some(Frame::Packet(pkt))
    }
}

/// Every frame of the file, as the packets the decoder takes: 42 bytes for SP
/// and variable rate SP, the G.723.1 frame for LP.
pub fn demux_dss_packets(data: &[u8], layout: DssLayout) -> Vec<Vec<u8>> {
    let mut walk = Walk::new(data, layout.header_size);
    let mut packets = Vec::new();
    loop {
        let frame = match layout.codec {
            DssCodec::Sp => walk.sp_frame(false),
            DssCodec::SpVariableRate => walk.sp_frame(true),
            DssCodec::Lp => walk.lp_frame(),
        };
        match frame {
            Some(Frame::Packet(pkt)) => packets.push(pkt),
            Some(Frame::Dropped) => {}
            None => break,
        }
    }
    packets
}

/// SP packets of a DSS file, for callers that only handle format 0.
pub fn demux_dss(data: &[u8]) -> Result<(Vec<Vec<u8>>, usize)> {
    let layout =
        dss_layout(data).ok_or_else(|| DecodeError::NotDss(std::path::PathBuf::from("<bytes>")))?;
    let packets = demux_dss_packets(data, layout);
    let total = packets.len();
    Ok((packets, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dss_file(codec_byte: u8, blocks: &[[u8; DSS_BLOCK_SIZE]]) -> Vec<u8> {
        let mut data = vec![0u8; 2 * DSS_BLOCK_SIZE];
        data[0] = 2;
        data[1..4].copy_from_slice(b"dss");
        data[DSS_HEAD_OFFSET_ACODEC] = codec_byte;
        for b in blocks {
            data.extend_from_slice(b);
        }
        data
    }

    fn block(swap: u8, byte1: u8, frames: u8, byte5: u8, fill: u8) -> [u8; DSS_BLOCK_SIZE] {
        let mut b = [fill; DSS_BLOCK_SIZE];
        b[0] = swap << 7;
        b[1] = byte1;
        b[2] = frames;
        b[3] = 0xff;
        b[4] = 0;
        b[5] = byte5;
        b
    }

    #[test]
    fn frames_in_pause_blocks_are_read() {
        // Byte 2 at 0 and byte 5 giving the count: a pause block.
        let data = dss_file(0, &[block(0, 3, 0, 4, 0x11)]);
        let layout = dss_layout(&data).unwrap();
        assert_eq!(demux_dss_packets(&data, layout).len(), 4);
    }

    #[test]
    fn byte5_0xff_means_no_frames() {
        let data = dss_file(0, &[block(0, 3, 0, 0xff, 0x11)]);
        let layout = dss_layout(&data).unwrap();
        assert!(demux_dss_packets(&data, layout).is_empty());
    }

    #[test]
    fn a_frame_cut_by_the_end_of_the_file_is_dropped() {
        // 13 frames would need 13 * 41 = 533 bytes, the block holds 506.
        let data = dss_file(0, &[block(0, 3, 13, 0, 0x22)]);
        let layout = dss_layout(&data).unwrap();
        assert_eq!(demux_dss_packets(&data, layout).len(), 12);
    }

    #[test]
    fn frames_run_on_into_the_next_block() {
        let data = dss_file(
            0,
            &[block(0, 3, 13, 0, 0x22), block(1, 3 + 13, 0, 0xff, 0x33)],
        );
        let layout = dss_layout(&data).unwrap();
        let packets = demux_dss_packets(&data, layout);
        assert_eq!(packets.len(), 13);
        assert!(packets.iter().all(|p| p.len() == DSS_SP_FRAME_SIZE));
    }

    #[test]
    fn a_first_swapped_frame_without_its_byte_is_left_out() {
        // The first block of the file starts on a swapped frame: the byte it
        // borrows is before the audio, so the frame is dropped.
        let data = dss_file(0, &[block(1, 2, 2, 0, 0x44)]);
        let layout = dss_layout(&data).unwrap();
        assert_eq!(demux_dss_packets(&data, layout).len(), 1);
    }

    #[test]
    fn variable_rate_short_frames_are_nine_bytes() {
        // All bytes 0: every type bit is clear, so every frame is short.
        let data = dss_file(1, &[block(0, 3, 40, 0, 0x00)]);
        let layout = dss_layout(&data).unwrap();
        assert_eq!(layout.codec, DssCodec::SpVariableRate);
        // 40 short frames take 360 bytes and fit in the block.
        assert_eq!(demux_dss_packets(&data, layout).len(), 40);
    }

    #[test]
    fn lp_frames_take_their_size_from_the_first_byte() {
        let mut b = block(0, 3, 3, 0, 0);
        b[6] = 0; // 24 bytes
        b[30] = 1; // 20 bytes
        b[50] = 2; // 4 bytes
        let data = dss_file(2, &[b]);
        let layout = dss_layout(&data).unwrap();
        let packets = demux_dss_packets(&data, layout);
        let sizes: Vec<usize> = packets.iter().map(|p| p.len()).collect();
        assert_eq!(sizes, vec![24, 20, 4]);
    }

    #[test]
    fn an_unset_header_codec_falls_back_to_the_block() {
        let mut data = dss_file(0xff, &[block(0, 3, 2, 0, 0x55)]);
        assert_eq!(dss_layout(&data).unwrap().codec, DssCodec::Sp);
        // A header of 19 blocks, as one real recorder writes.
        let mut big = vec![0u8; 19 * DSS_BLOCK_SIZE];
        big[..1024].copy_from_slice(&data[..1024]);
        big[0] = 19;
        big.extend_from_slice(&data[1024..]);
        data = big;
        let layout = dss_layout(&data).unwrap();
        assert_eq!(layout.header_size, 19 * DSS_BLOCK_SIZE);
        assert_eq!(demux_dss_packets(&data, layout).len(), 2);
    }

    #[test]
    fn ds2_sp_and_lp_use_the_block_format_byte() {
        for (fmt, codec) in [
            (0u8, DssCodec::Sp),
            (1, DssCodec::SpVariableRate),
            (2, DssCodec::Lp),
        ] {
            let mut data = vec![0u8; 3 * DSS_BLOCK_SIZE];
            data[..4].copy_from_slice(b"\x03ds2");
            let mut b = block(0, 3, 1, 0, 0);
            b[4] = fmt;
            data.extend_from_slice(&b);
            assert_eq!(dss_layout(&data).unwrap().codec, codec);
            // A file split out by SpeechExec says 1 for its 3-block header.
            data[0] = 1;
            assert_eq!(dss_layout(&data).unwrap().header_size, 3 * DSS_BLOCK_SIZE);
        }
    }

    #[test]
    fn walk_never_panics_on_garbage() {
        let mut seed = 0x1234_5678u32;
        for len in [0usize, 5, 600, 1030, 1536, 3000, 5000] {
            for codec in [DssCodec::Sp, DssCodec::SpVariableRate, DssCodec::Lp] {
                let data: Vec<u8> = (0..len)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                        (seed >> 16) as u8
                    })
                    .collect();
                for header_size in [0usize, 1024, 1536] {
                    let _ = demux_dss_packets(&data, DssLayout { header_size, codec });
                }
            }
        }
    }
}
