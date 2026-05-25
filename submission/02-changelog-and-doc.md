# Changelog + doc entries

> The mechanical bits that go in the patch alongside the code.

## `Changelog`

Add under the next "version <NEXT>" section, in the "audio decoders"
subsection (creating the subsection as needed if absent in the current
draft):

```
- Olympus Digital Speech Standard Pro (DS2, SP and QP modes) decoder
  and demuxer
```

That's it. One entry.

The existing `dss_sp` decoder is untouched, so no Changelog entry for
DSS itself.

## `doc/general_contents.texi`

Find the audio decoders table (`@section Audio Decoders`) and add an
entry in alphabetical position:

```texi
@item DS2 (Digital Speech Standard Pro) @tab @tab X
```

(`X` in the third column = supported for decoding only, no encoder.)

The existing `DSS Standard Play` row stays as-is — Rempel's
`dss_sp.c` decoder still handles that container. Don't duplicate or
merge them.

Before patching: verify the exact format of the table by reading the
current `doc/general_contents.texi`. The example above assumes a
3-column layout (`@item NAME @tab encoder @tab decoder`); if the
current table has more columns, mirror them.

## `MAINTAINERS`

Recommendation: **leave the new files unowned**. Patrick Domack has
asked to stay off the `ffmpeg-devel` mailing list, so we don't list
him as the maintainer of record. The submitter is not an FFmpeg
regular either and would rather not list a name that won't be
responsive to follow-up patches in the long term.

If reviewers insist on a name being on file, the lightest-touch option
is to ask Patrick (off-list) whether he's comfortable being added as
`R:` (Reviewer) rather than `M:` (Maintainer). The actual FFmpeg
`MAINTAINERS` syntax for that distinction is to be confirmed against
the current file before proposing it — FFmpeg uses Linux-kernel-style
conventions but not always the same fields.

## File headers (already in the patch)

The patch's `libavcodec/ds2.c` and `libavformat/ds2.c` carry the
standard FFmpeg LGPL header, with the same attribution block on both
files, only the one-line description changing (`decoder` vs `demuxer`).

### `libavcodec/ds2.c`

```c
/*
 * Digital Speech Standard Pro (DS2) audio decoder
 *
 * Copyright (c) 2026 Patrick Domack
 *
 * Based on a codec specification reverse-engineered from the Olympus
 * DLLs (DssDecoder.dll, dss32.dll) via Ghidra by Kieran Hirpara
 * (https://github.com/hirparak/dss-codec, MIT, 2026). The CELP
 * algorithm in this file was implemented from the specification
 * text in FFmpeg trac #6091; the quantization tables (reflection
 * codebooks, pitch and excitation gains, pulse amplitudes) are
 * sourced from Hirpara's reference implementation.
 *
 * Submitted to FFmpeg by Guillain d'Erceville on behalf of Patrick
 * Domack (DS2-Anywhere project,
 * https://github.com/Guillain-RDCDE/DS2-Anywhere, 2026).
 *
 * This file is part of FFmpeg.
 *
 * FFmpeg is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * FFmpeg is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with FFmpeg; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA
 * 02110-1301 USA
 */
```

### `libavformat/ds2.c`

Identical block as above, with the first line changed:

```c
 * Digital Speech Standard Pro (DS2) demuxer
```

(Rest of the header identical; same Copyright, same attribution,
same LGPL boilerplate. Both files declare the same provenance and
license terms.)

### Notes on the header

- `Patrick Domack` with no email — Patrick has not published a public
  contact email; using a GitHub handle in a Copyright line is
  non-conventional in FFmpeg. A plain name without email is accepted
  (other libavcodec files do this).
- Copyright year is `2026` because Patrick's gist (his first public
  contribution of this code) was posted in 2026-03. If reviewers
  surface an earlier date for the work itself (private development
  before the gist), it can be widened to `2025-2026` — to confirm
  with Patrick if asked.
- The reverse-engineering credit names Hirpara explicitly and points
  to the upstream MIT-licensed repo — this is the chain of provenance
  the LGPL boilerplate alone doesn't capture.
- The submission-on-behalf line names the project so reviewers have a
  single URL to follow back for context.
