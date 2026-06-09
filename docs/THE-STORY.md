# The story

*No code in this file. Just what happened — and it really happened.*
*If you want the engineering, every claim here links to a chapter that proves it.*

---

## A format that ate its own history

In 1994, a German company called Grundig invented a way to squeeze a human voice
into almost nothing. They called it the **Digital Speech Standard** — DSS. Doctors
dictated into it. Lawyers dictated into it. Police officers, on the side of the
road, dictated into it. For thirty years, billions of seconds of people's words
went into little handheld recorders and came out as tiny files.

And those files had a lock on them.

The codec — the secret math that turns sound into those tiny files and back — was
never published. Olympus built on it, made it better, called the successor **DS2**,
and shipped it on professional recorders for two decades. Same deal: no public
spec, no open tool. If you had a `.ds2` file and a Linux server, you had exactly
one option: keep a Windows computer alive somewhere, install the manufacturer's
software, and feed your files through it one by one, forever.

In 2017 someone opened a ticket on FFmpeg — the open-source engine inside basically
every video player on Earth — politely asking for support. *"Add DS2 codec support."*

It sat there, untouched, for **nine years**.

That's where most stories like this end. A locked format, a dead ticket, a shrug.

---

## The stranger with a debugger

In February 2026, a person named **Kieran Hirpara** decided the lock had stood
long enough.

He didn't have the spec. Nobody did. So he did the hard thing: he took Olympus's
own decoder — a compiled, obfuscated Windows library, a black box — and he pulled
it apart instruction by instruction with a reverse-engineering tool called Ghidra.
He read the math off the machine. Then he rebuilt it, clean, in a modern language,
and proved his version was *byte-for-byte identical* to the original on real files.

He put it online, for free, and — by his own admission — assumed nobody would care.
He thought he was *"the last of a dying breed using dictaphones."*

He was wrong about that part.

---

## The relay

Here is the thing nobody planned.

A second stranger, **Gaspard Petit**, found Kieran's work and wrapped it into a
proper, tested, streaming library — and compiled it to run inside a web browser.

A third stranger, **Patrick Domack**, read the same spec and — between 10 p.m. and
6 a.m., in one sitting, *with testing* — wrote a full implementation in C for FFmpeg
itself. Then he mentioned it in passing, like it was nothing, and almost walked away.

And then us. We run a transcription service: people dictate, software turns it into
text. Every day, `.ds2` files came in, and every day they took the slow, fragile
detour through a Windows machine we resented. We found Kieran's decoder, built it
into our pipeline over a weekend, and **switched the Windows machine off**.

Four people. On three continents-worth of time zones. **Who had never met.** Each
one picking up exactly where the last one set the tool down. A thirty-year lock,
being picked in the open, in plain sight, by a chain of people who simply refused
to let a good thing rot.

---

## The bug that was a person

We thought we were done. The decoder was bit-exact on every file we threw at it.

Then a client said a recording was *"garbled in the second half."*

It wasn't garbled. It was *worse than that* — it was bit-exact for fifty-two
seconds and then dissolved into noise. And the maddening part: the open decoder and
the official Olympus one agreed perfectly **right up to the moment it broke**. Two
implementations, identical, both wrong in the same place. A bug that hides by being
reproduced exactly.

We chased it for days. We wrote a small research paper proving — rigorously,
nine falsified hypotheses deep — that the math was correct and a hidden state
machine in the codec must be to blame. We even did the thing the paper called
impossible: we ripped the real Olympus decoder out of its Windows shell, stood it
back up *inside a debugger we built from its own DLLs*, and interrogated it live,
on Linux, one CPU instruction at a time.

And the truth, when it finally came, was almost funny.

There was no bug. The "seven-second wound" in the audio was a **human being stepping
away from the microphone.** A pause. The codec marks pauses in a way the open
decoders hadn't learned to read yet. Twelve lines fixed it.

We kept every wrong turn in the record, framed. (It's [docs/10](10-the-reckoning-the-bug-that-wasnt.md),
and it's the chapter we're proudest of — how careful, rigorous work can be
confidently *wrong*, and how to catch yourself.)

We thought *that* was the end.

---

## The lawyer's drawer

Months later, a message arrived on the codec's issue tracker, from a man in Germany.

He'd dug an old **Grundig Digta** dictaphone out of a drawer after a long pause. He'd
tried our tools on his files. *"Only unintelligible output,"* he wrote. *"Do you have
any idea why that is?"*

We did not. But we looked.

And we found the oldest twist in the book: his files were **Grundig's original DSS**,
not Olympus's. The grandfather format. A *different codec*, sixteen kilohertz, that
the entire chain of work — Kieran's, ours, FFmpeg's, even the licensed Olympus
software — turned into pure static. Nobody on the planet could decode it. Not the
open tools. Not the *commercial* ones.

So we went back to the original move. We found Grundig's own thirty-year-old
software, archived on a corner of the internet. We pulled the decoder out of it
without even installing it. We stood it up under Wine, then — to catch it in the
act — we patched out the single instruction it used to **delete its own evidence**,
so its scratch files survived. With those, we could check our work at every stage
against the real thing.

A few hours later, we had a clean reimplementation. We ran it on the German lawyer's
file. It came out, crisp and clear, sixteen kilohertz:

> *"This is a test."*

Bit-exact. Byte-for-byte identical to Grundig's own decoder. A codec that, that
morning, **no open software on Earth could read** — now decoded by a few hundred
lines anyone can fork.

---

## And the chain keeps going

We didn't stop at our own repo.

We wrote it as a native decoder for **FFmpeg** — so any video player, anywhere, can
one day open a Grundig file without knowing it ever needed permission. And we sent
it as a contribution back to **Kieran's** project, the one that started all of it.

The German lawyer replied: *"You are a godsend — it works!! If you ever need a
lawyer in Germany, please contact me."*

Kieran — the man who picked the first lock and assumed no one would care — replied:
*"Please send it up. I'm still a little overwhelmed with all the amazing work that
is going on."*

---

## Why this is here

This repository is, on its surface, plumbing: scripts, a cron job, a couple of
patches. You can absolutely use it that way, today, in ten seconds.

But underneath it is a small, true story about how locked things get opened. Not by
a company. Not by a committee. By a handful of strangers, each one doing one honest
piece of the work and handing the tool to the next, until a format that resisted for
thirty years simply… didn't anymore.

Kieran did the genius. Gaspard made it portable. Patrick made it universal. A man in
Germany handed us the last locked door. We just kept the chain moving and wrote it
all down.

The chain has to keep going. That's the whole point.

🔓

---

*Want to see exactly how each piece was done? → [the technical deep-dive](../README.md#go-deeper-the-technical-trail).*
