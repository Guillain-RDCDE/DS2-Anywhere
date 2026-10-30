pub mod bitstream;
pub mod codec;
pub mod crypto;
pub mod demux;
pub mod error;
pub mod output;
pub mod streaming;
pub mod tables;

use crate::crypto::ds2_encrypted::ENCRYPTED_MAGIC;
use crate::demux::{detect_format, AudioFormat};
use crate::error::{DecodeError, Result};
use crate::output::resample::resample;
use crate::output::wav::write_wav;
use crate::output::OutputConfig;
use crate::streaming::{DecryptStreamer, DecryptingDecoderStreamer};

use std::path::Path;

/// Decoded audio buffer
pub struct AudioBuffer {
    /// Samples as f64 (mono)
    pub samples: Vec<f64>,
    /// Native sample rate before any resampling. For a file whose block
    /// format changes, the highest native rate among its parts: the parts
    /// decoded at a lower rate are resampled to it.
    pub native_rate: u32,
    /// Detected format. For a file whose block format changes, the format of
    /// its first part at `native_rate`.
    pub format: AudioFormat,
    /// The runs of `samples` that one codec produced, in order. One part for
    /// a file in one format, none when it holds no audio.
    pub parts: Vec<AudioPart>,
}

/// A run of an [`AudioBuffer`]'s samples that one codec produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioPart {
    /// Index of the part's first sample in the buffer.
    pub start: usize,
    /// Number of samples of the part in the buffer.
    pub len: usize,
    /// Rate the part was decoded at; resampled to the buffer's rate when it
    /// differs.
    pub native_rate: u32,
    /// Format of the part's blocks.
    pub format: AudioFormat,
}

/// The samples of one part of a file, at the rate it was decoded at.
pub struct DecodedPart {
    pub samples: Vec<f64>,
    pub native_rate: u32,
    pub format: AudioFormat,
}

fn single_part(samples: &[f64], native_rate: u32, format: AudioFormat) -> Vec<AudioPart> {
    if samples.is_empty() {
        return Vec::new();
    }
    vec![AudioPart {
        start: 0,
        len: samples.len(),
        native_rate,
        format,
    }]
}

/// Lightweight file/container inspection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    /// Detected audio format.
    pub format: AudioFormat,
    /// Encryption metadata for the container.
    pub encryption: EncryptionInfo,
}

impl FileInfo {
    pub fn native_rate(&self) -> u32 {
        self.format.native_sample_rate()
    }
}

/// Encryption metadata detected from the file/container header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionInfo {
    None,
    EncryptedDs2Aes128,
    EncryptedDs2Aes256,
    EncryptedUnknown(u16),
}

/// Normalize a DSS/DS2 file to plain container bytes.
///
/// Plain DSS/DS2 input is returned unchanged.
/// Encrypted DS2 input is decrypted to plain DS2
pub fn decrypt_file(path: &Path, password: Option<&[u8]>) -> Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    decrypt_to_bytes(&data, password)
}

/// Normalize raw DSS/DS2 bytes to plain container bytes.
///
/// Plain DSS/DS2 input is returned unchanged.
/// Encrypted DS2 input is decrypted to plain DS2
pub fn decrypt_to_bytes(data: &[u8], password: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut decryptor = DecryptStreamer::new(password);
    let mut plain = decryptor.push(data)?;
    plain.extend(decryptor.finish()?);
    Ok(plain)
}

/// Inspect a DSS/DS2 file and report format/encryption metadata.
pub fn inspect_file(path: &Path) -> Result<FileInfo> {
    let data = std::fs::read(path)?;
    inspect_bytes(&data)
}

/// Inspect raw DSS/DS2 bytes and report format/encryption metadata.
pub fn inspect_bytes(data: &[u8]) -> Result<FileInfo> {
    let format = detect_format(data)
        .ok_or_else(|| DecodeError::UnsupportedFormat(data.first().copied().unwrap_or(0)))?;

    let encryption = if data.starts_with(&ENCRYPTED_MAGIC) {
        let mode_offset = crate::crypto::ds2_encrypted::DECRYPT_DESCRIPTOR_OFFSET;
        let mode = data
            .get(mode_offset..mode_offset + 2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .ok_or_else(|| DecodeError::EncryptedDs2("missing decrypt descriptor".to_string()))?;

        match mode {
            1 => EncryptionInfo::EncryptedDs2Aes128,
            2 => EncryptionInfo::EncryptedDs2Aes256,
            other => EncryptionInfo::EncryptedUnknown(other),
        }
    } else {
        EncryptionInfo::None
    };

    Ok(FileInfo { format, encryption })
}

/// Decode a DSS/DS2 file to an AudioBuffer.
pub fn decode_file(path: &Path) -> Result<AudioBuffer> {
    decode_file_with_password(path, None)
}

/// Decode a DSS/DS2 file to an AudioBuffer, optionally decrypting encrypted DS2 input first.
pub fn decode_file_with_password(path: &Path, password: Option<&[u8]>) -> Result<AudioBuffer> {
    let data = std::fs::read(path)?;
    decode_to_buffer_with_password(&data, password)
}

/// Decode raw file bytes to an AudioBuffer.
pub fn decode_to_buffer(data: &[u8]) -> Result<AudioBuffer> {
    decode_to_buffer_with_password(data, None)
}

/// Decode raw file bytes to an AudioBuffer, optionally decrypting encrypted DS2 input first.
pub fn decode_to_buffer_with_password(data: &[u8], password: Option<&[u8]>) -> Result<AudioBuffer> {
    // A file whose block format changes: each part with its own codec.
    if let Some(parts) = decode_mixed_parts(data, password)? {
        return assemble_parts(parts, None, mixed_format_of(data));
    }

    // SP, variable rate SP and LP, in DSS or DS2 files, go through the DSS
    // block walk. The block headers of an encrypted DS2 file are in the
    // clear, so the format is known before decrypting.
    if let Some(format) = detect_format(data).filter(AudioFormat::uses_dss_blocks) {
        let decrypted;
        let plain = if data.starts_with(&ENCRYPTED_MAGIC) {
            decrypted = decrypt_to_bytes(data, password)?;
            &decrypted[..]
        } else {
            data
        };
        let samples = decode_dss_blocks(plain)?;
        let parts = single_part(&samples, format.native_sample_rate(), format);
        return Ok(AudioBuffer {
            samples,
            native_rate: format.native_sample_rate(),
            format,
            parts,
        });
    }

    let mut decoder = DecryptingDecoderStreamer::new(password);
    let mut samples = decoder.push(data)?;
    samples.extend(decoder.finish_lenient()?);

    let format = decoder
        .format()
        .or_else(|| detect_format(data))
        .ok_or_else(|| DecodeError::UnsupportedFormat(data.first().copied().unwrap_or(0)))?;

    let parts = single_part(&samples, format.native_sample_rate(), format);
    Ok(AudioBuffer {
        samples,
        native_rate: format.native_sample_rate(),
        format,
        parts,
    })
}

/// Decode raw file bytes to its parts, each at the rate it was decoded at:
/// one part for a file in one format (none when it holds no audio), one per
/// run of blocks of one codec for a file whose block format changes.
pub fn decode_parts_with_password(
    data: &[u8],
    password: Option<&[u8]>,
) -> Result<Vec<DecodedPart>> {
    if let Some(parts) = decode_mixed_parts(data, password)? {
        return Ok(parts);
    }
    let buf = decode_to_buffer_with_password(data, password)?;
    if buf.samples.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![DecodedPart {
        samples: buf.samples,
        native_rate: buf.native_rate,
        format: buf.format,
    }])
}

/// The parts of a file whose block format changes, each decoded with its own
/// codec at its own rate, or `None` for a file in one format. Parts that give
/// no samples are left out.
pub(crate) fn decode_mixed_parts(
    data: &[u8],
    password: Option<&[u8]>,
) -> Result<Option<Vec<DecodedPart>>> {
    // The block headers of an encrypted DS2 file are in the clear.
    if crate::demux::parts::mixed_spans(data).is_none() {
        return Ok(None);
    }
    let decrypted;
    let plain = if data.starts_with(&ENCRYPTED_MAGIC) {
        decrypted = decrypt_to_bytes(data, password)?;
        &decrypted[..]
    } else {
        data
    };
    decode_mixed_plain(plain)
}

/// Format of a mixed file, from its block headers (in the clear in an
/// encrypted DS2 file).
pub(crate) fn mixed_format_of(data: &[u8]) -> AudioFormat {
    crate::demux::parts::mixed_format(data).unwrap_or(AudioFormat::Ds2Sp)
}

/// As [`decode_mixed_parts`], on plain container bytes.
pub(crate) fn decode_mixed_plain(data: &[u8]) -> Result<Option<Vec<DecodedPart>>> {
    use crate::codec::ds2_qp::Ds2QpDecoder;
    use crate::demux::ds2::{demux_qp_frames, Ds2Qp7Segment, Ds2QpSegment};
    use crate::demux::dss::{demux_dss_packets, DssCodec, DssLayout};
    use crate::demux::parts::{mixed_spans, PartCodec};

    let Some((ds2, spans)) = mixed_spans(data) else {
        return Ok(None);
    };
    let mut parts = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        // The part alone: its blocks, read as a file whose audio starts at
        // the part's first block and ends with its last.
        let data = &data[..span.end];
        let samples = match span.codec {
            PartCodec::Sp | PartCodec::SpVariableRate | PartCodec::Lp => {
                let codec = match span.codec {
                    PartCodec::Sp => DssCodec::Sp,
                    PartCodec::SpVariableRate => DssCodec::SpVariableRate,
                    _ => DssCodec::Lp,
                };
                let layout = DssLayout {
                    header_size: span.start,
                    codec,
                };
                decode_dss_packets(codec, &demux_dss_packets(data, layout))
            }
            PartCodec::Qp | PartCodec::Qp7 => {
                let qp7 = span.codec == PartCodec::Qp7;
                let (mut packets, truncated) = demux_qp_frames(data, span.start, qp7);
                // A frame that runs on into the blocks of the next part has
                // no end: drop it, as at a re-sync block. At the end of the
                // file a cut frame is kept, zero-filled, as Olympus does.
                if truncated && i + 1 < spans.len() {
                    packets.pop();
                }
                let mut decoder = Ds2QpDecoder::new();
                let total_frames = packets.len();
                if packets.is_empty() {
                    Vec::new()
                } else if qp7 {
                    decoder.decode_qp7_segments(&[Ds2Qp7Segment {
                        records: packets,
                        total_frames,
                        reset_before: false,
                    }])
                } else {
                    decoder.decode_qp_segments(&[Ds2QpSegment {
                        stream: packets.concat(),
                        total_frames,
                        reset_before: false,
                    }])
                }
            }
        };
        if !samples.is_empty() {
            parts.push(DecodedPart {
                samples,
                native_rate: span.codec.native_rate(),
                format: span.codec.format(ds2),
            });
        }
    }
    Ok(Some(parts))
}

/// One buffer from the parts of a mixed file: at `rate`, or by default at
/// the highest rate among the parts; a part at another rate is resampled to
/// it. `format` is the file's (`mixed_format`), for a file that gives no
/// samples at all.
pub(crate) fn assemble_parts(
    parts: Vec<DecodedPart>,
    rate: Option<u32>,
    format: AudioFormat,
) -> Result<AudioBuffer> {
    let top = parts.iter().map(|p| p.native_rate).max();
    let rate = rate.or(top).unwrap_or(format.native_sample_rate());
    // The format of the first part decoded at the buffer's rate, else the
    // first part's.
    let format = parts
        .iter()
        .find(|p| p.native_rate == rate)
        .or(parts.first())
        .map_or(format, |p| p.format);
    let mut samples = Vec::new();
    let mut runs = Vec::with_capacity(parts.len());
    for part in parts {
        let resampled;
        let part_samples = if part.native_rate == rate {
            &part.samples
        } else {
            // The codecs give whole numbers. The resampler's FFT runs other
            // code on other machines (SIMD on x86, scalar in WebAssembly),
            // which leaves its output about 1e-11 apart from one to the
            // other: rounding it to whole numbers puts the part on the same
            // grid as the others, and the native and web decoders agree.
            resampled = resample(&part.samples, part.native_rate, rate)?
                .into_iter()
                .map(f64::round)
                .collect::<Vec<_>>();
            &resampled
        };
        runs.push(AudioPart {
            start: samples.len(),
            len: part_samples.len(),
            native_rate: part.native_rate,
            format: part.format,
        });
        samples.extend_from_slice(part_samples);
    }
    Ok(AudioBuffer {
        samples,
        native_rate: rate,
        format,
        parts: runs,
    })
}

/// Decode the plain bytes of a file read by the DSS block walk: SP and
/// variable rate SP at 11000 Hz, LP at 8000 Hz.
pub(crate) fn decode_dss_blocks(data: &[u8]) -> Result<Vec<f64>> {
    use crate::demux::dss::{demux_dss_packets, dss_layout};

    let layout =
        dss_layout(data).ok_or_else(|| DecodeError::NotDss(std::path::PathBuf::from("<bytes>")))?;
    let packets = demux_dss_packets(data, layout);
    Ok(decode_dss_packets(layout.codec, &packets))
}

/// Decode the packets of the DSS block walk with the codec of their blocks.
fn decode_dss_packets(codec: crate::demux::dss::DssCodec, packets: &[Vec<u8>]) -> Vec<f64> {
    use crate::codec::dss_sp::DssSpDecoder;
    use crate::codec::g723_1::G7231Decoder;
    use crate::demux::dss::DssCodec;

    let mut samples = Vec::new();
    match codec {
        DssCodec::Sp | DssCodec::SpVariableRate => {
            let mut decoder = if codec == DssCodec::Sp {
                DssSpDecoder::new()
            } else {
                DssSpDecoder::new_variable_rate()
            };
            for pkt in packets {
                samples.extend(decoder.decode_frame(pkt).into_iter().map(f64::from));
            }
        }
        DssCodec::Lp => {
            let mut decoder = G7231Decoder::new();
            for pkt in packets {
                samples.extend(decoder.decode_frame(pkt).into_iter().map(f64::from));
            }
        }
    }
    samples
}

/// Decode a file and write to WAV with given output configuration.
pub fn decode_and_write(input: &Path, output: &Path, config: &OutputConfig) -> Result<AudioBuffer> {
    decode_and_write_with_password(input, output, config, None)
}

pub fn decode_and_write_with_password(
    input: &Path,
    output: &Path,
    config: &OutputConfig,
    password: Option<&[u8]>,
) -> Result<AudioBuffer> {
    let data = std::fs::read(input)?;
    // A file whose block format changes: each part goes to the output rate
    // straight from its own.
    if let Some(parts) = decode_mixed_parts(&data, password)? {
        // As for a file in one format, `native_rate` and `format` say what
        // the file decodes to, `samples` what was written.
        let decoded = parts.iter().map(|p| p.native_rate).max();
        let mut buf = assemble_parts(parts, config.sample_rate, mixed_format_of(&data))?;
        write_wav(
            output,
            &buf.samples,
            buf.native_rate,
            config.bit_depth,
            config.channels,
        )?;
        if let Some(rate) = decoded.filter(|&rate| rate != buf.native_rate) {
            buf.native_rate = rate;
            if let Some(part) = buf.parts.iter().find(|p| p.native_rate == rate) {
                buf.format = part.format;
            }
        }
        return Ok(buf);
    }

    let mut buf = decode_to_buffer_with_password(&data, password)?;

    let target_rate = config.sample_rate.unwrap_or(buf.native_rate);

    if target_rate != buf.native_rate {
        buf.samples = resample(&buf.samples, buf.native_rate, target_rate)?;
    }

    write_wav(
        output,
        &buf.samples,
        target_rate,
        config.bit_depth,
        config.channels,
    )?;

    Ok(buf)
}

#[cfg(test)]
mod inspection_tests {
    use super::*;
    use crate::crypto::ds2_encrypted::DECRYPT_DESCRIPTOR_OFFSET;

    fn encrypted_ds2(mode: u16, format_type: u8) -> Vec<u8> {
        let mut data = vec![0u8; 0x800];
        data[..4].copy_from_slice(&ENCRYPTED_MAGIC);
        data[DECRYPT_DESCRIPTOR_OFFSET..DECRYPT_DESCRIPTOR_OFFSET + 2]
            .copy_from_slice(&mode.to_le_bytes());
        data[0x604] = format_type;
        data
    }

    #[test]
    fn inspect_bytes_reports_encryption_and_format() {
        assert_eq!(
            inspect_bytes(&encrypted_ds2(1, 6)).unwrap(),
            FileInfo {
                format: AudioFormat::Ds2Qp,
                encryption: EncryptionInfo::EncryptedDs2Aes128,
            }
        );
        assert_eq!(
            inspect_bytes(&encrypted_ds2(2, 7)).unwrap(),
            FileInfo {
                format: AudioFormat::Ds2Qp7,
                encryption: EncryptionInfo::EncryptedDs2Aes256,
            }
        );
        assert_eq!(
            inspect_bytes(&encrypted_ds2(99, 0)).unwrap().encryption,
            EncryptionInfo::EncryptedUnknown(99)
        );
    }
}
