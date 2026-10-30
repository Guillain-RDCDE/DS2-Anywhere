//! Files whose block format changes partway, decoded part by part.
//!
//! Olympus's DssRawWriter (ODMS R8) writes these; Patrick Domack made the
//! samples (his FATE set for the dssds2 series, release n9.0.2c): 3 s of
//! "Composed upon Westminster Bridge" read for LibriVox (public domain),
//! encoded in one mode then in another, and edits of those files. Before,
//! the whole file was read with the codec of its first block, and everything
//! after the change came out as noise.
//!
//! What each part is checked against:
//!
//! - QP and QP7 parts: Patrick's dss-codec (hirparak/dss-codec#28 with the
//!   QP codebook clamp) gives the same samples, part for part; for the QP7
//!   part of the QP7-then-LP files, which his crate refuses for their LP
//!   part, his crate on the QP7 blocks alone, and his FFmpeg n9.0.2c, whose
//!   16000 Hz output starts with the same samples;
//! - LP parts: his FFmpeg n9.0.2c, whose 8000 Hz output ends with the same
//!   samples, and this crate on the LP blocks alone;
//! - SP parts, decoded at 11000 Hz by this crate's own SP decoder: this
//!   crate on the SP blocks alone (the test below cuts them out).
//!
//! The same 3 s encodes are in the other fixtures, and the hashes show it:
//! the SP parts are `olympus-sp-empty-lp`'s audio, the QP7 parts
//! `olympus-qp7-empty-lp`'s. The expected values are the SHA-256 of the s16
//! samples.

use dss_codec::demux::AudioFormat::{self, *};
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn sha(samples: &[f64]) -> String {
    let mut h = Sha256::new();
    for s in samples {
        h.update((*s as i16).to_le_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// SP, 3 s, at 11000 Hz: also `olympus-sp-empty-lp`'s audio.
const SP: &str = "3d8aa19ac5c784407496224f8614fada4e66264efbf200cfc09b93de5ae513a5";
/// QP, 3 s.
const QP: &str = "fe11365e2e7c1ae47a4981deb550b20a82e17bdb1b2e114e0929443a98221dc9";
/// QP7, 3 s: also `olympus-qp7-empty-lp`'s audio.
const QP7: &str = "4059dc28e4751f8c1d4a940a6cccc6c2ffaa7cecb03fb08db3111068b21f59e6";
/// LP, 3 s.
const LP: &str = "bcf96f5d4c79887043ee85ada395a1a2414fbeb2546c4d546b66f88de69e4e17";

/// The parts of a file at their own rates, and the decoded buffer.
fn check(name: &str, parts: &[(AudioFormat, u32, usize, &str)], buffer: (AudioFormat, &str)) {
    let data = fixture(name);
    let got = dss_codec::decode_parts_with_password(&data, None).unwrap();
    assert_eq!(got.len(), parts.len(), "{name}");
    for (i, (part, &(format, rate, len, hash))) in got.iter().zip(parts).enumerate() {
        assert_eq!(part.format, format, "{name} part {i}");
        assert_eq!(part.native_rate, rate, "{name} part {i}");
        assert_eq!(part.samples.len(), len, "{name} part {i}");
        assert_eq!(sha(&part.samples), hash, "{name} part {i}");
    }

    // One buffer at the highest rate; a part at a lower rate is resampled to
    // it and keeps its duration.
    let buf = dss_codec::decode_to_buffer(&data).unwrap();
    let top = parts.iter().map(|p| p.1).max().unwrap();
    assert_eq!(buf.native_rate, top, "{name}");
    assert_eq!((buf.format, sha(&buf.samples).as_str()), buffer, "{name}");
    assert_eq!(buf.parts.len(), parts.len(), "{name}");
    let mut start = 0;
    for (i, (run, part)) in buf.parts.iter().zip(&got).enumerate() {
        assert_eq!(run.start, start, "{name} part {i}");
        assert_eq!(run.format, part.format, "{name} part {i}");
        assert_eq!(run.native_rate, part.native_rate, "{name} part {i}");
        let len = (part.samples.len() as f64 * top as f64 / part.native_rate as f64).round();
        assert_eq!(run.len, len as usize, "{name} part {i}");
        if part.native_rate == top {
            assert_eq!(
                buf.samples[run.start..run.start + run.len],
                part.samples[..],
                "{name} part {i}"
            );
        }
        start += run.len;
    }
    assert_eq!(buf.samples.len(), start, "{name}");

    // The file's format, as inspection reports it, is the buffer's.
    let info = dss_codec::inspect_bytes(&data).unwrap();
    assert_eq!(
        (info.format, info.native_rate()),
        (buf.format, top),
        "{name}"
    );
}

#[test]
fn sp_then_qp() {
    let parts = [(Ds2Sp, 11000, 33000, SP), (Ds2Qp, 16000, 48128, QP)];
    let buffer = (Ds2Qp, SP_QP);
    check("olympus-sp-qp.ds2", &parts, buffer);
    // A block of format 3 among the QP blocks, or in front of the SP ones,
    // names no codec: it belongs to the part around it.
    check("olympus-sp-qp-format3.ds2", &parts, buffer);
    check("olympus-sp-qp-format3-first.ds2", &parts, buffer);
}

#[test]
fn sp_then_qp_edited() {
    // SP blocks 7 to 11 removed.
    check(
        "olympus-sp-qp-cut.ds2",
        &[(Ds2Sp, 11000, 19536, SP_CUT), (Ds2Qp, 16000, 48128, QP)],
        (Ds2Qp, SP_QP_CUT),
    );
    // A re-sync block in the QP part: it drops the frame in flight.
    check(
        "olympus-sp-qp-resync.ds2",
        &[(Ds2Sp, 11000, 33000, SP), (Ds2Qp, 16000, 47616, QP_RESYNC)],
        (Ds2Qp, SP_QP_RESYNC),
    );
}

#[test]
fn qp_then_qp7() {
    let parts = [(Ds2Qp, 16000, 48128, QP), (Ds2Qp7, 16000, 48128, QP7)];
    check("olympus-qp-qp7.ds2", &parts, (Ds2Qp, QP_QP7));
    check("olympus-qp-qp7-format3.ds2", &parts, (Ds2Qp, QP_QP7));
}

#[test]
fn qp7_then_lp() {
    let parts = [(Ds2Qp7, 16000, 48128, QP7), (Ds2Lp, 8000, 24000, LP)];
    check("olympus-qp7-lp.ds2", &parts, (Ds2Qp7, QP7_LP));
    // 109 empty QP7 blocks in front.
    check("olympus-qp7-lp-empty109.ds2", &parts, (Ds2Qp7, QP7_LP));
}

const SP_CUT: &str = "a03b17553ebfd41c76711a54d5899eef877f462b36e356efe6f6bbed576c2740";
const QP_RESYNC: &str = "5a1360f29384d38eaafd104b537ead2561b8c3ed347c62bf872313838ff1731a";
const SP_QP: &str = "7fc81614b0db6e65bce8c9b74b7ebe79c0de296167d1d7449f3b9c807743648b";
const SP_QP_CUT: &str = "2a893daabadb558edf38b25fc53ced0006285c4ce153710e756ec5a710a95d5d";
const SP_QP_RESYNC: &str = "c45fba8788a918d9c0a89c6b06e21803c55384764736bac0c680233e702cfe1f";
const QP_QP7: &str = "b212800cc9197067becb9fd4a40933db89ce85125a5fcc7ea6048b4c7c0d4e36";
const QP7_LP: &str = "4e53796761e4debf9c1f732ddf34463a7b5e2129ecff8fd7c7828d7a9a357949";

/// The blocks of a file from `first` to `last` (audio block indices, `last`
/// excluded), behind the file's header: a file of its own.
fn cut(data: &[u8], first: usize, last: usize) -> Vec<u8> {
    let header = 3 * 512;
    let mut out = data[..header].to_vec();
    out.extend_from_slice(&data[header + first * 512..(header + last * 512).min(data.len())]);
    out
}

#[test]
fn each_part_decodes_as_a_file_of_its_own() {
    for (name, parts) in [
        ("olympus-sp-qp.ds2", [(0, 11), (11, 32)]),
        ("olympus-sp-qp-cut.ds2", [(0, 6), (6, 27)]),
        ("olympus-sp-qp-resync.ds2", [(0, 11), (11, 32)]),
        ("olympus-qp-qp7.ds2", [(0, 21), (21, 42)]),
        ("olympus-qp7-lp.ds2", [(0, 21), (21, 26)]),
        ("olympus-qp7-lp-empty109.ds2", [(0, 130), (130, 135)]),
    ] {
        let data = fixture(name);
        let got = dss_codec::decode_parts_with_password(&data, None).unwrap();
        for (i, &(first, last)) in parts.iter().enumerate() {
            let alone = dss_codec::decode_to_buffer(&cut(&data, first, last)).unwrap();
            assert_eq!(alone.parts.len(), 1, "{name} part {i}");
            assert_eq!(alone.native_rate, got[i].native_rate, "{name} part {i}");
            assert_eq!(alone.samples, got[i].samples, "{name} part {i}");
        }
    }
}

#[test]
fn streaming_gives_the_same_samples_and_format() {
    use dss_codec::streaming::DecryptingDecoderStreamer;
    for name in [
        "olympus-sp-qp.ds2",
        "olympus-qp-qp7.ds2",
        "olympus-qp7-lp.ds2",
    ] {
        let data = fixture(name);
        let whole = dss_codec::decode_to_buffer(&data).unwrap();
        for chunk in [100, 777, data.len()] {
            let mut dec = DecryptingDecoderStreamer::new(None);
            let mut streamed = Vec::new();
            for c in data.chunks(chunk) {
                streamed.extend(dec.push(c).unwrap());
            }
            streamed.extend(dec.finish().unwrap());
            assert_eq!(streamed, whole.samples, "{name}, chunk {chunk}");
            assert_eq!(dec.format(), Some(whole.format), "{name}, chunk {chunk}");
            assert_eq!(
                dec.native_rate(),
                Some(whole.native_rate),
                "{name}, chunk {chunk}"
            );
        }
    }
}

#[test]
fn a_file_in_one_format_is_one_part() {
    let data = fixture("olympus-sp.ds2");
    let buf = dss_codec::decode_to_buffer(&data).unwrap();
    assert_eq!(buf.parts.len(), 1);
    assert_eq!(
        (
            buf.parts[0].start,
            buf.parts[0].len,
            buf.parts[0].native_rate
        ),
        (0, buf.samples.len(), 11000)
    );
    let parts = dss_codec::decode_parts_with_password(&data, None).unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].samples, buf.samples);
}
