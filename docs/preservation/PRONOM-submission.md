# PRONOM submission — Grundig DSS-SP (PH9607)

Material to register the **Grundig DSS-SP** dictation format with
[PRONOM](https://www.nationalarchives.gov.uk/PRONOM/), The National Archives (UK)
file-format registry, so that DROID/Siegfried and digital-preservation tools can
**identify** these files. Identification (knowing *what* a file is) is the first
step archives need; this codec is currently unidentifiable by any registry.

> **How to submit (current process, 2026).** The preferred route is GitHub:
> open a pull request on [`digital-preservation/PRONOM_Research`](https://github.com/digital-preservation/PRONOM_Research)
> adding a folder under `Submissions/`. A ready-to-copy folder is provided here:
> [`pronom-submission/Grundig DSS-SP/`](pronom-submission/) (a DROID signature XML
> + a Readme). Add a sample `.dss` under `Samples/` and open the PR.
> Lighter alternatives: open an Issue on that repo, post on the
> [PRONOM Google Group](https://groups.google.com/g/pronom), use the
> [web submission form](https://www.nationalarchives.gov.uk/contact-us/submit-information-for-pronom/pronom-request-form/),
> or email `pronom@nationalarchives.gov.uk` (best for private samples).
> Contributions are licensed OGL 2.0 / CC0. This document is the human-readable
> rationale; the machine-readable signature is in the folder above.

---

## Proposed format record

| Field | Value |
|---|---|
| **Name** | Grundig Digital Speech Standard – Standard Play |
| **Version** | SP (internal codec tag *PH9607*) |
| **Aliases** | Grundig DSS-SP; Digta DSS (SP); PH9607 |
| **Classification** | Audio |
| **Extension(s)** | `dss` |
| **Developer / vendor** | Grundig Business Systems |
| **Released** | DSS family from 1994; this SP variant on Digta recorders (e.g. Digta 415) |
| **Description** | Proprietary low-bitrate speech codec for handheld professional dictation recorders. A CELP coder synthesising 12 kHz PCM, resampled 3:4 to 16 kHz mono. Container: a header sized `byte[0] × 512`, followed by 512-byte audio blocks carrying a continuous MSB-first bitstream of 328-bit CELP frames. **Distinct from Olympus DSS** despite the shared extension and `dss` magic. |
| **Reference documentation** | Open specification + bit-exact reference decoder: <https://github.com/Guillain-RDCDE/DS2-Anywhere> (see `docs/SPEC-grundig-dss-sp.md`) |

### Relationships
- **Is supertype-of / related-to** existing PRONOM Olympus DSS records: *same
  extension and `…dss` magic, different codec.* A correct registry must
  disambiguate on the **first byte** (header-block count) and codec tag, not the
  `dss` string alone.
- **Has lower priority than** an internal-signature match (below): extension `dss`
  alone is ambiguous across vendors.

## Binary signatures

PRONOM "internal signatures" are byte sequences at file offsets. The
discriminating bytes for this format:

**Primary signature (BOF, offset 0):**

```
Hex:        06 64 73 73
ASCII:      .  d  s  s
Offset:     0 (Beginning of File, absolute)
```

`byte[0] = 0x06` is the header size in 512-byte blocks (= a 3072-byte header,
the value observed on all Grundig SP samples); `64 73 73` = `dss`.

PRONOM byte-sequence form (BOFoffset 0):

```
06647373
```

**Block-marker corroboration (offset = header_size + 3):** every 512-byte audio
block begins with a 6-byte header whose bytes 3–5 are the fixed marker:

```
Hex:   FF 00 FF   at offset (byte[0]*512)+3, then repeating every 512 bytes
```

This can be used as a secondary/confirming signature where supported (variable
offset from a value read at byte 0).

**Disambiguation from neighbours** (first four bytes):

| First 4 bytes | Format |
|---|---|
| `06 64 73 73` | **Grundig DSS-SP (this record)** |
| `02/03 64 73 73` | Olympus DSS family |
| `03 64 73 32` | Olympus DS2 |
| `03 65 6e 63` | Encrypted DS2/DSS |

## Supporting evidence

- **Bit-exact open decoder** verified byte-for-byte against the vendor decoder
  (Grundig `dss2wav.dll`) on every available sample (max abs sample diff = 0,
  correlation = 1.0). Three independent implementations agree (Python, C/FFmpeg,
  Rust).
- **Samples** (intelligible decoded speech confirms correct identification):
  Digta 415 *"This is a test."*; Grundig DigtaSoft demo *welcome*/*willkommen*.
  Provided on request / attached to the submission.
- **Full technical specification:** [`docs/SPEC-grundig-dss-sp.md`](../SPEC-grundig-dss-sp.md).
- **Provenance:** reverse-engineered clean-room; the codec was previously
  unreadable by all open *and commercial* third-party tools.

## Open items before/with submission
- Pin the **exact header offset of the `PH9607` / codec tag** inside the 3072-byte
  header for a stronger, header-anchored internal signature (currently the BOF
  `06 64 73 73` + the `FF 00 FF` block marker are used). The vendor library
  exposes `CDssHeader1..7` structures; the tag offset can be read from a sample.
- Confirm whether Grundig **QP** and **TrueSpeech** variants warrant their own
  records (QP overlaps the Olympus DS2 CELP; TrueSpeech is `CDssTrueSpeechCodec`).

---

*Prepared by the DS2-Anywhere project. Free to use by The National Archives and any
preservation registry. No vendor code is reproduced; only observable format
behaviour and identifying byte patterns are described.*
