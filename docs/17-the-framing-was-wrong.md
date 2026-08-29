# 17 — The framing was wrong all along

*Or: how a confidently published fix damped a symptom for two days while the actual bug sat six bytes away, in a header everyone had agreed to skip.*

---

This chapter closes the thread that [16 — The Q15 instability](16-the-q15-instability.md) and [The Lattice Hunt](THE-LATTICE-HUNT.md) opened. Both of those are wrong about the cause, and they are kept standing behind banners, because the way they went wrong is the most useful thing in them.

## The symptom, one more time

DSS SP recordings decoded by the open implementations — FFmpeg's `dss_sp.c`, the Rust `dss-codec` crate, and the npm and PyPI ports derived from it — sounded fine on most files and catastrophic on some. On the bad ones the output was not merely noisy: its energy grew without bound until samples slammed into the 16-bit rails, worse the further into the recording you went. About 0.6% of all samples ended up clipped.

A commercial decoder handled the same files without complaint. So the information was in the file, and something in the open path was destroying it.

## The answer we published, which was wrong

Unbounded energy growth in a CELP decoder points straight at the synthesis filter. DSS SP synthesises through a 14th-order direct-form LPC filter, and direct form is the structure every textbook warns you about: quantisation error feeds back through the recursion and, with unlucky coefficients, compounds.

The theory wrote itself. Accumulated Q15 error in a direct-form filter, diverging over time, worse on longer files — which matched the observation that it got worse further in. The fix wrote itself too: rebuild the synthesis as a lattice, unconditionally stable for reflection coefficients inside the unit circle, and a natural fit for coefficients the bitstream already carries in that form.

It worked, in the sense that the symptom improved a great deal. Energy stopped running away. Against the reference, the energy ratio came down from 2.6–5.3× to 1.3–1.5×. That looked like a fix, and it was published as one: a patch to FFmpeg's mailing list, a correction on the upstream issue tracker, and a pull request against `dss-codec` that was merged.

It was wrong. Not slightly wrong — wrong about which component was broken.

The trap deserves a name, because it is general:

> **A stabilising change applied to a system that is being fed garbage will damp the symptom without touching the cause.**

A lattice filter fed reflection coefficients that were never written to the file still produces nonsense. It just produces *bounded* nonsense. The improvement from 5× energy to 1.4× energy was the filter refusing to explode; it was not the decoder starting to be right. Sample correlation with the reference sat at **0.03** before the change and **0.03** after it, and 0.03 is not a decoder that is nearly working — it is a decoder emitting an unrelated signal.

Both numbers were on the screen the whole time. We watched the one that was moving.

## Getting a ground truth that cannot be argued with

The way out was to stop reasoning about the audio and start asking whether the *bits* were even possible.

DSS SP packs seven pulse positions into one combined index. Seven positions from 72 slots is C(72,7) = **1,473,109,704** combinations, and the field carrying the index is **31 bits** wide, which holds up to 2,147,483,647. So there is a gap: any value above 1,473,109,704 encodes a combination that does not exist. A correctly framed bitstream can never produce one.

That is a free oracle. No reference decoder, no listening, no correlation, no alignment: hand it a frame and it tells you whether that frame was even plausibly a frame. Run over a file that decoded badly, it found impossible pulse indices scattered throughout. Run over a file that decoded well, it found none.

The frames were wrong before they ever reached the filter. Everything about the synthesis had been a red herring.

This is the lesson worth carrying out of the whole affair. Correlation against a reference is a *similarity* measure, and similarity degrades gracefully — which makes it bad at distinguishing "nearly right" from "completely unrelated." The impossible-index test is a *hard constraint*: it cannot be nearly satisfied. It found in minutes what days of listening and correlating had not.

## Reading the container instead of guessing at it

With the search narrowed to framing, the remaining question was what the correct framing actually is. That meant the container, and the container is not documented.

Two DLLs from the manufacturer's own tooling — `DssDecoder.dll` and `DssParser.dll` — went through Ghidra. The parser decompiled to 436 functions. What mattered:

- A **frame-size table** indexed by a mode byte in each block header. Mode 0 is 328 bits, the 41-byte DSS SP frame. Mode 2 is 192 bits — which is **G.723.1**. So "DSS LP" is not a codec anyone needs to reverse engineer; it is G.723.1 in a different wrapper.
- The **512-byte block structure**, and what its six header bytes mean: byte 0 bit 7 is a byte-swap parity, byte 1 gives where the first whole frame begins, byte 2 is the frame count, byte 4 the frame mode.
- The **internal rate**: synthesis runs at 12000 Hz, and the output stage decimates by 11/12, taking 288 samples per frame down to 264.

That last one settled a separate question that had been quietly wrong for years. It gets its own section below.

## The actual defect

Every 512-byte audio block declares its own framing. It states:

- how many bytes at the start of its payload still belong to the frame that began in the **previous** block,
- the byte-swap parity of the first whole frame it contains,
- how many frames it holds.

Every open implementation skips all three. They read the six-byte header, throw it away, and derive the framing by running on from the previous frame — from the first block of the file to the last.

On a recording where the capture was never interrupted, the running walk agrees with the block headers at every single block. The headers are redundant, the walk is right, the file decodes perfectly. **That is most files, and that is why this survived for years.**

On a recording that was paused and resumed, or edited on the device, the block where the recording resumes **restates** the framing — different carry-over length, different parity. The running walk does not look, does not notice, and from that block onward reads every frame one byte out of phase for the rest of the file. Reflection coefficients land in the pulse fields, pulse positions land in the gain fields, and the synthesis filter is asked to make speech out of it. It does what any resonant filter does with noise in its coefficients: it rings, and it grows.

Ignoring the frame count has a second effect, milder but always present: the walk reads past the last recorded frame in a block and decodes whatever padding follows, manufacturing audio that was never captured. That is why some files decode noticeably *longer* than the duration their own header announces.

The fix is to believe the block. Start each block where its header says its first frame starts, with the parity its header gives, and emit exactly the number of frames it declares — letting frames straddle boundaries by stepping over the header they meet. It costs nothing when the headers and the walk agree, which is most of the time, and it keeps the walk in step when they do not.

About a hundred lines, in the demuxer, after three days in the codec.

## Results

Against a reference decoder, on seven recordings from Olympus and Philips machines:

| | before | after |
|---|---|---|
| mean sample correlation | 0.5849 | **0.9995** |
| the three misframed files | 0.03 / 0.03 / 0.04 | 0.9998 / 0.9990 / 0.9997 |
| clipped samples on those three | ~0.6% | 0.001% |

The four files that already decoded well are barely touched. Three come out with **not a single sample different**; two of those also shed a short tail — 2112 and 1320 samples — of padding the old walk had read past the last declared frame.

The fourth is the interesting one. It *looked* healthy, and it held a misframed stretch of its own that the sampled measurement windows had happened to walk straight past. Two percent of its samples change, and its clipped-sample share falls from 0.016% to 0.001%. **A file can be quietly damaged without ever sounding obviously broken**, which is a decent argument against trusting spot checks — including ours.

Across a wider set of 187 recordings, nothing that decoded before stops decoding, and four files that had produced no audio at all now decode. Where honouring the frame count makes the output shorter, the new length agrees to a tenth of a second with an independently validated decoder — so what disappeared is audio that was being invented.

FFmpeg's own regression tests, `fate-dss-sp` and `fate-dss-lp`, pass unchanged. On the test sample the block headers say exactly what the old walk assumed, so the output is bit-identical — which is the point: where the headers and the walk agree, believing the headers costs nothing.

## The sample rate, separately

DSS SP has been decoded at 11025 Hz since the decoder was written. It is **11000**.

The codec synthesises at 12000 Hz and the output stage decimates 11:12, taking 288 samples to 264. 12000 × 11/12 is 11000 exactly. A frame of 264 samples is 24 ms at 11000 Hz and 23.9456 ms at 11025 — and nothing in the format accounts for 23.9456 ms. Declaring 11025 plays every DSS SP file 0.23% fast: about three seconds of drift across a 23-minute recording, which is enough to pull a transcript away from its audio.

The container settles it independently, and this is the part we like best. A DSS header carries the recording length as an **ASCII HHMMSS field**, written by the recorder itself at offset `0x3E`. Divide the decoded sample count by that declared length and you get the rate the machine intended. Over **124 recordings longer than ten minutes**, the median implied rate is **11001.6 Hz**. The declared length is truncated to the second, so a ten-minute file pins the rate to within 9 Hz; 11025 sits 23 Hz away, outside the margin on the great majority of the set.

Take the median, not the mean — files with missing blocks drag the mean down to 10900 and tell you nothing.

## What to take from it

Three things, none of them about DSS.

**A stabilising fix that damps a symptom is not evidence you found the cause.** The lattice rewrite made the numbers better and the diagnosis no more correct. If a change improves a metric without that metric reaching *right*, the improvement is telling you something about the change, not about the bug.

**Look for a constraint the data must satisfy, not a signal it should resemble.** Correlation degrades gracefully, which is exactly what you don't want from a test. The impossible-pulse-index check cannot be nearly satisfied, and that is what made it decisive.

**When a format has self-describing fields, they are there for a reason.** Three independent implementations read those six header bytes and discarded them, because a running walk reproduced them on every file anyone happened to test. The fields exist precisely for the case the running walk cannot handle.

## Where it went

- **FFmpeg**: a two-patch series on `ffmpeg-devel` — the rate, and the framing — with a request to reject the lattice patch we sent earlier.
- **`hirparak/dss-codec`**: the correction posted on [issue #19](https://github.com/hirparak/dss-codec/issues/19), and a follow-up PR to undo the lattice change we had merged there.
- **Downstream**: the npm `dss-codec` and PyPI `pydsscodec` ports inherit the demuxer as it stands, so the same defect is in both.

---

*Seventeen chapters, and the one we are proudest of is still the one where we were wrong in public. Twice, now. 🔓*
