//! DSS and DS2 files decoded exactly as Patrick Domack's FFmpeg fork decodes
//! them (github.com/patrickdk77/FFmpeg, release n9.0.2b), sample for sample.
//!
//! The Olympus files are his FATE samples: ten seconds of "Composed upon
//! Westminster Bridge" read for LibriVox (public domain), encoded with
//! Olympus's ODMS R8 encoder in each mode. A .dss and a .ds2 file of the same
//! mode carry the same audio blocks. `../test_files/sample.DSS` is this
//! project's paused DSS SP excerpt, `sp-paused.dss` in his set. The expected
//! values are the SHA-256 of the fork's s16le output.

use sha2::{Digest, Sha256};
use std::path::Path;

fn check(path: &str, format: dss_codec::demux::AudioFormat, rate: u32, samples: usize, sha: &str) {
    let buf = dss_codec::decode_file(Path::new(path)).unwrap();
    assert_eq!(buf.format, format, "{path}");
    assert_eq!(buf.native_rate, rate, "{path}");
    assert_eq!(buf.samples.len(), samples, "{path}");
    let mut h = Sha256::new();
    for s in &buf.samples {
        h.update((*s as i16).to_le_bytes());
    }
    let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(got, sha, "{path}");
}

use dss_codec::demux::AudioFormat::*;

const SP: &str = "6665136c30ada9ec481dc99f5709824abe8f6755139400fc9035ae1b232981fd";
const FORMAT1: &str = "72636bbf03817f69b3cffa46918927ecaa342ea38478c1e74bc5c7ba1898d94f";
const LP: &str = "e57be2494829ad72f691d9debb1e7b82e28a2689561ebde7ff9a4e7a88f2d57b";

#[test]
fn ds2_sp_decodes_like_dss() {
    // Before, DS2 SP went down a path of its own and came out at 12000 Hz.
    check("tests/fixtures/olympus-sp.ds2", Ds2Sp, 11000, 110088, SP);
}

#[test]
fn variable_rate_sp_format_1() {
    check(
        "tests/fixtures/olympus-format1.dss",
        DssSp,
        11000,
        110088,
        FORMAT1,
    );
    check(
        "tests/fixtures/olympus-format1.ds2",
        Ds2Sp,
        11000,
        110088,
        FORMAT1,
    );
}

#[test]
fn lp_is_g723_1() {
    check("tests/fixtures/olympus-lp.dss", DssLp, 8000, 80400, LP);
    check("tests/fixtures/olympus-lp.ds2", Ds2Lp, 8000, 80400, LP);
}

#[test]
fn paused_dss_sp_plays_the_pause_block() {
    // The frames of a pause block, stated in byte 5 of its header, are played.
    check(
        "../test_files/sample.DSS",
        DssSp,
        11000,
        173448,
        "52fa1bbe5a4f288ca104ae639b007711d822bcb253d131a709f53d1252ff2d8d",
    );
}

#[test]
fn continuous_qp() {
    check(
        "../test_files/sample_qp.DS2",
        Ds2Qp,
        16000,
        591360,
        "21b23b6eca92adb3d98cad18e198ea3cdeb465038a5a4cbbd883efeb24e2d894",
    );
}

#[test]
fn streaming_gives_the_same_samples() {
    use dss_codec::streaming::DecryptingDecoderStreamer;
    for path in [
        "tests/fixtures/olympus-format1.ds2",
        "tests/fixtures/olympus-lp.dss",
        "../test_files/sample.DSS",
    ] {
        let data = std::fs::read(path).unwrap();
        let whole = dss_codec::decode_to_buffer(&data).unwrap().samples;
        let mut dec = DecryptingDecoderStreamer::new(None);
        let mut streamed = Vec::new();
        for chunk in data.chunks(777) {
            streamed.extend(dec.push(chunk).unwrap());
        }
        streamed.extend(dec.finish().unwrap());
        assert_eq!(streamed, whole, "{path}");
    }
}
