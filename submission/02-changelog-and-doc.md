# Changelog + doc entries

> The mechanical bits that go in the patch alongside the code.

## `Changelog`

Add under the next "version <NEXT>" section, in the "audio decoders" subsection (creating subsections as needed if absent in the current draft):

```
- Olympus Digital Speech Standard (DSS) and DSS Pro (DS2) decoder and demuxer
```

That's it. One line.

## `doc/general_contents.texi`

Find the audio decoders table (`@section Audio Decoders`) and add an entry in alphabetical position:

```texi
@item DS2 (Digital Speech Standard Pro) @tab @tab X
@item DSS (Digital Speech Standard) @tab @tab X
```

(`X` in the third column = supported for decoding only, no encoder.)

If a DSS entry already exists for the existing partial `dss_sp` support in FFmpeg, update that row to clarify both SP and Pro variants are now covered.

## `MAINTAINERS`

Suggested: **leave unowned**. Patrick Domack explicitly opted out of FFmpeg mailing-list interactions; we don't want to volunteer him as the listed maintainer. The author of this submission isn't an FFmpeg regular either.

If reviewers insist on having a name, an option is to add a line under "Reviewers" rather than "Maintainers", with a note that the author handles questions but not commits.

## File headers (already in the patch)

The patch's `libavcodec/ds2.c` and `libavformat/ds2.c` should keep their existing GPL/LGPL-style FFmpeg-conventional header. Patrick's authorship line goes inside:

```c
/*
 * Digital Speech Standard Pro (DS2) audio decoder
 *
 * Copyright (c) 2026 Patrick Domack <patrickdk77 (github)>
 * Based on the codec specification reverse-engineered by Kieran Hirpara
 *   (https://github.com/hirparak/dss-codec, MIT, February 2026)
 * Submitted to FFmpeg by Guillain d'Erceville (DS2-Anywhere project, May 2026)
 *
 * This file is part of FFmpeg.
 *
 * FFmpeg is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * (... standard LGPL boilerplate ...)
 */
```

The three-name credit reflects the actual chain of work: spec → C port → submission.
