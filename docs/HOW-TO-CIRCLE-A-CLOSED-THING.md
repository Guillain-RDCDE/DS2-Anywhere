# How to circle a closed thing

*The method behind the hack, rather than the result. What we actually did when we were stuck, which questions moved us and which only felt like progress, and the one mistake that cost the most. Written to be useful whether this is your first time opening something closed or your hundredth.*

---

There are three ways to read this project. [The story](THE-STORY.md) is what happened, told as it happened. [The paper](THE-DSS-PAPER.md) is what we now know, stated precisely. This page is the third thing, and the hardest to write down: **how you get from the first to the second when you have no idea what you are doing.**

It uses one worked example throughout — a proprietary dictation format — but almost nothing here is about audio. The situation generalises: something is closed, it misbehaves, and nobody is going to tell you why.

---

## 1. What "closed" actually means

A closed system is not a wall. It is a system that will not *explain* itself. That distinction matters, because it tells you where to push.

A closed format still has to be read by something. A closed protocol still has to be spoken to. A closed binary still has to run. Every one of those is a surface where the system does the very thing it refuses to describe — and doing is a much richer source than telling.

So the first move is not to attack the secret. It is to **inventory the surfaces where the system is obliged to behave**:

- Its own files, which encode its assumptions whether it likes it or not.
- Its own binaries, which can be read, and better, *run under observation*.
- Its own outputs, which can be compared against something.
- Its own claims — timestamps, lengths, counts, checksums — which it wrote down for its own use, not for yours, and which are therefore honest.

That last category is chronically underused, and it is where our whole rate question was eventually settled. Hold onto it.

---

## 2. Find the thing that cannot lie

Here is the single most useful idea in this project, and the one we found last.

When you are trying to tell whether you have decoded something correctly, the obvious instrument is **similarity**: does my output resemble the reference? Correlate them, listen to them, diff them. It feels rigorous. It produces a number.

Similarity measures degrade *gracefully*. That is exactly what you do not want. A correlation of 0.03 and a correlation of 0.93 look like points on the same scale — "far" and "close" — when in fact one of them means "unrelated signal" and the other means "nearly right", and no amount of staring at the number tells you which regime you are in. We spent days inside that ambiguity.

What breaks a deadlock is a **hard constraint**: a property the data must satisfy exactly, that cannot be nearly satisfied, and whose violation admits no interpretation.

Our example. The codec transmits seven pulse positions as a single combined index in a 31-bit field. Seven positions chosen from 72 slots is

```
C(72,7) = 1 473 109 704
```

but 31 bits holds up to 2 147 483 647. **Between those two numbers sit 674 million values that encode nothing.** A correctly framed stream can never produce one.

That test needs no reference decoder, no listening, no time alignment, no judgement. It is binary. Run it over a file that decodes badly and impossible values appear scattered throughout; run it over a good file and there are none. It relocated the entire problem — from the codec, where we had been digging, to the layer before it — in about ten minutes.

**How to find your own version of this.** Look for places where the encoding space is larger than the value space. They are everywhere once you look: an enum with 3 valid values in a byte; a length field that must match a chunk you can measure; a count that must equal what you counted; a checksum; a field whose range is bounded by physics; a delta that must keep a running value inside a legal window. Any of these gives you a yes/no on a single frame, with no reference implementation in sight.

The general form:

> **Prefer a constraint that can be violated over a signal that can be compared.**

---

## 3. The system describes itself — read that before you guess

Formats designed to be edited by firmware tend to be **self-describing**, because the firmware has little memory and cannot hold global state. It writes down what it needs locally, in every block.

Our container does exactly that. Every 512-byte block carries a six-byte header, three fields of which describe framing: how much of the previous frame spills into this block, the byte-swap parity of the first whole frame here, and how many frames the block holds.

Every open implementation — including ours, including FFmpeg's, including the ports downstream — read those six bytes, threw them away, and worked out the framing by running forward from the previous frame instead.

Why? Because **it works**. On a recording captured without interruption, the running walk agrees with the declared framing at every single block. The fields are redundant. Every test file anyone had ever tried was such a recording.

Which brings the lesson:

> **A field that seems redundant is telling you about a case you have not seen yet.**

The designer did not spend six bytes per block for decoration. If a self-describing field appears to duplicate something you can compute, the right question is not "why is this here?" but "**what situation makes these two disagree?**" For us the answer was: a recording paused, resumed, or edited on the device. From that block on, the running walk was one byte out of phase for the rest of the file, and every remaining frame decoded into noise.

---

## 4. The trap: a good hypothesis is more dangerous than a bad one

This is the part we got wrong in public, and the reason this page exists.

The symptom was audio whose energy grew without bound, worse the longer the recording ran, until samples slammed into the rails. In a codec of this family that points immediately at the synthesis filter, and specifically at the *direct-form* filter structure — the one every textbook warns you about, because quantisation error feeds back through the recursion and can compound.

The hypothesis wrote itself: accumulated error in a direct-form filter, worse on longer files. It matched the symptom precisely. The remedy wrote itself too: rebuild the filter as a lattice, which is unconditionally stable for the coefficients this format carries.

And **it worked**. The energy stopped running away. The ratio against the reference fell from 5× to 1.4×. We published it — a patch to FFmpeg, a correction on the upstream tracker, a merged pull request.

It was wrong. Not partly wrong: wrong about which component was broken.

Because the frames reaching the filter were already corrupt, and:

> **A stabilising change applied to a system fed corrupt input will damp the symptom without touching the cause.**

A lattice fed coefficients that were never written still produces nonsense. It just produces *bounded* nonsense. What we had built was a very good clamp on a signal that was meaningless to begin with.

The tell was on screen the whole time. Correlation with the reference read **0.03 before the change and 0.03 after**. We were watching the number that was moving, because a moving number feels like progress, and ignoring the number that was not, because a stuck number feels like a measurement problem.

**The habit to build:** when a change improves a metric, ask a second question before you celebrate.

- Did the metric move, or did it **arrive**? Improvement without arrival is information about your change, not about the bug.
- Which metric would have to move if my *diagnosis* were right, as opposed to my *patch* being soothing?
- If I had made a change that merely suppressed the symptom, what would I be seeing right now — and is it different from what I am seeing?

A plausible hypothesis is dangerous precisely because it survives weak evidence. Bad hypotheses die on their own. Good ones need you to kill them.

---

## 5. Run the closed thing, do not only read it

Static reverse engineering — disassembly, decompilation — tells you what the code *is*. It is slow, and it is easy to misread structure for meaning.

Running the closed component under observation tells you what it *does*, on real input, at speed. When it is available, it is worth more.

In this project both mattered, and it is worth being precise about which gave what. Decompiling the vendor's parser (436 functions) gave us the **vocabulary**: the frame-size table, the meaning of each header byte, the internal 12000 Hz pipeline. It did not tell us that those fields were being ignored by every open implementation. That came from the constraint test in §2.

So the honest division of labour:

- **Static reading** gives you names and structures — the map.
- **Dynamic observation** gives you ground truth on specific inputs — the territory.
- **A constraint on the data itself** tells you when you are lost, without needing either.

The third is the cheapest and is usually discovered last, which is exactly backwards.

---

## 6. Choose instruments that fail differently

When you cannot trust any single measurement, use several that cannot be wrong in the same way. We ended with four:

| Instrument | What it catches | How it can mislead you |
|---|---|---|
| Correlation against a reference decoder | gross divergence | degrades gracefully; blind to constant gain; the reference is not ground truth |
| The impossible-index constraint | misframing, exactly | says nothing about audio quality |
| The duration the recorder wrote in the file | rate errors, lost frames | truncated to the second; useless on short files |
| The existing regression test suite | unintended change to healthy files | only covers what someone already thought to test |

The fourth deserves a note. FFmpeg ships a DSS test whose sample is an ordinary undisturbed recording. Our framing change had to leave its output **bit-identical** — because on such a file the declared framing and the running walk agree, so a correct fix changes nothing. A patch that altered that output would have been wrong by construction, whatever else it improved. An existing test suite is not just a safety net; used this way it is a *proof obligation*.

And the third instrument is the one to steal. The recorder writes the recording's length into the file header, in plain ASCII, for its own display. It has no interest in our argument. Dividing decoded samples by that declared length gave us the sample rate to within 9 Hz on a ten-minute file — and later revealed, on a real delivered recording, that we had been shipping audio eight seconds short of what the device captured.

> **The system's own bookkeeping is the most honest witness you will get, because it was not written for you.**

---

## 7. When you are stuck, change the question

Being stuck usually means you are asking a question your instruments cannot answer, and answering it harder will not help. Some concrete moves that unstick things, roughly in order of how often they work:

**Reframe from "why is this wrong?" to "when is this right?"** Our files did not divide into working and broken; they divided into *undisturbed* and *edited*. That reframing named the bug before we understood it.

**Find the boundary and walk up to it.** Not "this file is bad" but "this file is good until frame 9562". A defect with a location is a different problem from a defect without one — usually an easier one.

**Ask what would have to be true.** If it were the filter, the same file should misbehave with different table data. It did not. That is a five-minute experiment that should have killed our hypothesis on day one.

**Go and look at who else has the problem.** We eventually searched properly and found another maintainer's fork with parallel work on the same format. Two independent decoders failing at *the same frame* is not a coincidence — it is a message about where the fault is not. We had spent days assuming we were alone; we were not, and it was findable.

**Reduce until it is boring.** The final reproducer is one recording paused mid-capture, one decode, one clipping measurement. Everything ornate we built along the way was scaffolding.

**Explain it out loud to someone who will not be polite.** Ours was blunt: *stop bringing me an absurd debate, go and look it up, we cannot be the only ones asking.* That instruction — go outside your own head — was the turn.

---

## 8. Write down the dead ends

We keep our wrong chapters in this repository, each behind a banner saying it is wrong, because a reverse-engineering log whose failures have been tidied away is worth much less than one where they are marked.

Two reasons, one generous and one selfish.

The generous one: the next person is standing at the same fork, and the wrong branch looks just as attractive to them as it did to us. Telling them where it leads is most of the value we can offer.

The selfish one: writing down *why* a wrong answer was convincing is how you learn to recognise the shape of it. "We thought it was the filter" is not useful. "We thought it was the filter because the symptom matched a known failure mode of that component, and our fix improved the metric we were watching without ever making it correct" is a pattern you can catch next time — and there will be a next time, because we did it twice on this same format.

Publishing a correction costs less than you fear, too. We had to write to a mailing list and to an upstream maintainer and say: the patch we sent you is wrong, please reject it. The response was *"Merged, and thank you for the extra investigation!"* Nobody minds a corrected mistake nearly as much as the person who made it expects.

---

## 9. If this is your first time

You do not need permission, a licence, or a disassembler to start. A first pass on almost any closed format looks like this:

1. **Get many samples**, not one. Differences between files carry more information than any single file does.
2. **Look at the bytes.** `xxd yourfile | head -40`. Find the parts that are the same across every file — those are structure. Find the parts that change — those are data.
3. **Find the units.** Almost every container is built from repeated fixed-size blocks. Look for a period: a byte pattern that recurs every 512, or 1024, or 2048 bytes. That period is the format's heartbeat.
4. **Find the counters.** A number that increases by one per block, or a length that matches something you can measure, tells you what the format thinks it is doing.
5. **Find the ASCII.** `strings` on a binary format still finds timestamps, device names, and — as here — a duration written in plain digits, which turned out to be the most valuable field in the whole file.
6. **Change one byte and see what breaks.** Destructive testing on a copy is the fastest way to learn what a field means.
7. **Write down what you think each field means, with your confidence.** Being explicit about "I am guessing" is what lets you revisit it later instead of building on sand.

None of that requires expertise. It requires patience and a willingness to be wrong in writing.

And when you get stuck — and you will — come back to §2. Ask what the data *cannot* do. Somewhere in the format there is a value that would be impossible if you were reading it wrong, and finding it is usually the whole game.

---

## The short version

- Inventory the surfaces where the system must behave, not the secret it will not explain.
- Prefer a constraint that can be violated over a signal that can be compared.
- A field that looks redundant is describing a case you have not seen.
- A plausible hypothesis is more dangerous than an implausible one; kill it deliberately.
- Improvement is not arrival. Ask which number would move if the diagnosis were right.
- Use instruments that cannot fail in the same way.
- The system's own bookkeeping is the most honest witness available.
- When stuck, change the question — and go and look at who else has the problem.
- Keep the dead ends, labelled.

---

*The worked example in full: [the paper](THE-DSS-PAPER.md). What actually happened, in order: [the story](THE-STORY.md). Where we were wrong, preserved on purpose: [chapter 16](16-the-q15-instability.md), [The Lattice Hunt](THE-LATTICE-HUNT.md), and the correction in [chapter 17](17-the-framing-was-wrong.md).*
