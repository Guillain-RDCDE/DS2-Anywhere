# I have a dictation file. How do I get the words out?

*Start here if you have never opened this repo before, and you are not especially interested in codecs. No audio background needed, no programming background needed. Ten minutes, and you can stop reading at any point once your file has opened.*

---

## First: what you have

Someone handed you a file ending in **`.dss`**, **`.ds2`**, or **`.DSS`**. It came off a handheld voice recorder — an Olympus, a Philips, a Grundig — of the kind doctors, lawyers, surveyors and police have dictated into for thirty years.

Double-clicking it does nothing useful. VLC will not play it. That is not your fault and nothing is broken: the format was kept proprietary, and for most of those thirty years the only way to open one was the software that shipped with the recorder, on Windows.

That is what this project changed. **You can open your file right now, on any machine, without installing anything.**

---

## The fast way: drag it onto a web page

**→ [Open the decoder](https://guillain-rdcde.github.io/DS2-Anywhere/)**, drag your file onto it, get audio back.

It handles Olympus DSS and DS2, Grundig DSS, and encrypted DS2 files (it will ask for the password). You get a player and a **Download WAV** button.

**Your file never leaves your computer.** The decoder is compiled to WebAssembly and runs inside your browser tab — there is no upload, no server, no account. You can check this yourself: open the page, disconnect from the internet, and drop your file. It still works.

If you have one file, or a handful, **stop here**. You have your audio. Skip to *[Turning audio into text](#turning-audio-into-text)*.

---

## If you have a lot of files: run it locally

Two commands, and then one command per file:

```bash
git clone https://github.com/Guillain-RDCDE/DS2-Anywhere
cd DS2-Anywhere
docker compose up --build
```

That gives you a small web interface at `http://localhost:8080/convertisseur.php` where you can drop files, and an HTTP endpoint if you want to script it.

If you would rather have a plain command:

```bash
conv-dss-ds2-to-mp3 recording.dss
# [dss_sp 11000Hz, 23.4min] recording.mp3  OK  (10.71 Mo en 3.0 s)
```

For a real deployment — a watched folder, a cron job, a service — there is `sudo ./src/bin/install.sh` and [chapter 02](02-integration.md). You do not need any of that to open one file.

---

## Turning audio into text

This project gets you **audio**. It does not transcribe — that is a different problem, well solved elsewhere, and we would rather point you at the good tools than ship a worse one.

Once you have a `.wav` or `.mp3`, [OpenAI's Whisper](https://github.com/openai/whisper) and its faster reimplementations run locally and handle dictation well:

```bash
pip install -U openai-whisper
whisper recording.wav --language fr --model medium
```

Dictation is close to the easy case for speech recognition: one speaker, close to the microphone, deliberate delivery. Expect good results, and expect to still reread proper nouns, figures and dates — that is where every system slips, ours included.

A practical note if the transcript matters legally or medically: keep the original `.dss` alongside the audio and the text. The recorder's file is the primary record, and now that it is readable by open tools, it will stay readable.

---

## When something goes wrong

**"This is a DSS-LP file"** — your recording is a little older (an Olympus DS-4000, typically). Its audio is **G.723.1**, a published international standard rather than the proprietary codec. The web page cannot decode it, on purpose; any recent `ffmpeg` can:

```bash
ffmpeg -i recording.dss recording.wav
```

**"This file is encrypted"** — some DS2 files are, and you need the password the author set on the recorder. Enter it when prompted. There is no way around it and there should not be one.

**"Not a recognized dictation file"** — check the file is what you think. We have found `.dss` files in the wild that turned out to be a PNG image renamed, a file of 10 bytes, and several of exactly 0 bytes. `head -c 4 yourfile.dss | xxd` will tell you: a real one starts with a byte, then the letters `ds2` or `dss`.

**The audio plays but sounds like static** — if you are using an older build, or another tool, this is a real bug that existed everywhere until August 2026: the file's own framing information was being ignored, so recordings that had been **paused or edited on the device** were read one byte out of step and decoded into noise. It is fixed here. If another tool does this to your file, [chapter 17](17-the-framing-was-wrong.md) explains exactly what to tell its author.

**Nothing above matches** — [open an issue](https://github.com/Guillain-RDCDE/DS2-Anywhere/issues). A file that fails is genuinely useful to us; two of the formats this project supports exist because someone showed up with a recording nobody could read.

---

## So what is this project, actually?

A handful of people who never met opened a format that had been closed for thirty years, and gave the result back to everyone.

The decoding work is not ours alone and we are careful about that: [Kieran Hirpara](https://github.com/hirparak) cracked the first piece, [Patrick Domack](https://github.com/patrickdk77) and [Gaspard Petit](https://github.com/gaspardpetit) made it portable, Oleksij Rempel wrote the original decoder inside FFmpeg back in 2014. What we added is a production pipeline, two more device families, and — after getting it publicly wrong once — the fix for a framing bug that had been quietly corrupting paused recordings in *every* open implementation. That fix is now upstream in the shared codec and submitted to FFmpeg, so it reaches far beyond this repo.

If any of that sounds interesting, the story is in **[THE-STORY.md](THE-STORY.md)** (ten minutes, no code) and the full technical account is in **[the paper](THE-DSS-PAPER.md)**.

If it does not, that is completely fine. You came for your file, and you have it.
