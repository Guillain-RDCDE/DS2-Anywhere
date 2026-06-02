# 10 — The reckoning: the bug that wasn't

*How we built a debugger-grade oracle from the closed-source decoder's own DLLs to settle a seven-second mystery — and how the cheapest test in the world, listening, dissolved it. The most honest chapter in this repo.*

> **Abstract.** Chapter 09 is a research paper that corners a residual DS2 decoding bug with real rigour: analysis-by-synthesis proves the spectral filter is bit-exact, nine hypotheses are falsified, the divergence is pinned to a single re-sync block, and the open question is handed off cleanly. It is also, in its central claim, **wrong** — and we are leaving it standing, unedited except for a banner, because the honest value is in *how* a careful investigation can chase a measurement that was never a sound. This chapter is the reckoning. We did the thing chapter 09 said was blocked: we made the real Olympus decoder run under our own instrumentation (Linux + Wine + gdb). It taught us the format's connection ritual at the instruction level — and it taught us something sharper: the oracle we built to judge our decoder needed judging itself. Then we stopped measuring and *listened*. The file was clean. There was no seven-second wound. The "10× excitation collapse" our math screamed about was a person walking to a quieter corner of the room. We tell you exactly how far the rigour went, exactly where it fooled us, and exactly what we can and cannot prove. **No bug — and an honest accounting of why we thought there was one.**

---

## Part 0 — For the newcomer: what just happened in this saga

If you've read chapters 06–09, skip ahead. Otherwise, sixty seconds.

A dictaphone stores voice as a *recipe* (CELP) rather than a waveform. Decoding means reading the recipe and rebuilding the sound. Over chapters 06 and 07 we found and fixed two **real** bugs in how the file is cut into frames (the *demuxer*): empty pause-markers (chapter 06) and an over-counted re-sync block (chapter 07). Both fixes are proven bit-exact against the official Olympus software, deployed in production, and submitted upstream to FFmpeg. They are solid. Nothing in this chapter touches them.

What this chapter is about is the *third* thing — the one we thought was left over. After those two demux fixes, one test file still seemed to have a seven-second stretch of bad audio right after a pause. Chapters 08 and 09 chased it as a **decoder** bug and built a beautiful case that it was real. This chapter is what happened when we finally settled it. The short version: it wasn't real. The longer version is worth your time, because the way a true-looking thing turned out to be false is the most useful lesson in the whole repo.

---

## Part 1 — Where 09 left us, and why we couldn't let it sit

Chapter 09's wall was specific: to see *exactly* what the real decoder does to the excitation across the re-sync block, you have to watch it run, and the Olympus decoder refuses to run anywhere you can instrument it. NCH Switch (which decodes fine) is anti-debug. The two Olympus DirectShow filters (`DssParser` → `DssDecoder`) refuse to interconnect through any public API. So 09 closed on an honest "we can't see inside, here's the open problem."

We had two reasons not to leave it there. The small one: pride — a repo that prides itself on reading the silicon shouldn't quit at a locked door it hasn't actually tried every key on. The big one: **the entire case rested on one number** — the inverse-filter A/B that said the real decoder's excitation in the band was 10× louder than ours and totally decorrelated. Every conclusion in 08 and 09 inherited the trust we'd placed in that number. Before we wrote "unsolved decoder bug" into the permanent record and told the FFmpeg list their patch had a hole, that number deserved a second, independent witness.

So we set out to do the one thing 09 said was blocked: get the real decoder running where we could watch it.

---

## Part 2 — Building the oracle: the Olympus decoder, on Linux, under gdb

The breakthrough was a change of host. Chapter 07 had already re-hosted the Olympus DLLs inside a hand-built DirectShow graph *on Windows* to read the parser. For a fully instrumentable oracle we went further and stranger: **we ran the Windows decoder on Linux, under Wine, under gdb.**

The harness is a small Win32 program, cross-compiled with mingw and run through Wine on a headless Linux box:

```
i686-w64-mingw32-gcc harness.c -lole32 -loleaut32 -lstrmiids -luuid
```

It builds the graph by hand and feeds it the file:

```
[ our own source filter: IFileSourceFilter + IAsyncReader ]
            │
            ▼
   real DssParser.dll   (Olympus demuxer)
            │
            ▼
   real DssDecoder.dll  (Olympus CELP synthesis)
            │
            ▼
[ our own sink filter ]  ← every PCM sample the real decoder produces
```

Under Wine the whole thing is a normal Linux process. gdb attaches. Breakpoints land in Olympus's own code. We could, at last, single-step the closed decoder.

**One discipline mattered enough to call out:** Wine randomises module load addresses on every run (ASLR). Every breakpoint address has to be computed from the module base the harness prints at startup — never hard-coded. Hard-code one and you set a breakpoint in the void and conclude, wrongly, that a function never runs. (We did this once. It cost an evening.)

### What the silicon told us — genuine, durable RE

Standing the oracle up paid for itself in pure format knowledge, independent of the bug:

- **The parser doesn't read the file the obvious way.** `DssParser`'s `CompleteConnect` doesn't pull bytes through its input pin first. It calls `QueryPinInfo` on the upstream pin, then queries the upstream **filter** for `IFileSourceFilter` (`{56A868A6-…}`) and calls `GetCurFile` to recover the `.ds2` path directly. Only then does it read, via `IAsyncReader` (`SyncRead` / `Request` + `WaitForNext`). This is why every naive "wire the pins together" attempt in chapters 08–09 failed: the connection ritual is bespoke, and it reaches *around* the pin graph to find the filename.
- **The format descriptor, read straight from memory.** Once connected, the parser fills a format block tagged `427032AF` with `80 3E 00 00 | 60 6D 00 00 | 06 00 00 00` = **16000 Hz, 28000, format 6** — the QP profile, confirmed in the decoder's own words rather than inferred.

That knowledge is real and it stays. Then the oracle turned on us.

---

## Part 3 — The oracle needs an oracle: the underwater reference

Here is the turn, and it is the whole chapter.

To make `DssDecoder` accept the parser's output and actually decode, its `CheckMediaType` (`DssDecoder.dll+0x2760`, reached from `ReceiveConnection`) runs a classifier (`+0xfd50` → validator `+0xfe70`) that **chooses the CELP mode** (it returns 1, 2, or 3) and, to do so, consults an Olympus configuration key in the Windows registry — a key that does not exist on a bare Wine install. Missing key → the decoder rejects the connection with `0x83e807d0`. The graph won't run.

The tempting fix — and we took it, to get *anything* moving — was to patch past the check: overwrite the validator's entry with `mov al, 1; ret` so it always says "yes." The graph connected. The decoder ran. Audio came out.

**And the audio was wrong.** Garbled, swimmy — "a man talking underwater." Because the validator we'd lobotomised wasn't just a gate; it was the thing that **selected the CELP mode**. Force it to return a constant and the decoder synthesises in the wrong mode. The output isn't Olympus's decode of the file; it's Olympus's decode *of the file in the wrong mode*, an artefact we manufactured.

Sit with what that means. We had set out to build a trusted oracle to check our 10× number. The oracle we built, the moment we forced it to run, produced output **we already knew was false** — and it was loud and decorrelated, the *exact texture* of the "bug" we were chasing. A faithful oracle requires importing Olympus's real configuration so the classifier picks the genuine mode; the patched oracle is worse than no oracle.

The ground had shifted. If the reference-of-references we'd just built could be subtly, confidently wrong in a way that looked exactly like the symptom — what about the Switch reference the entire 10× case rested on? We had spent two chapters treating "the reference is ground truth" as axiom. We had just watched a reference lie to us.

---

## Part 4 — The cheapest experiment in the world

We stopped computing. We took the file — the actual one, decoded by our actual production decoder with both demux fixes in place — and we **listened to it, end to end.**

It was clean.

Not "mostly clean." Not "clean except seven seconds." Clean, beginning to end, the way every other dictation off that pipeline is clean. There was a passage in the middle where the voice gets softer and the room tone changes — and that passage sits exactly where our spectrograms had been screaming. It is not garbled. It is **a person who has walked a few steps away from the microphone, or turned to face a document, in a different corner of the room.** A human being moving through a real space while dictating. The "wound" was someone's footsteps.

This is the part worth being slow about, because it is where rigour failed and the ear didn't. A genuine 10× excitation collapse with correlation −0.02 is not a subtle thing. It is *grossly* audible — it is the difference between speech and noise, the literal definition of "garbled" chapter 06 opened with. If that had been in the file, you could not miss it. It was not in the file. Therefore — and this is forced, not hoped — **the number was not measuring the sound.** Our decoder's output was faithful. The thing the inverse-filter A/B diverged from was not the truth of the file.

---

## Part 5 — What we can prove, and what we can't (the honest part)

This is the section the rest of the repo earns the right to have. We will not dress up the limit.

**What we can prove:**

- The two demux fixes (chapters 06, 07) are bit-exact and correct. Re-verified, unaffected, deployed.
- Our production decoder's output for the contested file is **perceptually faithful** end to end — confirmed by ear against the source, the lowest-tech and, here, the most decisive test available.
- A real defect of the magnitude the A/B reported would be unmistakably audible. It is absent. So the A/B was not tracking audible decode quality.

**What we cannot prove — and won't pretend to:**

We did **not** produce a clean post-mortem isolating the exact step at which the inverse-filter A/B misled us. We have three candidate explanations and we did not run the contest to a verdict, because once the ear settled it, re-litigating a non-problem was not a good use of anyone's night:

1. **A contaminated reference.** The "ground truth" excitation in the late comparison runs may have drifted from a clean Switch decode toward something less faithful — and Part 3 proves how easily a reference can be wrong while looking authoritative. A loud, decorrelated reference produces exactly the 10× decorrelated result.
2. **Band-local misalignment.** The re-sync run is precisely where frame timing shifts. An inverse-filter A/B that aligns perfectly outside the band (correlation 1.000) but carries a small sample offset *inside* it would manufacture decorrelation from two correct signals that are simply slid apart by a few samples.
3. **A metric that doesn't map to perception.** Excitation-domain energy and correlation are not loudness and intelligibility; a number can move 10× in a region the ear hears as "a bit softer."

Most likely it is some braid of (1) and (2). We are not certain, and we are telling you we are not certain. The discipline that matters is the one we should have applied two chapters earlier: **a measurement is a hypothesis about a sound; close the loop with the sound before you carve the conclusion in stone.**

---

## Part 6 — Why we are not deleting chapter 09

The easy move is to quietly delete 09 and 08 and let the repo read as an unbroken string of wins. We're not doing that, for three reasons.

- **It's dishonest.** The investigation happened. The hours were real. Hiding the wrong turn would make the repo a highlight reel instead of a record.
- **The method is genuinely good.** Analysis-by-synthesis — inverse-filtering a reference to split coefficients from excitation — *is* the right technique. It correctly proved our filter coefficients are bit-exact. It failed only at the last step, where it was pointed at a reference it shouldn't have trusted. A newcomer learns more from a sharp tool used on a flawed target than from a tidy success.
- **The trap is the lesson.** "Our beautiful, internally-consistent analysis was chasing our own measurement artefact" is the single most useful thing in this repository for anyone who reverse-engineers for a living. You will do this. We did. Here is what it looks like from the inside, and here is the cheap test that breaks the spell.

Chapter 09 now opens with a banner pointing here. It is preserved as the honest record of a rigorous investigation that reached a confident, wrong conclusion — and this chapter is the correction that a professional record requires.

---

## Part 7 — What's left standing (it's a lot)

Strip away the bug that wasn't, and the saga's ledger is overwhelmingly positive:

- **Two real demux bugs, found and fixed**, bit-exact, in production, fixing files the format had silently corrupted for a decade — and a nine-year-old FFmpeg ticket explained in the process.
- **The format's connection and demux rituals, read off the silicon** — the `IFileSourceFilter` reach-around, the `427032AF` descriptor, the `2 × byte1 − 6` re-anchoring law — knowledge that exists nowhere else in public.
- **A reusable oracle.** The Wine + gdb harness that runs the closed Olympus decoder as an instrumentable Linux process is built and kept. If a file ever genuinely misbehaves, we no longer have a wall — we have a debugger pointed at the vendor's own code. (To make it a *faithful* oracle, import Olympus's real registry configuration so the classifier selects the true CELP mode; do not lobotomise the validator — Part 3 is why.)
- **A correctly scoped FFmpeg contribution.** The upstream patch needs the demux fixes (that follow-up is real and in flight). It does **not** need an excitation fix for a bug that doesn't exist — a correction we're glad we made *before* asking reviewers to chase a phantom.

The format gave up its real secrets. The one we thought it was still holding, it never had.

---

## What to take away

- **A measurement is not a sound.** Inverse-filter SNR, correlation, energy ratios — all real, all useful, none of them is the thing your user hears. Before you write "bug" into the record, listen.
- **Your reference can be the artefact.** We spent two chapters with "the reference is ground truth" as an unquestioned axiom, then watched a reference we built lie to us in the exact shape of the symptom. Trust references the way you trust any other input: provisionally.
- **A patch that makes it *run* can stop it being *right*.** Lobotomising the classifier got the decoder moving and silently changed what it computed. When you force a closed system past a gate, ask what the gate was *for*.
- **Keep your wrong turns in the record, framed.** The value of a reverse-engineering log is not that every road led somewhere; it's that the dead ends are marked so the next person doesn't drive down them. Including the beautiful one that fooled you.
- **The cheapest test is sometimes the decisive one.** A research paper's worth of rigour was overturned in ninety seconds of pressing play. Order your experiments by what they can *kill*, not by how sophisticated they look.

---

*Two demux bugs slain, one phantom laid to rest, one oracle built and kept — and the locked door, when we finally opened it, had an empty room behind it. 👂*
