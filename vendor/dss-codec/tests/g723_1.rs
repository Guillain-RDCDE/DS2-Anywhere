//! G.723.1 decoder pinned against FFmpeg's `g723_1` decoder.
//!
//! Fixtures are packet streams (`u16` little-endian length + packet bytes,
//! repeated), exactly the packets FFmpeg's decoder receives:
//! - `olympus-lp.g723pk`: the 335 6.3 kbit/s frames of an Olympus DSS LP
//!   recording (ffprobe -show_packets -show_data).
//! - `g723-mix.g723pk`: 900 synthetic packets mixing every frame type
//!   (6.3k, 5.3k, SID, untransmitted) and corrupted frames (forbidden pitch
//!   codes, bad gains), so frame erasure and comfort noise are covered.
//!
//! The expected SHA-256 values are of FFmpeg's s16le output for the same
//! packets (postfilter on, its default, and off).

use dss_codec::codec::g723_1::G7231Decoder;
use sha2::{Digest, Sha256};

fn packets(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 <= data.len() {
        let len = u16::from_le_bytes([data[i], data[i + 1]]) as usize;
        i += 2;
        out.push(&data[i..i + len]);
        i += len;
    }
    out
}

fn decode_sha(data: &[u8], postfilter: bool) -> (usize, String) {
    let mut dec = G7231Decoder::new();
    dec.set_postfilter(postfilter);
    let mut h = Sha256::new();
    let mut n = 0;
    for pkt in packets(data) {
        let pcm = dec.decode_frame(pkt);
        n += pcm.len();
        for s in pcm {
            h.update(s.to_le_bytes());
        }
    }
    let hex = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    (n, hex)
}

const OLYMPUS: &[u8] = include_bytes!("fixtures/olympus-lp.g723pk");
const MIX: &[u8] = include_bytes!("fixtures/g723-mix.g723pk");

#[test]
fn olympus_lp_matches_ffmpeg() {
    assert_eq!(
        decode_sha(OLYMPUS, true),
        (
            80400,
            "e57be2494829ad72f691d9debb1e7b82e28a2689561ebde7ff9a4e7a88f2d57b".into()
        )
    );
}

#[test]
fn olympus_lp_no_postfilter_matches_ffmpeg() {
    assert_eq!(
        decode_sha(OLYMPUS, false),
        (
            80400,
            "b9ed97346f7d2c999ea4742a3fce868775c6af165784c6b8b993e01b75dcf23e".into()
        )
    );
}

#[test]
fn mixed_frame_types_match_ffmpeg() {
    assert_eq!(
        decode_sha(MIX, true),
        (
            216000,
            "ea82a7dc93fae6019f886c42be36a19603cfa8235f9d513fdd8a58b4d2868cbf".into()
        )
    );
    assert_eq!(
        decode_sha(MIX, false),
        (
            216000,
            "c067118a52d472bd1099fa2412cbe87067df24897f64f892f4d42b7a985ffae8".into()
        )
    );
}

#[test]
fn reset_restarts_identically() {
    let pk = packets(OLYMPUS);
    let mut dec = G7231Decoder::new();
    let first: Vec<i16> = pk
        .iter()
        .take(20)
        .flat_map(|p| dec.decode_frame(p))
        .collect();
    dec.reset();
    let again: Vec<i16> = pk
        .iter()
        .take(20)
        .flat_map(|p| dec.decode_frame(p))
        .collect();
    assert_eq!(first, again);
}
