/// Files whose block format changes partway: "mixed" files.
///
/// Olympus's DssRawWriter (ODMS R8) can write one DSS or DS2 file whose
/// audio blocks change format partway, SP then QP, QP then QP7, QP7 then LP.
/// Each block header states its format in byte 4, and Olympus's R8 decoder
/// plays each run of blocks with the codec that byte names. Read with the
/// codec of the first block only, everything after the change is noise.
///
/// The rules, as Patrick Domack's dss-codec (hirparak/dss-codec#28) reads
/// these files:
///
/// - byte 4 counts only when byte 3 of the block header is 0xff;
/// - formats 0 and 1 are SP and variable rate SP, 2 and 4 LP, 6 QP and 7 QP7;
///   any other value names no codec;
/// - a block that states no frames, or names no codec, decides nothing: it
///   belongs to the part in front of it, or, before the first block that
///   decides, to the first part.
///
/// A file is mixed when its blocks name two codecs or more; a file whose
/// blocks name one codec, or none, is not, and decodes exactly as before.
/// Each part is then read as a file of its own whose audio is that part's
/// blocks: the block walks never see the blocks of another part.
use crate::crypto::ds2_encrypted::ENCRYPTED_MAGIC;
use crate::demux::dss::{dss_block_frames, dss_header_size};
use crate::demux::AudioFormat;

const BLOCK_SIZE: usize = 512;
const BLOCK_HEADER_SIZE: usize = 6;

/// Codec of a part of a mixed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartCodec {
    /// Format 0: SP.
    Sp,
    /// Format 1: variable rate SP.
    SpVariableRate,
    /// Formats 2 and 4: LP, G.723.1 frames.
    Lp,
    /// Format 6: QP.
    Qp,
    /// Format 7: QP7.
    Qp7,
}

impl PartCodec {
    /// The codec a block header names, if it decides one: byte 3 is 0xff,
    /// the block states frames and byte 4 is a known format.
    pub fn from_block_header(header: &[u8]) -> Option<Self> {
        if header[3] != 0xff || dss_block_frames(header) == 0 {
            return None;
        }
        match header[4] {
            0 => Some(PartCodec::Sp),
            1 => Some(PartCodec::SpVariableRate),
            2 | 4 => Some(PartCodec::Lp),
            6 => Some(PartCodec::Qp),
            7 => Some(PartCodec::Qp7),
            _ => None,
        }
    }

    /// Rate this crate decodes the codec at.
    pub fn native_rate(self) -> u32 {
        self.format(true).native_sample_rate()
    }

    /// The format of the part, in a DS2 file or a DSS file.
    pub fn format(self, ds2: bool) -> AudioFormat {
        match (self, ds2) {
            (PartCodec::Sp | PartCodec::SpVariableRate, false) => AudioFormat::DssSp,
            (PartCodec::Sp | PartCodec::SpVariableRate, true) => AudioFormat::Ds2Sp,
            (PartCodec::Lp, false) => AudioFormat::DssLp,
            (PartCodec::Lp, true) => AudioFormat::Ds2Lp,
            (PartCodec::Qp, _) => AudioFormat::Ds2Qp,
            (PartCodec::Qp7, _) => AudioFormat::Ds2Qp7,
        }
    }
}

/// A run of audio blocks of one codec: file offsets `start..end`. `start` is
/// the offset of a block header; `end` is the next part's `start`, or the end
/// of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartSpan {
    pub start: usize,
    pub end: usize,
    pub codec: PartCodec,
}

/// The parts of a mixed file, in file order, and whether it is a DS2 file.
/// `None` when the file is not a DSS or DS2 file read by block, or when its
/// blocks name fewer than two codecs.
pub fn mixed_spans(data: &[u8]) -> Option<(bool, Vec<PartSpan>)> {
    if data.len() < 4 {
        return None;
    }
    // The containers the block walks read (Grundig files, first byte 6, have
    // their own demuxer).
    let dss = data[1..4] == *b"dss" && data[0] > 0 && data[0] <= 32 && data[0] != 6;
    let ds2 =
        (data[1..4] == *b"ds2" || data[..4] == ENCRYPTED_MAGIC) && (1..=16).contains(&data[0]);
    if !dss && !ds2 {
        return None;
    }
    let header_size = dss_header_size(data[0]);

    let mut spans: Vec<PartSpan> = Vec::new();
    let mut pos = header_size;
    while pos + BLOCK_HEADER_SIZE <= data.len() {
        if let Some(codec) = PartCodec::from_block_header(&data[pos..pos + BLOCK_HEADER_SIZE]) {
            match spans.last_mut() {
                Some(last) if last.codec == codec => {}
                Some(last) => {
                    last.end = pos;
                    spans.push(PartSpan {
                        start: pos,
                        end: data.len(),
                        codec,
                    });
                }
                // The blocks in front of the first that decides belong to it.
                None => spans.push(PartSpan {
                    start: header_size,
                    end: data.len(),
                    codec,
                }),
            }
        }
        pos += BLOCK_SIZE;
    }
    (spans.len() > 1).then_some((ds2, spans))
}

/// Format a mixed file decodes to: that of its first part at the highest
/// rate among its parts, which is the rate of the decoded file.
pub fn mixed_format(data: &[u8]) -> Option<AudioFormat> {
    let (ds2, spans) = mixed_spans(data)?;
    let rate = spans.iter().map(|s| s.codec.native_rate()).max()?;
    spans
        .iter()
        .find(|s| s.codec.native_rate() == rate)
        .map(|s| s.codec.format(ds2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(byte3: u8, format: u8, frames: u8) -> [u8; BLOCK_SIZE] {
        let mut b = [0u8; BLOCK_SIZE];
        b[..6].copy_from_slice(&[0x0f, 3, frames, byte3, format, 0xff]);
        b
    }

    fn ds2(blocks: &[[u8; BLOCK_SIZE]]) -> Vec<u8> {
        let mut data = vec![0u8; 3 * BLOCK_SIZE];
        data[..4].copy_from_slice(b"\x03ds2");
        for b in blocks {
            data.extend_from_slice(b);
        }
        data
    }

    #[test]
    fn one_codec_is_not_mixed() {
        assert_eq!(
            mixed_spans(&ds2(&[block(0xff, 6, 9), block(0xff, 6, 9)])),
            None
        );
        // An empty block of another format decides nothing.
        assert_eq!(
            mixed_spans(&ds2(&[block(0xff, 2, 0), block(0xff, 0, 9)])),
            None
        );
        // Nor does a block whose byte 3 is not 0xff.
        assert_eq!(
            mixed_spans(&ds2(&[block(0xff, 6, 9), block(0, 0, 9)])),
            None
        );
        // Nor an unknown format.
        assert_eq!(
            mixed_spans(&ds2(&[block(0xff, 3, 9), block(0xff, 0, 9)])),
            None
        );
        assert_eq!(mixed_spans(&ds2(&[])), None);
    }

    #[test]
    fn each_codec_change_starts_a_part() {
        let data = ds2(&[
            block(0xff, 2, 0),
            block(0xff, 3, 9),
            block(0xff, 0, 9),
            block(0xff, 0, 9),
            block(0, 6, 9),
            block(0xff, 6, 9),
            block(0xff, 7, 0),
            block(0xff, 8, 9),
            block(0xff, 4, 9),
            block(0xff, 2, 9),
        ]);
        let h = 3 * BLOCK_SIZE;
        let at = |i: usize| h + i * BLOCK_SIZE;
        let (is_ds2, spans) = mixed_spans(&data).unwrap();
        assert!(is_ds2);
        assert_eq!(
            spans,
            vec![
                PartSpan {
                    start: h,
                    end: at(5),
                    codec: PartCodec::Sp
                },
                PartSpan {
                    start: at(5),
                    end: at(8),
                    codec: PartCodec::Qp
                },
                PartSpan {
                    start: at(8),
                    end: data.len(),
                    codec: PartCodec::Lp
                },
            ]
        );
        assert_eq!(mixed_format(&data), Some(AudioFormat::Ds2Qp));
    }

    #[test]
    fn a_dss_file_names_dss_formats() {
        let mut data = vec![0u8; 2 * BLOCK_SIZE];
        data[..4].copy_from_slice(b"\x02dss");
        data.extend_from_slice(&block(0xff, 1, 9));
        data.extend_from_slice(&block(0xff, 2, 9));
        let (is_ds2, spans) = mixed_spans(&data).unwrap();
        assert!(!is_ds2);
        assert_eq!(spans.len(), 2);
        assert_eq!(mixed_format(&data), Some(AudioFormat::DssSp));
        // Grundig files are not read by block here.
        data[0] = 6;
        assert_eq!(mixed_spans(&data), None);
    }

    #[test]
    fn spans_never_panic_on_garbage() {
        let mut seed = 0x2468_ace0u32;
        for len in [0usize, 3, 4, 600, 1030, 1536, 1541, 1542, 3000, 9000] {
            for magic in [*b"\x03ds2", *b"\x02dss", ENCRYPTED_MAGIC, *b"\x10ds2"] {
                let mut data: Vec<u8> = (0..len)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                        (seed >> 16) as u8
                    })
                    .collect();
                if len >= 4 {
                    data[..4].copy_from_slice(&magic);
                }
                let _ = mixed_spans(&data);
                let _ = mixed_format(&data);
            }
        }
    }
}
