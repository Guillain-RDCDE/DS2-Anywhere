/// DS2 file demuxer for QP (format 6) and QP7 (format 7).
///
/// DS2 SP, variable rate SP and LP files carry the DSS payload behind a bigger
/// header and are read by the DSS block walk in `demux::dss`.
///
/// The QP/QP7 reader is a port of the DS2 demuxer in Patrick Domack's FFmpeg
/// fork (libavformat/ds2.c), which matches Olympus's own decoder
/// (DssDecoder.dll from ODMS R8) frame for frame: frames re-anchor at every
/// block, pause/stop blocks play the frames their byte 5 states, re-sync
/// blocks drop the frame in flight, and a file that ends inside a frame keeps
/// that frame zero-filled.
use crate::error::{DecodeError, Result};

const DS2_BLOCK_SIZE: usize = 512;
const DS2_BLOCK_HEADER_SIZE: usize = 6;
const DS2_BLOCK_PAYLOAD_SIZE: usize = DS2_BLOCK_SIZE - DS2_BLOCK_HEADER_SIZE;
const DS2_QP_FRAME_SIZE: usize = 56;
const DS2_QP7_NOISE_FRAME_SIZE: usize = 12;
const DS2_FORMAT_QP: u8 = 6;
const DS2_FORMAT_QP7: u8 = 7;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ds2QpSegment {
    /// Concatenated 56-byte frames.
    pub stream: Vec<u8>,
    pub total_frames: usize,
    pub reset_before: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ds2Qp7Segment {
    /// One packet per frame: 56 bytes (long frame) or 12 bytes (noise frame).
    pub records: Vec<Vec<u8>>,
    pub total_frames: usize,
    pub reset_before: bool,
}

/// Format byte of the first block that states frames; failing that, of the
/// first block. A cut file's partial last block counts when its header is
/// there, as in the demuxer.
pub(crate) fn detect_ds2_format_type(data: &[u8], header_size: usize) -> u8 {
    let mut bstart = header_size;
    while bstart + DS2_BLOCK_HEADER_SIZE <= data.len() {
        if ds2_block_frames(&data[bstart..bstart + DS2_BLOCK_HEADER_SIZE]) > 0 {
            return data[bstart + 4];
        }
        bstart += DS2_BLOCK_SIZE;
    }
    data.get(header_size + 4).copied().unwrap_or(0)
}

/// Header size in bytes from the first byte of the file: the header size in
/// 512-byte blocks (Olympus 2/3, Grundig/Philips 6/7, newer Olympus firmware
/// 8). Philips SpeechExec writes 1 in the files it splits out of a recording,
/// over the usual 3-block header.
pub(crate) fn ds2_header_size(first_byte: u8) -> usize {
    (if first_byte == 1 {
        3
    } else {
        first_byte as usize
    }) * DS2_BLOCK_SIZE
}

/// Frames that start in a block: byte 2 when it is not 0, otherwise byte 5
/// unless that is 0xff. A recorder writes the second form where it pauses or
/// stops, and Olympus's decoder plays those frames.
fn ds2_block_frames(hdr: &[u8]) -> usize {
    if hdr[2] != 0 || hdr[5] == 0xff {
        hdr[2] as usize
    } else {
        hdr[5] as usize
    }
}

/// Payload offset of the first frame that starts in a block, past the tail of
/// a frame carried over from the previous block.
fn ds2_block_anchor(hdr: &[u8]) -> usize {
    let a = 2 * hdr[1] as isize + 2 * (hdr[0] >> 7) as isize - DS2_BLOCK_HEADER_SIZE as isize;
    a.clamp(0, DS2_BLOCK_PAYLOAD_SIZE as isize) as usize
}

/// Frames that start in a block, from its header and the bytes of it the file
/// holds (`buf`), as the reader takes them. Returns the count and the bytes the
/// last frame of a whole block takes from the blocks after it.
fn ds2_block_frames_held(buf: &[u8], qp7: bool) -> (usize, usize) {
    let size = buf.len();
    let frame_count = ds2_block_frames(buf);
    let mut n = 0;
    let mut off = DS2_BLOCK_HEADER_SIZE + ds2_block_anchor(buf);

    while n < frame_count && off < size {
        n += 1;
        if qp7 && off + 1 < size && buf[off + 1] & 0x80 == 0 {
            off += DS2_QP7_NOISE_FRAME_SIZE;
        } else {
            off += DS2_QP_FRAME_SIZE;
        }
    }
    let need = if size == DS2_BLOCK_SIZE {
        off.saturating_sub(DS2_BLOCK_SIZE)
    } else {
        0
    };
    (n, need)
}

/// Whether the reader drops a frame that the block at `pos` ends `need` bytes
/// into: it does at a re-sync block, whose anchor is not where the rest ends.
/// A block that states no frames passes up to its anchor's worth on to the
/// block after it. Where the file ends, the reader keeps the frame.
fn ds2_frame_dropped(data: &[u8], mut pos: usize, need: usize) -> bool {
    let mut need = need as isize;
    while need > 0 {
        pos += DS2_BLOCK_SIZE;
        let Some(hdr) = data.get(pos..pos + DS2_BLOCK_HEADER_SIZE) else {
            return false;
        };
        let anchor = ds2_block_anchor(hdr) as isize;
        if ds2_block_frames(hdr) != 0 {
            return anchor != need;
        }
        need -= anchor;
    }
    false
}

/// Frames the reader returns that start in the block at `pos`; `buf` holds the
/// bytes of the block the file has.
fn ds2_block_frames_read(data: &[u8], pos: usize, buf: &[u8], qp7: bool) -> usize {
    let (n, need) = ds2_block_frames_held(buf, qp7);
    if need > 0 && ds2_frame_dropped(data, pos, need) {
        n - 1
    } else {
        n
    }
}

fn ds2_count_total_frames(data: &[u8], header_size: usize, qp7: bool) -> u64 {
    let size = data.len();
    let mut total = 0u64;
    let mut pos = header_size;
    while pos < size {
        let len = (size - pos).min(DS2_BLOCK_SIZE);
        if len < DS2_BLOCK_HEADER_SIZE {
            break;
        }
        total += ds2_block_frames_read(data, pos, &data[pos..pos + len], qp7) as u64;
        pos += DS2_BLOCK_SIZE;
    }
    total
}

/// End of the stream (AVERROR_EOF in the fork).
struct Eof;

/// Sequential QP/QP7 frame reader over the file bytes, mirroring
/// ds2_qp_read_frame() and its helpers. `pos` plays the AVIOContext position.
struct QpReader<'a> {
    data: &'a [u8],
    pos: usize,
    qp7: bool,
    /// bytes remaining in the current block payload
    counter: usize,
    total_frames: u64,
    frames_read: u64,
    /// partial frame orphaned at a re-sync block
    resync_drop: bool,
    /// size of the frame being read
    frame_size: usize,
    /// frames still to start in the current block
    blk_frames: isize,
}

impl<'a> QpReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.data.len().saturating_sub(self.pos));
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        n
    }

    /// A forward skip past the end of the file fails; the stream then ends.
    fn skip(&mut self, n: usize) -> std::result::Result<(), Eof> {
        if self.pos + n <= self.data.len() {
            self.pos += n;
            Ok(())
        } else {
            self.pos = self.data.len();
            Err(Eof)
        }
    }

    fn load_block(&mut self, in_frame_offset: usize) -> std::result::Result<(), Eof> {
        if self.total_frames > 0 && self.frames_read >= self.total_frames {
            return Err(Eof);
        }
        let mut hdr = [0u8; DS2_BLOCK_HEADER_SIZE];
        if self.read(&mut hdr) != DS2_BLOCK_HEADER_SIZE {
            return Err(Eof);
        }

        let frame_count = ds2_block_frames(&hdr);
        let anchor = ds2_block_anchor(&hdr);
        self.blk_frames = frame_count as isize;

        if frame_count == 0 {
            self.counter = anchor;
        } else {
            // At a re-sync block the anchor is not where the frame in flight
            // ends: that frame is stale and framing restarts at the anchor.
            let aligned = (in_frame_offset == 0 && anchor == 0)
                || (in_frame_offset != 0 && in_frame_offset + anchor == self.frame_size);
            if !aligned {
                self.resync_drop = true;
                if anchor > 0 {
                    self.skip(anchor)?;
                }
                self.counter = DS2_BLOCK_PAYLOAD_SIZE - anchor;
                return Ok(());
            }
            self.counter = DS2_BLOCK_PAYLOAD_SIZE;
        }
        Ok(())
    }

    /// Skip the rest of the current block and load the next one.
    fn next_block(&mut self, in_frame_offset: usize) -> std::result::Result<(), Eof> {
        let in_block = self.pos % DS2_BLOCK_SIZE;
        self.counter = 0;
        self.resync_drop = false;
        if in_block > 0 {
            self.skip(DS2_BLOCK_SIZE - in_block)?;
        }
        self.load_block(in_frame_offset)
    }

    fn read_frame(&mut self) -> Option<Vec<u8>> {
        if self.total_frames > 0 && self.frames_read >= self.total_frames {
            return None;
        }
        while self.blk_frames <= 0 || self.counter == 0 {
            self.next_block(0).ok()?;
        }

        let mut pkt = [0u8; DS2_QP_FRAME_SIZE];
        let mut offset = 0usize;
        self.frame_size = DS2_QP_FRAME_SIZE;
        let mut truncated = false;

        while offset < self.frame_size {
            let mut to_read = (self.frame_size - offset).min(self.counter);
            if to_read == 0 {
                if self.next_block(offset).is_err() {
                    if offset > 0 && !self.resync_drop {
                        truncated = true;
                        break;
                    }
                    return None;
                }
                if self.resync_drop {
                    offset = 0;
                    self.frame_size = DS2_QP_FRAME_SIZE;
                }
                to_read = (self.frame_size - offset).min(self.counter);
                if to_read == 0 {
                    continue;
                }
            }

            // QP7: the type bit in the first 16-bit word gives the frame size
            if self.qp7 && offset == 0 {
                to_read = to_read.min(2);
            }

            let got = self.read(&mut pkt[offset..offset + to_read]);
            if got < to_read {
                offset += got;
                if offset == 0 {
                    return None;
                }
                truncated = true;
                break;
            }

            if offset == 0 {
                self.blk_frames -= 1;
                if self.qp7 && pkt[1] & 0x80 == 0 {
                    self.frame_size = DS2_QP7_NOISE_FRAME_SIZE;
                }
            }
            offset += to_read;
            self.counter -= to_read;
        }

        if truncated {
            // The file ends inside a frame its block still counts. Olympus
            // decodes that frame too, so keep it, with the missing bytes zeroed.
            pkt[offset..self.frame_size].fill(0);
            self.counter = 0;
        }
        self.frames_read += 1;
        Some(pkt[..self.frame_size].to_vec())
    }
}

/// QP/QP7 packets of a DS2 file, as the fork's demuxer returns them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ds2QpPackets {
    /// 6 (QP) or 7 (QP7)
    pub format_type: u8,
    /// One packet per frame: 56 bytes, or 12 for a QP7 noise frame.
    pub packets: Vec<Vec<u8>>,
}

fn is_ds2_magic(data: &[u8]) -> bool {
    data.len() >= 4 && (1..=16).contains(&data[0]) && &data[1..4] == b"ds2"
}

/// Demux the frames of a DS2 QP or QP7 file.
pub fn demux_ds2_qp(data: &[u8]) -> Result<Ds2QpPackets> {
    if !is_ds2_magic(data) {
        return Err(DecodeError::NotDs2(std::path::PathBuf::from("<bytes>")));
    }
    let header_size = ds2_header_size(data[0]);
    data.get(header_size..header_size + DS2_BLOCK_HEADER_SIZE)
        .ok_or_else(|| DecodeError::Truncated("DS2 audio block header".to_string()))?;
    // The first block with frames says what the file is: an empty block of
    // another format may come first.
    let format_type = detect_ds2_format_type(data, header_size);
    if format_type != DS2_FORMAT_QP && format_type != DS2_FORMAT_QP7 {
        return Err(DecodeError::UnsupportedFormat(format_type));
    }
    let qp7 = format_type == DS2_FORMAT_QP7;

    let mut reader = QpReader {
        data,
        pos: header_size,
        qp7,
        counter: 0,
        total_frames: ds2_count_total_frames(data, header_size, qp7),
        frames_read: 0,
        resync_drop: false,
        frame_size: DS2_QP_FRAME_SIZE,
        blk_frames: 0,
    };
    let mut packets = Vec::new();
    while let Some(pkt) = reader.read_frame() {
        packets.push(pkt);
    }
    Ok(Ds2QpPackets {
        format_type,
        packets,
    })
}

/// Demux a DS2 QP or QP7 file into one segment of frames, decoded without a
/// state reset, as Olympus's decoder and the fork play it.
pub fn demux_ds2(data: &[u8]) -> Result<DemuxedDs2> {
    let Ds2QpPackets {
        format_type,
        packets,
    } = demux_ds2_qp(data)?;
    let total_frames = packets.len();

    if format_type == DS2_FORMAT_QP7 {
        let segments = if packets.is_empty() {
            Vec::new()
        } else {
            vec![Ds2Qp7Segment {
                records: packets,
                total_frames,
                reset_before: false,
            }]
        };
        Ok(DemuxedDs2::Qp7Segments {
            segments,
            total_frames,
        })
    } else {
        let segments = if packets.is_empty() {
            Vec::new()
        } else {
            vec![Ds2QpSegment {
                stream: packets.concat(),
                total_frames,
                reset_before: false,
            }]
        };
        Ok(DemuxedDs2::QpSegments {
            segments,
            total_frames,
        })
    }
}

pub enum DemuxedDs2 {
    QpSegments {
        segments: Vec<Ds2QpSegment>,
        total_frames: usize,
    },
    Qp7Segments {
        segments: Vec<Ds2Qp7Segment>,
        total_frames: usize,
    },
}

/// Buffers the whole file: frame boundaries and the frame total depend on
/// block headers further on (re-sync blocks, the last block).
pub(crate) struct Ds2QpStreamDemuxer {
    data: Vec<u8>,
}

impl Ds2QpStreamDemuxer {
    pub(crate) fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub(crate) fn push(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        self.data.extend_from_slice(data);
        Ok(Vec::new())
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<Vec<u8>>> {
        self.finish_lenient()
    }

    pub(crate) fn finish_lenient(&mut self) -> Result<Vec<Vec<u8>>> {
        Ok(demux_ds2_qp(&self.data)?.packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS2_HEADER_SIZE: usize = 0x600;

    fn block(b0: u8, b1: u8, fc: u8, mode: u8, b5: u8, pattern: u8) -> [u8; DS2_BLOCK_SIZE] {
        let mut block = [0u8; DS2_BLOCK_SIZE];
        block[..6].copy_from_slice(&[b0, b1, fc, 0xff, mode, b5]);
        for (i, byte) in block[DS2_BLOCK_HEADER_SIZE..].iter_mut().enumerate() {
            *byte = pattern.wrapping_add(i as u8) | 0x01;
        }
        block
    }

    fn make_ds2_file(mode: u8, frame_count: u8, payload_pattern: u8) -> Vec<u8> {
        let mut data = vec![0u8; DS2_HEADER_SIZE];
        data[..4].copy_from_slice(b"\x03ds2");
        data.extend_from_slice(&block(0, 0, frame_count, mode, 0xff, payload_pattern));
        data
    }

    #[test]
    fn test_ds2_qp_stream_demux_matches_batch() {
        let data = make_ds2_file(6, 3, 0x40);
        let expected = demux_ds2_qp(&data).unwrap().packets;
        assert_eq!(expected.len(), 3);
        assert_eq!(expected[1], data[0x600 + 6 + 56..0x600 + 6 + 112].to_vec());

        let mut demuxer = Ds2QpStreamDemuxer::new();
        let mut actual = Vec::new();
        for chunk in data.chunks(113) {
            actual.extend(demuxer.push(chunk).unwrap());
        }
        actual.extend(demuxer.finish().unwrap());
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_ds2_qp_block_frame_rule() {
        // byte 2 when non-zero, else byte 5 unless 0xff
        assert_eq!(ds2_block_frames(&[0, 0, 4, 0xff, 6, 7]), 4);
        assert_eq!(ds2_block_frames(&[0, 0, 0, 0xff, 6, 3]), 3);
        assert_eq!(ds2_block_frames(&[0, 0, 0, 0xff, 6, 0xff]), 0);
    }

    #[test]
    fn test_ds2_qp_pause_block_plays_its_frames_and_skips_padding() {
        let mut data = vec![0u8; DS2_HEADER_SIZE];
        data[..4].copy_from_slice(b"\x03ds2");
        data.extend_from_slice(&block(0, 0, 0, 6, 2, 0x10));
        data.extend_from_slice(&block(0, 3, 1, 6, 0xff, 0x20));
        let packets = demux_ds2_qp(&data).unwrap().packets;
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[2], data[0x800 + 6..0x800 + 62].to_vec());
    }

    #[test]
    fn test_ds2_qp_truncated_frame_is_zero_filled() {
        let mut data = make_ds2_file(6, 9, 0x30);
        data.truncate(DS2_HEADER_SIZE + 6 + 56 + 20);
        let packets = demux_ds2_qp(&data).unwrap().packets;
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[1].len(), 56);
        assert!(packets[1][20..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_ds2_qp7_frame_sizes_follow_type_bit() {
        let mut data = make_ds2_file(7, 2, 0x50);
        let p = DS2_HEADER_SIZE + DS2_BLOCK_HEADER_SIZE;
        data[p + 1] = 0x00; // noise frame
        data[p + 12 + 1] = 0x80; // long frame
        let packets = demux_ds2_qp(&data).unwrap().packets;
        assert_eq!(packets.iter().map(Vec::len).collect::<Vec<_>>(), [12, 56]);
    }

    #[test]
    fn test_ds2_qp_demux_never_panics_on_garbage() {
        let mut seed = 0x1234_5678u32;
        for len in [0usize, 4, 0x600, 0x605, 0x606, 0x700, 0x900, 0x1400] {
            for mode in [6u8, 7] {
                let mut data: Vec<u8> = (0..len)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                        (seed >> 16) as u8
                    })
                    .collect();
                if data.len() >= 4 {
                    data[..4].copy_from_slice(b"\x03ds2");
                }
                if data.len() > 0x604 {
                    data[0x604] = mode;
                }
                let _ = demux_ds2_qp(&data);
            }
        }
    }
}
