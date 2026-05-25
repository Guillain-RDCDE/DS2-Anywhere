# Sending the patch — manual copy-paste procedure

> Why manual? Because `git send-email` requires SMTP credentials set up
> locally, and we chose to keep the email going out from the submitter's
> normal mail client. This folder contains the patch in three forms so
> the copy-paste is foolproof.

## Files in this folder

| File | What it is |
|---|---|
| [`0001-avcodec-avformat-add-Olympus-DS2-decoder-and-demuxer.patch`](0001-avcodec-avformat-add-Olympus-DS2-decoder-and-demuxer.patch) | The raw `git format-patch` artefact. mbox-style, includes `From:`/`Date:`/`Subject:` headers. If you ever switch to `git send-email`, this is the file you pass to it. |
| [`email-subject.txt`](email-subject.txt) | Single line — paste verbatim into the `Subject:` field of your mail client. |
| [`email-body.txt`](email-body.txt) | The patch with the email headers stripped — paste verbatim into the body of your mail client. **This is what reviewers will see.** |

## How the patch was built

```bash
# On a clean FFmpeg master clone (HEAD = 69bdb05, 2026-05-25):
git apply ../domack-v1.patch
git add -A
git commit -s \
  --author="Patrick Domack <patrickdk77@users.noreply.github.com>" \
  -F /path/to/commit-message.txt
git format-patch -1 -o patches/
```

Result:
- **Author:** `Patrick Domack <patrickdk77@users.noreply.github.com>` (GitHub noreply convention, since Patrick has no public email)
- **Signed-off-by:** `Guillain d'Erceville <guillain@poulpe.us>` (the submitter, certifying the DCO)
- **md5:** `faaf6467d966f16ec89c51811cb5daff`
- **size:** 65,467 bytes

## Verification (already done — see `02-validation` below)

```
# Fresh clone, fresh apply, fresh build:
git clone <ffmpeg> /tmp/fresh && cd /tmp/fresh
git reset --hard 69bdb05
git am 0001-avcodec-avformat-add-Olympus-DS2-decoder-and-demuxer.patch  # → applies cleanly
./configure --enable-decoder=ds2 --enable-demuxer=ds2 ...
make ffmpeg  # → builds cleanly

# Decode the FATE sample, compare against the reference:
./ffmpeg -i fate/sample-qp.ds2 -f framecrc /tmp/regen.framecrc
diff /tmp/regen.framecrc ../fate/fate-ds2-qp.ref  # → byte-identical
```

Both md5 = `43cb8828c12b7482474aef4481a59f5d`.

## Sending procedure

### Recommended client: Thunderbird

Thunderbird is the easiest for mailing-list patches because it can be
configured to send strictly plain-text without any reformatting.

1. **One-time setup**:
   - Account Settings → Composition & Addressing → uncheck "Compose
     messages in HTML format"
   - Tools → Options → Advanced → Config Editor → set
     `mail.compose.default_to_paragraph` to `false`
   - Set `mailnews.send_plaintext_flowed` to `false`

2. **Compose the mail**:
   - To: `ffmpeg-devel@ffmpeg.org`
   - Subject: paste contents of `email-subject.txt`
   - Body: paste contents of `email-body.txt` **as-is**, no edits
   - Send

3. **Verify before sending**: open the "Recipients" → "Show full
   headers" and confirm `Content-Type: text/plain; charset=UTF-8` (NOT
   `multipart/...`, NOT `text/html`).

### Alternative: Gmail web

Gmail can work but it's tricky. The body must NOT be reflowed.

1. Compose a new email.
2. Click the three dots at the bottom → "Plain text mode" (toggle ON).
3. To: `ffmpeg-devel@ffmpeg.org`
4. Subject: paste `email-subject.txt`.
5. Body: paste `email-body.txt`.
6. **CRITICAL**: do NOT let Gmail "fix" anything. If it offers to
   "send as HTML", refuse. If it underlines URLs, that's a sign it's
   not in plain text mode.
7. Send.

**Risk with Gmail**: it sometimes wraps long lines silently. If
reviewers complain about broken patch hunks, switch to Thunderbird
for the next iteration.

### Alternative: `git send-email` (if you change your mind)

If at any point you decide to switch to `git send-email`, the .patch
file is ready:

```bash
git config --global sendemail.smtpserver smtp.gmail.com
git config --global sendemail.smtpuser <your-email>
git config --global sendemail.smtpencryption tls
git config --global sendemail.smtpserverport 587

git send-email --to=ffmpeg-devel@ffmpeg.org \
  0001-avcodec-avformat-add-Olympus-DS2-decoder-and-demuxer.patch
```

(You'll need a Gmail App Password since Google deprecated plain SMTP
auth — generate at `https://myaccount.google.com/apppasswords`.)

## After sending

1. **Confirm the mail arrived**: subscribe to `ffmpeg-devel` for a few
   weeks (https://lists.ffmpeg.org/mailman/listinfo/ffmpeg-devel) and
   check the archive at https://ffmpeg.org/pipermail/ffmpeg-devel/.
2. **Patience**: review cadence is days to weeks. No bumping before 7
   days have elapsed.
3. **If reviewers reply**: respond inline (quote with `>`, reply below
   the quote), one topic per thread.
4. **If they request changes**: prepare v2 in a fresh thread with
   subject `[PATCH v2] avcodec, avformat: add Olympus DS2 decoder and
   demuxer`. Reset the FFmpeg clone, redo the work, regenerate the
   .patch from this folder's procedure.

## Common review feedback to anticipate

Based on reading recent FFmpeg patch threads, expect to be asked
about:

- **The `×` (U+00D7) characters in source comments** (4 occurrences,
  in CELP subframe size descriptions like `4×72-sample`). Some
  reviewers will ask for ASCII (`4x72-sample`). Easy fix in v2.
- **`MAINTAINERS` entry**: the patch leaves the new files unowned.
  Reviewers may push for an owner. See
  [`../02-changelog-and-doc.md`](../02-changelog-and-doc.md) §
  MAINTAINERS for the prepared response.
- **FATE sample license**: the sample comes from a public vendor test
  page without explicit CC0. Reviewers may want explicit grant from
  dictate.com.au. See [`../01-fate-sample-plan.md`](../01-fate-sample-plan.md)
  § Licensing position for the prepared response.
- **`Author:` without real email**: GitHub noreply is conventional
  but some FFmpeg reviewers prefer real emails. If asked, Patrick may
  agree to a real email; if not, the noreply is defensible.
- **Codec tables, doc/general_contents.texi format**: see
  [`../02-changelog-and-doc.md`](../02-changelog-and-doc.md) for the
  prepared additions.
