//! DS2 QP / QP7 regression against the reference decode (Patrick Domack's
//! FFmpeg fork, itself within 1 LSB of Olympus's R8 DssDecoder.dll).
//!
//! Fixtures: paused-qp.ds2 (CC0, a QP file with pause blocks and a last frame
//! cut by the end of the file) and olympus-qp7.ds2 (LibriVox public-domain
//! reading encoded by the Olympus encoder in mode 7).

use std::path::Path;

/// FNV-1a 64 over the s16 little-endian PCM, truncated toward zero as written
/// to a 16-bit WAV.
fn pcm_fnv1a(samples: &[f64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &s in samples {
        let v = s.clamp(-32768.0, 32767.0) as i16;
        for b in v.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn decode_fixture(name: &str) -> dss_codec::AudioBuffer {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    dss_codec::decode_file(&path).unwrap()
}

#[test]
fn paused_qp_matches_reference() {
    let buf = decode_fixture("paused-qp.ds2");
    assert_eq!(buf.native_rate, 16000);
    // Olympus R8 and the fork: pause-block frames played, padding skipped,
    // the frame the file ends inside kept zero-filled. Block 28 (0f 04 13 ff
    // 06 ff) claims 19 frames, more than a QP block can start: it counts as
    // empty, as Philips reads it, and its 9 frames are gone (92416 samples
    // before). 323 of the subframes left carry a fixed-codebook index of
    // C(64,11) or more, clamped as R8 and Philips's codec do; the s16 output's
    // md5 is 294bb8fc3bb31ff141bc2aba74c15bac, the same as Patrick Domack's
    // dss-codec (hirparak/dss-codec#28, commit d3d58c3).
    assert_eq!(buf.samples.len(), 90112);
    assert_eq!(pcm_fnv1a(&buf.samples), 0x326d_9d81_fae1_36cc);
}

#[test]
fn olympus_qp7_matches_reference() {
    let buf = decode_fixture("olympus-qp7.ds2");
    assert_eq!(buf.native_rate, 16000);
    assert_eq!(buf.samples.len(), 160000);
    assert_eq!(pcm_fnv1a(&buf.samples), 0x050a_5d75_cfb0_4ab1);
}

#[test]
fn qp_files_cut_anywhere_never_panic() {
    for name in ["paused-qp.ds2", "olympus-qp7.ds2"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let data = std::fs::read(path).unwrap();
        for cut in (0..data.len()).step_by(1499) {
            if let Ok(buf) = dss_codec::decode_to_buffer(&data[..cut]) {
                assert_eq!(buf.samples.len() % 256, 0);
            }
        }
    }
}
