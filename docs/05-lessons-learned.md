# 05 — Lessons learned

> The bugs we ate, the gotchas that bit us, and the operational quirks worth knowing if you do this in your own pipeline. Honest, in chronological order.

Every "we put X in production" story is a lot more useful when it includes the parts that went sideways. So here they are, the way they actually happened.

---

## Lesson 1 — PHP 5.6 doesn't have `??`

The admin web UI is PHP. The backend stack we're integrating with is on PHP 5.6 (don't ask). I wrote the admin page in modern PHP idioms without thinking, including a few null-coalescing operators:

```php
$msg = "Erreur : " . safe($j['error'] ?? 'inconnue');
```

`??` is PHP 7+. On PHP 5.6 it's a parse error. The page returned **HTTP 500** the moment it was deployed — no syntax checking caught it because I'd done the SCP-then-mv-without-lint sequence (the discipline I knew I was supposed to follow, the one I skipped on the first try because the page was new and "nobody was looking yet").

The fix is mechanical:

```php
// Before (PHP 7+)
$err = $j['error'] ?? 'inconnue';

// After (PHP 5.6 compatible)
$err = isset($j['error']) ? $j['error'] : 'inconnue';
```

The lesson is procedural, not technical. **Always run `php -l` on the `.tmp` file before the atomic `mv`.** If `php -l` fails, you don't move. I knew this. I skipped it. It cost me a `git diff` of shame.

Build this into the deploy script and you can't make this mistake:

```bash
scp file.php server:/path/file.php.tmp
ssh server "php -l /path/file.php.tmp || exit 1"
ssh server "mv /path/file.php.tmp /path/file.php"
```

The `|| exit 1` is the part you can't forget.

## Lesson 2 — Windows OpenSSH `scp -r src/. dest/` is not POSIX

On Linux/macOS, `scp -r src/. dest/` copies the *contents* of `src/` into `dest/`, merging into existing subdirectories. It's the idiom for "copy everything from here to there".

On Windows OpenSSH (the version that ships with Windows 10/11), the same command sometimes creates a `dest/src/` subdirectory instead of merging. It depends on whether the destination structure exists and on small differences in how the path is parsed.

The first time I deployed the project from my Windows workstation to the Linux server, all the files ended up under `/home/guillain/conv-dss-ds2-to-mp3/madac-build/...` instead of `/home/guillain/conv-dss-ds2-to-mp3/...`. The cron immediately complained that it couldn't find `lib/core.mjs`.

Two robust workarounds:

```bash
# Option A — explicit "merge into" on the receiving side
scp -r local-src/. server:/dest/.tmp/
ssh server "cp -r /dest/.tmp/. /dest/ && rm -rf /dest/.tmp"

# Option B — tar pipe (works everywhere)
tar c -C local-src . | ssh server "tar x -C /dest"
```

The `tar` pipe is the more portable and predictable choice for any non-trivial directory transfer. The `scp -r` shortcut is fine for single files and small flat folders, but for nested project structures, it's a footgun.

## Lesson 3 — The "feed amont" was already running, we just didn't know

When we built the cron, we assumed (based on grepping the existing scripts) that the legacy "feed" — the script that copies DS2 files from the email intake into the `audio_aconv/` queue — had stopped working. We'd seen 22 files stuck in the queue since the previous Sunday, and zero new ones since. We concluded the feed was dead.

So the cron's "mode 2" (scan `mail/` directly) was designed as the *replacement* for the dead feed. It works on its own, doesn't need anything upstream.

Two days into production, the log showed something weird:

```
[2026-05-25 10:09:05] [2940793 feed] OK -> BPACA_21_05.mp3 (conv 4s)
[2026-05-25 10:11:05] [2940793 aconv] OK -> BPACA_21_05.mp3 (conv 4s)
```

Same file, two conversions, two minutes apart. The cron's mode 2 (scan `mail/`) picked it up. Then mode 1 (consume `audio_aconv/`) picked it up again. Which means *something* had copied the file into `audio_aconv/` after the fact.

Hunting it down: `grep -rIE 'audio_aconv' /home/madactylo/` turned up a 11 KB bash script (`conv_fast.sh`) that we'd missed in the initial sweep. It was called by the email intake cron (`extract_emails.sh`, runs every 3 minutes), it had been modified two weeks earlier — and it had been running the entire time. The 5-day quiet period we'd seen earlier wasn't because the feed was dead; it was because of an unrelated event (a brief intake hiccup, files queued up elsewhere) that we'd mis-diagnosed as "the feed is dead".

The fix for the duplicate is a 5-line idempotency check in our cron's mode 1: if the destination MP3 already exists and is less than 10 minutes old, treat the `audio_aconv/` copy as a duplicate, just delete it and skip the conversion. Now mode 1 and mode 2 coexist gracefully.

The meta-lesson: **when you think a system component is dead, look harder before building its replacement**. It's much easier to add a second thing alongside a first thing than to figure out why the first thing was doing nothing.

## Lesson 4 — Magic bytes are still the best signal

Detecting whether a DS2 file is encrypted *without* attempting to decode it (and waiting for an error) is most reliable via the first 4 bytes:

```
\x03ds2  — plain DS2
\x03dss  — plain DSS (older format)
\x03enc  — encrypted DS2
```

This works because Olympus consistently writes these magic bytes regardless of encoder version, recorder model, or firmware. In our 35-file dataset, every file's first 4 bytes matched one of these three signatures exactly.

The alternative — let `dss-decode-native --info` try to read the file and parse its output — works for plain files but is fragile for encrypted ones (the binary errors out with a message that doesn't always parse cleanly). 4 bytes from disk + a `case` statement in bash is faster, more deterministic, and harder to break.

If you find yourself writing complex parser logic to detect a file format, **check the magic bytes first**. They were put there for exactly this reason.

## Lesson 5 — `nocaseglob` doesn't propagate to subshells

In the bash cron, we needed to match `*.ds2` and `*.DS2` (clients are inconsistent about file extension case). The clean way to do this in bash is:

```bash
shopt -s nocaseglob
files=( "$DIR"/*.ds2 )   # matches *.ds2 AND *.DS2 AND *.Ds2 etc.
```

What we initially wrote (because we wanted a timeout around the glob):

```bash
shopt -s nocaseglob
f=$(timeout 25 bash -c "ls $DIR/*.ds2 2>/dev/null | head -1")
```

The `bash -c "..."` spawns a **new shell**. The `shopt -s nocaseglob` from the outer shell does not propagate. The inner `ls *.ds2` is case-sensitive. It matches only files named exactly `*.ds2`, missing every `*.DS2`. We spent half an hour wondering why the cron was returning "no files found" when we could clearly see `*.DS2` files in the directory.

Two fixes:

```bash
# Option A: set nocaseglob inside the subshell explicitly
f=$(timeout 25 bash -c 'shopt -s nocaseglob; ls "$1"/*.ds2 2>/dev/null | head -1' _ "$DIR")

# Option B: don't subshell, glob in the parent
shopt -s nocaseglob nullglob
files=( "$DIR"/*.ds2 "$DIR"/*.dss )
```

Option B is what we ended up with. Cleaner and avoids the subshell trap entirely.

## Lesson 6 — `node --check` rejects `.tmp` extensions

When we updated the JS module, the deploy script did the standard:

```bash
scp lib/core.mjs server:/path/lib/core.mjs.tmp
ssh server "node --check /path/lib/core.mjs.tmp && mv /path/lib/core.mjs.tmp /path/lib/core.mjs"
```

`node --check` failed with:

```
TypeError [ERR_UNKNOWN_FILE_EXTENSION]: Unknown file extension ".tmp"
```

Modern Node (>= 18) is strict about ESM file extensions — it wants `.mjs` or `.cjs` to decide which module system to use, and won't even *parse* a file with an arbitrary extension. Our `.tmp` suffix for the atomic rename trick (well-known Linux practice) confused it.

Three workarounds, in order of preference:

```bash
# Option A: name the temp file with the right extension
scp lib/core.mjs server:/path/lib/core.mjs.new
ssh server "node --check /path/lib/core.mjs.new && mv /path/lib/core.mjs.new /path/lib/core.mjs"

# Option B: feed via stdin
ssh server "node --check < /path/lib/core.mjs.tmp"

# Option C: do the lint pre-deploy, locally
node --check lib/core.mjs && scp lib/core.mjs server:/path/lib/core.mjs.tmp
ssh server "mv /path/lib/core.mjs.tmp /path/lib/core.mjs"
```

The Linux practice of "deploy as `.tmp` then atomic-rename" predates Node's ESM strictness. Adjust the practice for Node files: use `.new` or `.staging` suffix.

## Lesson 7 — Watch what the cron *doesn't* do

The cron logs every pass:

```
[2026-05-25 10:09:01] === audio_cron pass (dryrun=0) ===
[2026-05-25 10:09:01]   audio_aconv empty
[2026-05-25 10:09:01]   feed[ok=0 noop=40] === pass done
```

For days at a time, the log is full of `noop` lines. Nothing happening. That's the **expected** state — the cron should be silent when there's nothing to do.

A natural impulse, when reviewing logs, is to scroll past the `noop`s as boring. Don't. The presence of regular `noop` lines is what tells you the cron is alive. The day you stop seeing them is the day to investigate — the cron is jammed, or the database query is failing silently, or systemd has stopped firing it.

We added a Nagios-style heartbeat check: alert if the most recent `=== audio_cron pass` log line is more than 5 minutes old. It's the smallest possible "is this thing alive" signal, and it would have caught the historical Switch.exe failures we couldn't catch before.

## Lesson 8 — Document the rollback before you need it

For every component deployed, we wrote down the rollback command **in the same commit** as the deploy step. Not later, not "we'll document it after it's stable". Then and there.

This sounds obvious. It almost never happens in practice. The moment you actually need the rollback (because something is on fire in production), is the worst possible moment to be reading code to figure out how to undo a change. The 30 seconds it takes to write `# To roll back: systemctl stop audio-convert && systemctl disable audio-convert` saves you ten minutes of panic later.

Every script in this project's `src/` directory has a rollback header comment. Every `systemctl enable` command has its `systemctl disable` counterpart documented. Every cron entry has the `rm /etc/cron.d/...` line written down.

When we did need to roll something back (we changed the WASM chain to native — see [04](04-wasm-vs-native.md)), the rollback path took 60 seconds to execute and zero seconds to figure out. Document the way out before you walk in.

## Lesson 9 — Validation campaign chronology > size

We picked 35 files for validation, not because 35 was a magic number, but because that's how many recent files we could find that met all our criteria: in active commands, on disk, with comparable Switch-produced outputs to A/B against. We didn't want to lower the bar.

A more sophisticated team would have done a 500-file campaign. We didn't have the budget for that. What we *did* do is pick files from **multiple weeks**, from **multiple clients**, from **multiple recorder models** (inferred from file metadata). That spread mattered more than the count.

If you're tempted to "just test on the 5 files you have lying around" — do better. The decoder might handle those 5 fine and choke on the 6th in a way you wouldn't notice until production. A mix-by-date and mix-by-source sample of 20-30 files is far more informative than 200 files from one source.

## Lesson 10 — Honesty in the bilan

Throughout the project we wrote up "synthesis" reports for the stakeholder after each step. Two-sentence summaries: what we did, what worked, what didn't. *Including* the things that didn't work.

The instinct to gloss over the mistakes ("we deployed, all good, moving on") is strong. Resist it. The PHP 5.6 `??` mistake described in Lesson 1 went into the synthesis report the same day, with the same plain language:

> "First deploy of the page: I'd used `??` (PHP 7+) which is incompatible with PHP 5.6 — the lint failed but I'd already `mv`ed without checking. The page returned 500 for a few seconds. Fixed (`isset(...) ? ... : ...`), redeployed with proper lint-before-mv this time. No production impact (the page was new and nobody was using it yet)."

That paragraph is more useful than a hundred "everything is great" reports. It tells the stakeholder you saw the mistake, you understood it, you fixed it, you adjusted the process to not repeat it. It also tells them they can trust the rest of your reports — because if a small mistake got reported, you're not hiding the big ones.

The five-year version of this project should still have this paragraph in the changelog. It's part of the story.

---

There are three more chapters, written later — after bugs we didn't see coming surfaced in production months in. They're the most detailed debugging walkthroughs in this repo, and the most reusable: [06 — The empty-block bug](06-the-empty-block-bug.md), its sequel [07 — Cracking the re-sync block](07-cracking-the-resync-block.md) (where we wiretapped the real Olympus decoder to read the format's last undocumented rule), and the open handoff [08 — The decoder black hole](08-the-decoder-black-hole.md). Otherwise, back to [the README](../README.md) for the source tree, the install steps, and the next steps.

---

*The bugs we got to keep. 🐛*
