# 19 — The last format

*Or: how LP got a decoder that belongs to nobody but its licence, and what that turned up on the way.*

---

[Chapter 18](18-scored-against-the-source.md) left every DSS and DS2 mode decoding, LP included. LP decoded through a port of FFmpeg's G.723.1 decoder, and that port carries FFmpeg's licence, the LGPL. Here that was fine, because [NOTICE](../NOTICE) says so. It was not fine for the crate this repository vendors: [Kieran Hirpara's `dss-codec`](https://github.com/hirparak/dss-codec) is MIT, and an LGPL file cannot go into it. So the one decoder every other project builds on still refused LP, and so did Patrick Domack's port of his fork to it ([dss-codec#28](https://github.com/hirparak/dss-codec/pull/28)): "It refuses LP parts, since it has no LP decoder."

LP is the format of older Olympus recorders, the DS-4000 generation. Its frames are not a vendor codec at all: they are **ITU-T G.723.1**, a published standard. Nothing about it is secret. What was missing was a decoder anyone could take under any licence.

## Why not just port something

There are two G.723.1 decoders worth the name. FFmpeg's is LGPL. The ITU's own reference, the fixed-point C code that comes with the Recommendation, is given "only for purpose of establishing the specification of a coder": you may read it to understand the standard, not lift it into your product. Translating either into Rust would carry its terms with it.

The way out is old and well understood: a **clean room**. One side reads everything, the Recommendation and the reference code, and writes a description of what the decoder must do, in prose and formulas, with no code in it. The other side never sees the reference or FFmpeg, and writes the decoder from that description and the Recommendation alone. What crosses the wall is a specification, and specifications are what standards are for.

The patents are not in the way either. Most G.723.1 patents have expired, and since 1 January 2017 the remaining licensors have offered theirs royalty-free.

## The specification

The description is published here: [`docs/g723-1/SPEC.md`](g723-1/SPEC.md), with its tables as plain CSV in [`docs/g723-1/tables/`](g723-1/tables/). It covers both rates (6.3 kbit/s MP-MLQ and 5.3 kbit/s ACELP), Annex A (SID frames, untransmitted frames, comfort noise), frame erasure, the postfilter, and every 16- and 32-bit operation with its exact rounding and saturation. Bit-exactness lives in those details. A few that the prose of the Recommendation gets wrong, or does not say, and that the reference decides:

- the comfort-noise excitation draws its second lag from 123 to 141, not 123 to 143, and its gain indices run 1 to 50, not 0 to 49;
- a 32 × 16 multiply must split the 32-bit value into an unsigned low half and a signed high half; a plain wide product shifted by 15 gives other results;
- pulse signs are opposite between the two rates;
- at 6.3 kbit/s the gain table and the pulse train follow the lag of the even subframe, not the one just decoded.

Section 7 of the specification lists thirty such traps. The specification is the reason the work can be redone and checked by anyone: it is the part of this chapter we would most like to see reused.

## The decoder, and what it was checked against

The decoder is [`vendor/dss-codec/src/codec/g723_1.rs`](../vendor/dss-codec/src/codec/g723_1.rs), MIT, written from the specification and the Recommendation by a side that never saw the reference source or any other G.723.1 implementation. Then it was checked, sample for sample, with no tolerance:

| Reference | Cases | Result |
|---|---|---|
| ITU decoder test vectors (main body and Annex A) | 11 | **identical** |
| Extra cases decoded by the ITU reference: the 7 encoder bitstreams with the postfilter on and off, bursts of erasures, 3,000 frames of random bits | 17 | **identical** |
| Patrick's LP samples, decoded by his FFmpeg fork n9.0.2c | 6 LP-only files | **identical** |
| The LP part of his mixed-format samples, against FFmpeg's raw G.723.1 decoder | 2 files | **identical** |

The ITU vectors are not in this repository, since their licence is unclear. They come free with the Recommendation, and [`tests/g723_1_vectors.rs`](../vendor/dss-codec/tests/g723_1_vectors.rs) runs all 28 cases when `G723_1_VECTORS` points at them.

## FFmpeg is not the Recommendation

The old port had been pinned against FFmpeg on two packet streams. One is a real Olympus LP recording, and the new decoder gives FFmpeg's output on it, postfilter on and off. The other is 900 synthetic packets of every frame type, corrupted ones included. On that one the two decoders part ways: 520 frames differ.

So we asked which of them follows the Recommendation. FFmpeg's G.723.1 decoder gives the ITU's output on all 12 official and extra cases that have no erased frame, so it is a good decoder. Then we looked at where the two outputs separate. There are 21 such points in the stream. 20 of them start at the same kind of frame: **an untransmitted frame that follows speech with no SID frame in between**. Annex A calls this "first SID lost": the decoder has to rebuild a comfort-noise level from the speech it has just heard. The 21st point carries on from state left behind by an earlier one. The new decoder reproduces the ITU reference on this case, 283 times over in the random-bits vector. FFmpeg does something else.

The test now expects the ITU's behaviour, and says why ([`tests/g723_1.rs`](../vendor/dss-codec/tests/g723_1.rs)). On real recordings this case does not arise: Olympus recorders write LP with every frame active. It is worth a note to FFmpeg all the same.

## What the swap turned up

Swapping one decoder for another is the moment to decode everything again, old and new, and compare. That is 355 files: the 272 production recordings and every sample we have from Patrick, ourselves and Kieran's crate. All of them came out bit-identical except four of Patrick's n9.0.2c samples. Three of those were wrong under **both** decoders, and the fourth was refused.

They are the files where Olympus's R8 encoder writes **an empty LP block in front of audio of another format**: `olympus-sp-empty-lp.dss`, `olympus-sp-empty-lp.ds2` and `olympus-qp7-empty-lp.ds2`. Our demuxer took the codec from the first audio block, saw LP, and decoded the whole file as G.723.1, which turned it into noise. Each decoder made different noise of it, and that is all the swap changed.

The DS2 QP path already had the right rule: the codec is that of **the first block with frames**. It now applies to DSS and DS2 SP and to the QP demuxer itself. What we checked:

- `olympus-qp7-empty-lp.ds2` gives FFmpeg n9.0.2c's output, sample for sample.
- For the two SP files the judge has to run at 11000 Hz, and n9.0.2b, the release this decoder follows, cannot open them: it refuses the DSS one and reads the DS2 one as LP. So we removed the empty block and gave the cleaned files to n9.0.2b. Its output is identical to ours on the original files.

The fourth of these samples, `olympus-sp-empty-qp.ds2`, has an empty **QP** block in front of SP audio. It used to be refused, and it now decodes, with the same proof: n9.0.2b's output on the file with the empty block cut out.

All four are now in [`tests/fate_vectors.rs`](../vendor/dss-codec/tests/fate_vectors.rs).

## Back to Kieran's crate

The decoder went to Kieran's crate first, on top of Patrick's PR, together with one more thing that crate lacked: files that lost their header. [dss-codec#26](https://github.com/hirparak/dss-codec/pull/26) was cut down to just these two. With it, every DSS and DS2 format we know of decodes in an MIT crate:

- 28 of 28 G.723.1 cases identical;
- 8 of 8 LP samples identical;
- 272 production recordings unchanged, bit for bit, against Patrick's PR;
- 271 production recordings with their header cut off decode to the same samples as the whole files, and none of 3,326 other files is taken for one.

## What is not proven

- Patrick's LP samples hold only 24-byte 6.3 kbit/s frames, as Olympus recorders write them. Inside LP blocks, 20-byte, SID and untransmitted frames are read by their type bits, as the Recommendation sizes them. The G.723.1 decoder handles all of them exactly, but no real recording has shown them inside a DSS block.
- The four real DS-4000 LP recordings this project once had are no longer on our storage, so the real-world LP evidence is Patrick's encodes of a public-domain reading.

*Next time a format is "only" a published standard, check whose terms its implementations carry.*
