# Security policy

## Supported versions

Latest `main` and the most recent tagged release receive security fixes. Older releases do not.

## Reporting a vulnerability

If you find a security issue in this project (the integration code, the install script, the daemon, the web UI) — **please do not open a public GitHub issue**. Instead:

1. Email the maintainer directly: open a GitHub issue saying *"I'd like to report a security issue privately, please contact me"* with no details, and we'll exchange a private channel.
2. Or use [GitHub's private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability) directly on this repo.

We'll acknowledge within 7 days, investigate, and credit you in the fix's release notes unless you prefer to stay anonymous.

## Out of scope

This project depends on upstream codecs and tools (`dss-codec`, `lamejs`, `ffmpeg`). Vulnerabilities in those should be reported upstream:

- `dss-codec` / `dss-codec-wasm` → <https://github.com/hirparak/dss-codec/issues>
- `lamejs` → <https://github.com/breezystack/lamejs/issues>
- `ffmpeg` → <https://ffmpeg.org/security.html>

## Threat model

This project is designed to run on a **trusted Linux host** processing audio files from your own pipeline. Specifically:

- The HTTP daemon listens on `127.0.0.1` only — never expose it directly to the internet.
- The admin web UI assumes you've placed it behind your existing authentication layer.
- Encrypted DS2 password handling: passwords are passed via URL query parameter to the local daemon, then via subprocess argv to the binary. Both stay on the local machine, but `127.0.0.1` HTTP request URLs may end up in access logs — keep that in mind if logging is verbose.

If your use case differs (multi-tenant, internet-facing, untrusted inputs), please review the threat model before deploying.
