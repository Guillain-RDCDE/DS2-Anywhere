---
name: Bug report
about: Something doesn't work as documented
title: ''
labels: bug
assignees: ''
---

## What happened

A clear description of what went wrong. Include the exact command you ran or the exact action in the UI.

## What you expected

What you expected to happen instead.

## To reproduce

Step-by-step (minimal). If your bug is reproducible only with a specific DS2 file, can you share a minimal one (e.g. a short test recording)? If your file is sensitive (legal/medical), please don't share it — just describe it.

## Environment

- OS + version :
- `conv-dss-ds2-to-mp3 --inspect` on the affected file output :
- `dss-decode-native --version` output (if you have a release tagged build) :
- ffmpeg version (`ffmpeg -version` first line) :
- Node version (`node -v`) :
- Where you got the project from : git clone / Docker / install.sh / other

## Logs

If relevant, paste the last few lines from `/var/log/audio_converter.log` or `journalctl -u audio-convert -n 50`.

```
(paste here)
```

## Anything else

Workarounds you've tried, related issues, ideas for the fix, etc.
