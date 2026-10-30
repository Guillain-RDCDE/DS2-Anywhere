pub mod ds2;
pub mod dss;
pub mod grundig;
pub mod parts;

use crate::crypto::ds2_encrypted::ENCRYPTED_MAGIC;
use crate::demux::ds2::detect_ds2_format_type;

/// Detected audio format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// DSS file (.dss), SP codec, format 0 or variable rate format 1:
    /// synthesised at 12000 Hz, then decimated 11:12 to 11000 Hz output
    DssSp,
    /// DS2 file (.ds2) in SP or variable rate SP (block format 0 or 1). Its
    /// audio blocks are the same as a DSS file's, so it decodes like one, at
    /// 11000 Hz.
    Ds2Sp,
    /// DS2 file (.ds2), QP mode (mode byte 6), 16000 Hz
    Ds2Qp,
    /// DS2 file (.ds2), QP7 mode (mode byte 7), 16000 Hz
    Ds2Qp7,
    /// Grundig DSS file (first byte 6, magic "dss"), SP codec at 16000 Hz output
    GrundigSp,
    /// DSS file in LP, format 2: G.723.1 frames at 8000 Hz. Written by older
    /// recorders such as the DS4000.
    DssLp,
    /// DS2 file in LP, block format 2: the same G.723.1 blocks as DSS LP.
    Ds2Lp,
}

impl AudioFormat {
    pub fn native_sample_rate(&self) -> u32 {
        match self {
            AudioFormat::DssSp | AudioFormat::Ds2Sp => 11000,
            AudioFormat::Ds2Qp | AudioFormat::Ds2Qp7 => 16000,
            AudioFormat::GrundigSp => 16000,
            AudioFormat::DssLp | AudioFormat::Ds2Lp => 8000,
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            AudioFormat::DssSp | AudioFormat::DssLp | AudioFormat::GrundigSp => "dss",
            AudioFormat::Ds2Sp | AudioFormat::Ds2Lp | AudioFormat::Ds2Qp | AudioFormat::Ds2Qp7 => {
                "ds2"
            }
        }
    }

    /// Formats read by the DSS block walk: SP, variable rate SP and LP, in a
    /// DSS or a DS2 file.
    pub fn uses_dss_blocks(&self) -> bool {
        matches!(
            self,
            AudioFormat::DssSp | AudioFormat::Ds2Sp | AudioFormat::DssLp | AudioFormat::Ds2Lp
        )
    }
}

/// Result of demuxing a file
pub struct DemuxResult {
    pub format: AudioFormat,
    pub frame_data: FrameData,
    pub total_frames: usize,
}

/// Frame data varies by format
pub enum FrameData {
    /// List of fixed-size packets (DSS SP, DS2 SP)
    Packets(Vec<Vec<u8>>),
    /// Continuous bitstream (DS2 QP)
    Stream(Vec<u8>),
}

/// Detect format from file header bytes
pub fn detect_format(data: &[u8]) -> Option<AudioFormat> {
    if data.len() < 4 {
        return None;
    }
    if data[1..4] == *b"dss" && data[0] == 6 {
        return Some(AudioFormat::GrundigSp);
    }
    // A file whose block format changes decodes to one buffer at the highest
    // rate among its parts; its format is that of the first such part.
    if let Some(format) = parts::mixed_format(data) {
        return Some(format);
    }
    let ds2 = data[1..4] == *b"ds2" || data[..4] == ENCRYPTED_MAGIC;
    // SP, variable rate SP and LP, in a DSS file or a DS2 file: the same
    // audio blocks, read the same way.
    if let Some(layout) = dss::dss_layout(data) {
        let lp = layout.codec == dss::DssCodec::Lp;
        return Some(match (ds2, lp) {
            (false, false) => AudioFormat::DssSp,
            (false, true) => AudioFormat::DssLp,
            (true, false) => AudioFormat::Ds2Sp,
            (true, true) => AudioFormat::Ds2Lp,
        });
    }
    if data[..4] == ENCRYPTED_MAGIC && data.len() > 0x604 {
        return match data[0x600 + 4] {
            7 => Some(AudioFormat::Ds2Qp7),
            6 => Some(AudioFormat::Ds2Qp),
            _ => None,
        };
    }
    if data[1..4] == *b"ds2" && (1..=16).contains(&data[0]) && data.len() > 0x604 {
        // As the demuxer reads it: byte 0 is the header size in blocks.
        let header_size = dss::dss_header_size(data[0]);
        if data.len() <= header_size + 4 {
            return None;
        }
        return match detect_ds2_format_type(data, header_size) {
            7 => Some(AudioFormat::Ds2Qp7),
            6 => Some(AudioFormat::Ds2Qp),
            _ => None,
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ds2_like_file(magic: [u8; 4], mode: u8) -> Vec<u8> {
        let mut data = vec![0u8; 0x600 + 0x200];
        data[..4].copy_from_slice(&magic);
        data[0x600 + 4] = mode;
        data
    }

    #[test]
    fn detect_format_recognizes_encrypted_ds2_qp() {
        let data = make_ds2_like_file(*b"\x03enc", 6);
        assert_eq!(detect_format(&data), Some(AudioFormat::Ds2Qp));
    }

    #[test]
    fn detect_format_recognizes_encrypted_ds2_qp7() {
        let data = make_ds2_like_file(*b"\x03enc", 7);
        assert_eq!(detect_format(&data), Some(AudioFormat::Ds2Qp7));
    }

    #[test]
    fn detect_format_recognizes_encrypted_ds2_sp_and_lp() {
        for (mode, format) in [
            (0u8, AudioFormat::Ds2Sp),
            (1, AudioFormat::Ds2Sp),
            (2, AudioFormat::Ds2Lp),
        ] {
            let data = make_ds2_like_file(ENCRYPTED_MAGIC, mode);
            assert_eq!(detect_format(&data), Some(format));
        }
    }

    #[test]
    fn detect_format_reads_the_dss_codec_byte() {
        for (byte, format) in [
            (0u8, AudioFormat::DssSp),
            (1, AudioFormat::DssSp),
            (2, AudioFormat::DssLp),
        ] {
            let mut data = vec![0u8; 0x400 + 0x200];
            data[0] = 2;
            data[1..4].copy_from_slice(b"dss");
            data[0x2a4] = byte;
            assert_eq!(detect_format(&data), Some(format));
        }
    }
}
