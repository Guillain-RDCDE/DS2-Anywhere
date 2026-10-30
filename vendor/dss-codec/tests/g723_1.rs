//! G.723.1 decoder checked against FFmpeg's `g723_1` decoder, a second
//! reference next to the ITU test vectors (tests/g723_1_vectors.rs).
//!
//! Fixtures are packet streams (`u16` little-endian length + packet bytes,
//! repeated), exactly the packets FFmpeg's decoder receives:
//! - `olympus-lp.g723pk`: the 335 6.3 kbit/s frames of an Olympus DSS LP
//!   recording (ffprobe -show_packets -show_data). The decoder gives
//!   FFmpeg's output, postfilter on and off.
//! - `g723-mix.g723pk`: 900 synthetic packets mixing every frame type
//!   (6.3k, 5.3k, SID, untransmitted) and corrupted frames (forbidden pitch
//!   codes, bad gains), so frame erasure and comfort noise are covered.
//!   Here the ITU reference decides, not FFmpeg: FFmpeg leaves the
//!   Recommendation when an untransmitted frame follows speech with no SID
//!   frame in between (the "first SID lost" case of Annex A). 20 of the 21
//!   places where the two outputs part start at such a frame, and the 21st
//!   carries on from state left by an earlier one. FFmpeg matches the ITU
//!   vectors everywhere else, and this decoder matches all of them,
//!   including 283 first-SID-lost frames, so the expected values below are
//!   this decoder's output. FFmpeg's were ea82a7dc... (postfilter on) and
//!   c067118a... (off).

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
    let mut dec = G7231Decoder::with_postfilter(postfilter);
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
fn mixed_frame_types_follow_the_itu_reference() {
    assert_eq!(
        decode_sha(MIX, true),
        (
            216000,
            "622947cb5f1dd9594a830c11544c5b08e9c55f2a8658c5cf77f146868e2694c2".into()
        )
    );
    assert_eq!(
        decode_sha(MIX, false),
        (
            216000,
            "727f3c25c3988724bf7a2c952591c2a52a7e1220fe94573949977b5529a28825".into()
        )
    );
}

#[test]
fn a_new_decoder_restarts_identically() {
    let pk = packets(OLYMPUS);
    let decode = |n: usize| -> Vec<i16> {
        let mut dec = G7231Decoder::new();
        pk.iter()
            .take(n)
            .flat_map(|p| dec.decode_frame(p))
            .collect()
    };
    assert_eq!(decode(20), decode(20));
}
