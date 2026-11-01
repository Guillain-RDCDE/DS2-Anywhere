//! Two block headers that Olympus's own encodes never hold, from Patrick
//! Domack's hirparak/dss-codec#28 (commits 8ae78ea and d3d58c3), with his
//! fixtures:
//!
//! - Philips's pause blocks (DPM 6000, 8000 and 9600, DSS-Lib): byte 2 is 0,
//!   the frame count is in byte 5, and the first frame's bit offset is in the
//!   bottom half of byte 0 and in byte 3, where bytes 0 and 1 leave it at 0;
//! - blocks that claim more than 13 frames, as the Olympus DS-2600 writes at
//!   its splices (0f 04 13 ff 06 ff), whose frames are random bits: an SP or
//!   QP block holding them counts as empty, as Philips's decoder takes it.
//!
//! The philips-pause files and olympus-qp-count19.ds2 are the first 700
//! frames of an Olympus R8 encode of test01_20s.wav (voxserv
//! audio_quality_testing_samples), with three block headers set to a pause
//! block and its neighbours from real recordings, or to a block claiming
//! 19 frames; the other files are edits of those (see NOTICE).
//!
//! The QP files give the same samples as Patrick's dss-codec: the md5 of the
//! s16 PCM, in each test, is the one his tests expect. The SP files, decoded
//! at 11000 Hz here and at 12000 Hz by his crate, give the same number of
//! frames as his. The expected values are the SHA-256 of the s16 samples.

use dss_codec::demux::AudioFormat;
use sha2::{Digest, Sha256};

/// Samples of a QP frame at 16000 Hz and of an SP frame at 11000 Hz.
const QP_FRAME: usize = 256;
const SP_FRAME: usize = 264;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn sha(samples: &[f64]) -> String {
    let mut h = Sha256::new();
    for s in samples {
        h.update((s.clamp(-32768.0, 32767.0) as i16).to_le_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn check(data: &[u8], format: AudioFormat, rate: u32, samples: usize, hash: &str) {
    let buf = dss_codec::decode_to_buffer(data).unwrap();
    assert_eq!(buf.format, format);
    assert_eq!(buf.native_rate, rate);
    assert_eq!(buf.samples.len(), samples);
    assert_eq!(sha(&buf.samples), hash);
}

/// The file with the header of audio block `block` replaced.
fn with_header(mut data: Vec<u8>, block: usize, header: [u8; 6]) -> Vec<u8> {
    let header_size = 512 * if data[0] == 1 { 3 } else { data[0] as usize };
    let at = header_size + 512 * block;
    data[at..at + 6].copy_from_slice(&header);
    data
}

fn decoded(data: &[u8]) -> Vec<f64> {
    dss_codec::decode_to_buffer(data).unwrap().samples
}

/// QP, blocks 39 to 41: 0f 14 09 ff 06 ff, 01 00 00 c0 06 09, 0f 1b 09 ff 06 ff.
/// The pause block's frames start at bit 0x1c0, payload byte 50; 699 of the
/// 700 frames: the one running into the pause block is dropped because its
/// tail does not reach the pointer, as Olympus's R8 decoder does.
/// md5 b4fe8e94e760538834711caf75e646ea.
const QP_PAUSE: &str = "a316cc48e707da6a408e1299b5f8d8e33ac1c026ba08446a6af35c290d8fbad5";

#[test]
fn qp_philips_pause_block() {
    let data = fixture("olympus-qp-philips-pause.ds2");
    check(&data, AudioFormat::Ds2Qp, 16000, 699 * QP_FRAME, QP_PAUSE);
    // The same pointer in Olympus's form, in byte 1.
    let olympus = with_header(data.clone(), 40, [0x0f, 0x1c, 0x09, 0xff, 0x06, 0xff]);
    assert_eq!(decoded(&olympus), decoded(&data));
}

/// SP, blocks 15 to 17: 0f 15 0c ff 00 ff, 01 00 00 78 00 0c, 8f 10 0c ff 00 ff.
/// The pause block's first frame is swapped, at bit 0x178. All 700 frames.
const SP_PAUSE: &str = "58ffa42aad0792bab1ae1714ff879a7d4e2bad71a7db11db4408e0b8f88c5521";

#[test]
fn sp_philips_pause_block() {
    let data = fixture("olympus-sp-philips-pause.dss");
    check(&data, AudioFormat::DssSp, 11000, 700 * SP_FRAME, SP_PAUSE);
    let olympus = with_header(data.clone(), 16, [0x8f, 0x17, 0x0c, 0xff, 0x00, 0xff]);
    assert_eq!(decoded(&olympus), decoded(&data));
}

/// Block 31 is 0f 15 ff ff 00 0c: 0xff in byte 2, the count in byte 5.
#[test]
fn sp_byte2_ff_takes_the_count_from_byte5() {
    let data = fixture("olympus-sp-byte2-ff.dss");
    check(&data, AudioFormat::DssSp, 11000, 700 * SP_FRAME, SP_PAUSE);
}

/// QP, blocks 41 to 43: 0f 12 09 ff 06 ff, 0f 11 13 ff 06 ff, 0f 1b 09 ff 06 ff,
/// as at a DS-2600 splice. The middle block claims 19 frames and holds 9:
/// they are gone, 691 frames. md5 759432a0076049d6de39b96df4bf5cf9.
const QP_COUNT19: &str = "221bf09caf2a4737dbae6d73d24fd0f0a692a776eeb061345e39fa132de42b6b";

#[test]
fn qp_block_claiming_19_frames_is_empty() {
    let data = fixture("olympus-qp-count19.ds2");
    check(&data, AudioFormat::Ds2Qp, 16000, 691 * QP_FRAME, QP_COUNT19);
}

/// The same file with 0 in byte 3 and 7 in byte 4 of block 42: its format
/// does not count, so the limit applies all the same.
#[test]
fn qp_block_claiming_19_frames_without_a_format_is_empty() {
    let data = fixture("olympus-qp-count19-byte3.ds2");
    check(&data, AudioFormat::Ds2Qp, 16000, 691 * QP_FRAME, QP_COUNT19);
}

/// olympus-sp-philips-pause.dss with block 31 set to 0f 15 13 ff 00 ff: 19
/// frames claimed, 12 held, none played. 688 frames.
#[test]
fn sp_block_claiming_19_frames_is_empty() {
    let data = fixture("olympus-sp-count19.dss");
    check(
        &data,
        AudioFormat::DssSp,
        11000,
        688 * SP_FRAME,
        "397b78b20824b8e6a541f5c3a3626d8a732f0c55cd6309b600db21f0be9a09e6",
    );
}
