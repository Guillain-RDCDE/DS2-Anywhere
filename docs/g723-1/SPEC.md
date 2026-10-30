# ITU-T G.723.1 decoder (main body + Annex A) — implementation specification

Version 1, 2026-10-30.

Purpose: this document, together with the machine-readable tables in `tables/`, is the
complete description needed to write a bit-exact G.723.1 speech decoder (both rates,
Annex A comfort-noise / SID / untransmitted frames, frame-erasure concealment, postfilter
on or off). It was derived from the ITU fixed-point reference (version 5.2, June 2006,
main body + Annex A) and from Recommendation G.723.1 (05/2006). Where the Recommendation's
prose and the reference behaviour differ, this document describes the reference behaviour,
because bit-exactness against the ITU test vectors depends on it (the Recommendation itself,
clause 1.5, says the fixed-point description takes precedence). Every such divergence is
flagged with **[DIFFERS FROM PROSE]**.

Out of scope: encoder, VAD, encoder-side CNG, Annex B (floating point), Annex C (channel
coding).

Reading order: section 0 (notation) and section 1 (arithmetic) first, they are used
everywhere. Section 4 is the algorithm. Section 7 lists the traps that most often break
bit-exactness.

Validation: the decoder in DS2-Anywhere (`vendor/dss-codec/src/codec/g723_1.rs`) was
written from this document, its tables and the Recommendation's text, by someone who
never saw the reference source or any other G.723.1 implementation. It reproduces,
sample for sample, all 11 official decoder test vectors and the 17 extra cases of
section 6.6 (`tests/g723_1_vectors.rs`). A clean-room provenance note is in
[`../19-the-last-format.md`](../19-the-last-format.md). Any mismatch in a new
implementation is therefore most likely an implementation slip; section 7 lists where to
look first.

---------------------------------------------------------------------------------------

## 0. Notation and conventions

* **int16** = two's-complement signed 16-bit integer, range [-32768, 32767].
  **int32** = two's-complement signed 32-bit integer, range [-2^31, 2^31-1].
  Every stored quantity in this decoder is an int16 or an int32; the width of each
  intermediate is given explicitly.
* All arithmetic below is exact integer arithmetic on mathematical integers *unless* a named
  primitive from section 1 is used. When a primitive is named, its exact semantics
  (saturation, rounding, flooring) apply. When this document writes a plain `+`, `-`, `*`
  on small values it states that no overflow is possible.
* `floor(x)` is the mathematical floor. `x >> n` on a signed value always means
  arithmetic shift = `floor(x / 2^n)` (never truncation toward zero).
* `x mod L` for x >= 0, L > 0 is the usual non-negative remainder.
* Arrays are 0-indexed. "Subframe i" is i = 0..3, each 60 samples; a frame is 240 samples.
* "Sequential accumulation" means: start from the stated initial value, then apply the
  stated primitive once per term **in the stated index order**, the output of each step
  being the input of the next. Because the primitives saturate, the order matters and must
  be respected exactly.
* Hexadecimal constants are written 0x....; when used as int16 they denote the
  two's-complement value (e.g. 0xc000 = -16384, 0xfffc = -4).
* Names of tables refer to files in `tables/` (e.g. table `fixed_gain` =
  `tables/fixed_gain.csv`). Tables of more than one column are row-major:
  `T[r][c]`.

Constants used throughout:

| name | value | meaning |
|---|---|---|
| FRAME | 240 | samples per frame |
| SUB | 60 | samples per subframe |
| ORDER | 10 | LPC order |
| HIST | 145 | length of the excitation history (= maximum pitch lag 18+127) |
| LAGMIN | 18 | smallest coded pitch lag |

---------------------------------------------------------------------------------------

## 1. Arithmetic model

All primitives are pure functions of their arguments. The reference also keeps a global
"overflow" flag and a global "carry" flag; **the decoder never reads either of them**, and
the only primitives that read or write the carry flag are not used by the decoder. An
implementation can therefore ignore both flags entirely.

Helpers (not primitives, used to define them):
* `sat16(v)` = 32767 if v > 32767, -32768 if v < -32768, else v.
* `sat32(v)` = 2^31-1 if v > 2^31-1, -2^31 if v < -2^31, else v.

### 1.1 16-bit primitives (inputs int16, output int16)

| primitive | definition | edge cases |
|---|---|---|
| `add16(a,b)` | sat16(a+b) | |
| `sub16(a,b)` | sat16(a-b) | |
| `neg16(a)` | -a, except neg16(-32768) = 32767 | |
| `abs16(a)` | abs(a), except abs16(-32768) = 32767 | |
| `mul16(a,b)` | sat16(floor(a*b / 2^15)) | only (-32768)*(-32768) saturates: result 32767. Note floor: mul16(-1, 1) = -1, mul16(3, -16384) = -2 |
| `mul16r(a,b)` | sat16(floor((a*b + 2^14) / 2^15)) | (-32768)*(-32768) gives 32767 |
| `shl16(a,n)` | if n < 0: shr16(a, -n). Else: if n > 15 and a != 0, or if a*2^n is outside int16: result is 32767 when a > 0, -32768 when a < 0; otherwise a*2^n. (a = 0 gives 0 for any n >= 0.) | saturating left shift |
| `shr16(a,n)` | if n < 0: shl16(a, -n). Else if n >= 15: (a < 0 ? -1 : 0). Else floor(a / 2^n) | arithmetic (flooring) right shift |
| `norm16(a)` | number of left shifts needed to normalise a: if a = 0 → 0; if a = -1 → 15; otherwise let b = a if a > 0, b = (bitwise NOT a) = -a-1 if a < 0; result = the number of times b must be doubled until b >= 16384 (i.e. 14 - floor(log2(b))) | norm16(16384)=0, norm16(1)=14, norm16(-32768)=0, norm16(-16385)=0, norm16(-16384)=1 |
| `div16(a,b)` | requires 0 <= a <= b and b > 0 (the reference aborts the program otherwise; the decoder never violates this). If a = 0 → 0; if a = b → 32767; else floor(a * 2^15 / b) | result in [0, 32767] |

### 1.2 32-bit and mixed primitives

| primitive | inputs → output | definition | edge cases |
|---|---|---|---|
| `lmul(a,b)` | int16,int16 → int32 | 2*a*b, except when a*b = 2^30 (only a = b = -32768) the result is 2^31-1 | |
| `add32(x,y)` | int32 → int32 | sat32(x+y) | |
| `sub32(x,y)` | int32 → int32 | sat32(x-y) | |
| `mac32(x,a,b)` | int32,int16,int16 → int32 | add32(x, lmul(a,b)) — note the product is saturated first (lmul), then the sum | |
| `msu32(x,a,b)` | | sub32(x, lmul(a,b)) | |
| `neg32(x)` | | -x, except neg32(-2^31) = 2^31-1 | |
| `abs32(x)` | | abs(x), except abs32(-2^31) = 2^31-1 | |
| `shl32(x,n)` | int32,int16 → int32 | if n <= 0: shr32(x, -n). Else sat32(x * 2^n) (any overflow saturates to the sign-appropriate extreme; x = 0 gives 0) | |
| `shr32(x,n)` | | if n < 0: shl32(x, -n). Else if n >= 31: (x < 0 ? -1 : 0). Else floor(x / 2^n) | arithmetic (flooring) shift |
| `hi16(x)` | int32 → int16 | floor(x / 2^16) (the upper 16 bits, no rounding, no saturation) | |
| `lo16(x)` | int32 → int16 | the lower 16 bits of x reinterpreted as two's-complement int16 (i.e. wrap-around, NOT saturation) | lo16(0x00018000) = -32768 |
| `dep_hi(a)` | int16 → int32 | a * 2^16 | |
| `dep_lo(a)` | int16 → int32 | a (sign-extended) | |
| `round16(x)` | int32 → int16 | hi16(add32(x, 32768)) | saturating add first: round16(2^31-1) = 32767; round16(-2^31) = -32768 |
| `norm32(x)` | int32 → int16 | if x = 0 → 0; if x = -1 → 31; otherwise b = x if x > 0 else (bitwise NOT x) = -x-1; result = number of doublings until b >= 2^30 (= 30 - floor(log2 b)) | |
| `mul32x16(x,v)` | int32,int16 → int32 | let lo = x AND 0xffff taken as an UNSIGNED value 0..65535, hi = hi16(x) (signed). t = floor(lo * v / 2^15) (exact: lo*v always fits in int32), result = mac32(t, v, hi) = add32(t, lmul(v, hi)) | approximates floor(x*v/2^15) with saturation; use exactly this decomposition |
| `div32x16(N,d)` | int32,int16 → int16 | requires N >= 0 and d > 0 (never violated in the decoder). If N >= d*2^16 → 32767. Otherwise floor(N / (2*d)) | proof sketch: the reference halves both operands and runs a 15-step restoring division; the closed form is exact |

### 1.3 Composite routines

**`isqrt(N)`** (int32 N → int16). Greedy bitwise square root:
r = 0; for k = 14 down to 1 (14 steps, step value s = 2^k, i.e. 16384, 8192, ..., 2):
if N >= lmul(add16(r, s), add16(r, s)) then r = r + s. Result r.
Equivalently: the largest r whose bit 0 is 0 and whose bits 1..14 are chosen greedily
from the top such that 2*r*r <= N. (No saturation can occur: r + s <= 32766.)
For N <= 7 the result is 0. Note the comparison is a signed 32-bit comparison; negative N
gives 0.

**`rand16(seed)`** (seed is an int16 state variable, updated in place; returns int16):
u = seed taken as unsigned 16-bit (0..65535); w = u*521 + 259 (exact, fits in int32);
new seed = lo16(w); return value = the same lo16(w).
There are two independent seeds in the decoder (section 3): the concealment seed and the
comfort-noise seed.

**`vnorm(v[0..n-1])`** (vector normalisation, in place, returns an int16 shift value):
1. m = max over k of abs16(v[k]) (start m = 0; abs16 maps -32768 to 32767).
2. z = norm16(m) (z = 0 when m = 0; z <= 14 otherwise).
3. M = `norm_multiplier`[z] = 2^z (entry 15 would be 32767 but is never reached).
4. For every k: v[k] = lo16(shr32(lmul(M, v[k]), 4)). (Mathematically v[k]*2^z/8,
   floored; it always fits.)
5. Return z - 3. (So an all-zero vector returns -3.)

**`energy_q(x[0..59])`** (subframe energy used by the gain-scaling unit, int32):
sequential accumulation from 0 of mac32(acc, y, y) with y = shr16(x[k], 2), k = 0..59.

---------------------------------------------------------------------------------------

## 2. Bitstream

### 2.1 Bit numbering

A frame is a sequence of octets. Bit number k of the frame (k = 0, 1, 2, ...) is bit
(k mod 8) of octet floor(k/8), where bit 0 is the least significant bit of the octet.
Fields are read in the order listed below; each field of width w occupies w consecutive
bit numbers and its **first** bit is its **least significant** bit:
value = sum over t = 0..w-1 of bit(start + t) * 2^t.

This matches clause 4 Tables 5, 6 and A.1 of the Recommendation (those tables list each
octet MSB-first; the field bits progress from LSB to MSB through increasing bit numbers).

### 2.2 Frame type

The first field, bits 0..1 (2 bits), is the frame type T = bit0 + 2*bit1:

| T | meaning | frame length (octets) | bits used |
|---|---|---|---|
| 0 | active speech, 6.3 kbit/s | 24 | 192 |
| 1 | active speech, 5.3 kbit/s | 20 | 160 |
| 2 | SID (silence insertion descriptor) | 4 | 32 |
| 3 | untransmitted (no data; inside the test files it is stored as a single octet) | 1 | 2 |

(bit0 is RATEFLAG, bit1 is VADFLAG in the Recommendation's terminology.)

### 2.3 6.3 kbit/s active frame (T = 0), field list with bit positions

| bits | width | field | notes |
|---|---|---|---|
| 0–1 | 2 | frame type (= 0) | |
| 2–25 | 24 | LSP index L24 | bits 0–7 of L24 = band 2 index, bits 8–15 = band 1 index, bits 16–23 = band 0 index |
| 26–32 | 7 | ACL0 | pitch code for subframes 0–1 |
| 33–34 | 2 | ACL1 | lag delta code, subframe 1 |
| 35–41 | 7 | ACL2 | pitch code for subframes 2–3 |
| 42–43 | 2 | ACL3 | lag delta code, subframe 3 |
| 44–55 | 12 | GAIN0 | combined gain, subframe 0 |
| 56–67 | 12 | GAIN1 | |
| 68–79 | 12 | GAIN2 | |
| 80–91 | 12 | GAIN3 | |
| 92 | 1 | GRID0 | |
| 93 | 1 | GRID1 | |
| 94 | 1 | GRID2 | |
| 95 | 1 | GRID3 | |
| 96 | 1 | reserved | ignored by the decoder |
| 97–109 | 13 | MSBPOS | combined most significant parts of the 4 position indices |
| 110–125 | 16 | POS0 low part | |
| 126–139 | 14 | POS1 low part | |
| 140–155 | 16 | POS2 low part | |
| 156–169 | 14 | POS3 low part | |
| 170–175 | 6 | PSIG0 | pulse signs, subframe 0 |
| 176–180 | 5 | PSIG1 | |
| 181–186 | 6 | PSIG2 | |
| 187–191 | 5 | PSIG3 | |

### 2.4 5.3 kbit/s active frame (T = 1)

Bits 0–95 are laid out exactly as for 6.3 kbit/s (type, L24, ACL0..3, GAIN0..3,
GRID0..3). Then:

| bits | width | field |
|---|---|---|
| 96–107 | 12 | POS0 |
| 108–119 | 12 | POS1 |
| 120–131 | 12 | POS2 |
| 132–143 | 12 | POS3 |
| 144–147 | 4 | PSIG0 |
| 148–151 | 4 | PSIG1 |
| 152–155 | 4 | PSIG2 |
| 156–159 | 4 | PSIG3 |

### 2.5 SID frame (T = 2)

| bits | width | field |
|---|---|---|
| 0–1 | 2 | frame type (= 2) |
| 2–25 | 24 | LSP index L24 (same layout as active frames) |
| 26–31 | 6 | SID gain index |

### 2.6 Untransmitted frame (T = 3)

No field other than the type. (In a real system nothing is received; the reference's file
format stores one octet whose two low bits are 11.)

### 2.7 Parameter derivation from the fields (active frames), including validity checks

This is done **in exactly this order**, because the first invalid field stops the parsing
and turns the frame into a "bad frame" (section 4.1). If a frame-erasure flag is already set
for this frame by the outside world (section 4.1, step 1), none of this is done at all (and the
current rate is not changed).

1. **Rate.** rate := 6.3 if T = 0, 5.3 if T = 1. This "current rate" is a decoder-wide
   variable; it is set here, before any validity check, so a frame that turns out to be bad
   still changes it. (In practice the current rate is only consumed while decoding a good
   active frame, which always sets it first; see 7.12.)
2. **LSP index** L24 (no check).
3. **Even lag 0.** If ACL0 > 123 → bad frame, stop. Else L0 := ACL0 + 18 (range 18..141).
4. **Delta 1.** D1 := ACL1 (0..3).
5. **Even lag 2.** If ACL2 > 123 → bad frame, stop. Else L2 := ACL2 + 18.
6. **Delta 3.** D3 := ACL3.
   The lag-delta for subframes 0 and 2 is fixed to D0 = D2 = 1.
   The even lag that governs subframe i is Le(i) = L0 for i = 0,1 and L2 for i = 2,3.
7. **Gains**, for i = 0, 1, 2, 3 in this order, with G = GAINi (12 bits):
   * train(i) := 0; bound := 170; G' := G.
   * If rate is 6.3 **and** Le(i) < 58: train(i) := floor(G / 2048) (bit 11 of G);
     G' := G mod 2048; bound := 85.
   * P(i) := floor(G' / 24) (adaptive codebook gain index); M(i) := G' mod 24 (fixed
     codebook gain index).
   * If P(i) >= bound → bad frame, stop.
8. **Grids** grid(i) := GRIDi (0 or 1).
9. **6.3 kbit/s only:** skip the reserved bit. Let X := MSBPOS (0..8191), q := floor(X/90),
   s := X mod 90. Then
   m0 := floor(q / 9), m1 := q mod 9, m2 := floor(s / 9), m3 := s mod 9.
   pos(0) := m0*65536 + POS0low; pos(1) := m1*16384 + POS1low;
   pos(2) := m2*65536 + POS2low; pos(3) := m3*16384 + POS3low  (int32 values).
   sign(i) := PSIGi (6, 5, 6, 5 bits).
   No validity check here: m0 can be as large as 10, which yields a position index beyond
   the codebook; that case is handled during excitation decoding (4.5.1, step 1).
10. **5.3 kbit/s only:** pos(i) := POSi (12 bits), sign(i) := PSIGi (4 bits).

SID frame derivation: L24 and the SID gain index (6 bits). No checks.

---------------------------------------------------------------------------------------

## 3. Decoder state

Everything below persists from frame to frame. Nothing else does (all other quantities are
recomputed every frame). Initial values are those set at decoder creation (the reference
also never resets them later).

| state | type | initial value | role |
|---|---|---|---|
| prev_lsp[10] | int16 | the `lsp_dc` table | quantized LSP vector of the previous frame (subframe 3) |
| prev_exc[145] | int16 | 0 | excitation history: the last 145 excitation samples (oldest first) |
| synth_mem[10] | int16 | 0 | LPC synthesis filter memory; synth_mem[0] is the most recent output |
| pf_fir_mem[10] | int16 | 0 | formant postfilter numerator memory (past synthesis outputs, [0] most recent) |
| pf_iir_mem[10] | int16 | 0 | formant postfilter denominator memory (past ARMA outputs, [0] most recent) |
| tilt_k | int16 | 0 | smoothed first reflection coefficient (k1 of clause 3.8) |
| out_gain | int16 | 4096 (0x1000) | gain-scaling-unit gain g[n] |
| erase_count | int16 | 0 | number of consecutive erased active frames, capped at 3 |
| conceal_gain | int16 | 0 | concealment gain for unvoiced regeneration |
| conceal_lag | int16 | 0 | voicing decision / period for concealment (0 = unvoiced) |
| conceal_seed | int16 | 0 | random seed for unvoiced concealment (never reset) |
| past_ftype | int16 | 1 | frame type decided for the previous frame: 1 active, 2 SID, 0 untransmitted |
| sid_gain | int16 | 0 | decoded SID gain; **also** reused as storage for the energy estimate of the last good active frame (see 4.4.2) |
| cur_gain | int16 | 0 | smoothed CNG target gain; **also** reused as storage for the normalisation shift of that energy estimate (see 4.4.2) |
| sid_lsp[10] | int16 | the `lsp_dc` table | LSP vector used during comfort noise |
| cng_seed | int16 | 12345 | random seed for comfort-noise generation |
| rate | enum | 6.3 | current rate (only meaningful within a good active frame, see 7.12) |

Postfilter switch: a decoder configuration flag, **default ON**. The reference decoder
exposes it as a command-line option ("-Nop" turns it off). Three of the official decoder test
vectors must be run with it OFF (section 6). Its effects are described in 4.4.3, 4.7 and 4.8.

---------------------------------------------------------------------------------------

## 4. Decoding algorithm

### 4.1 Frame-level control flow

For every received frame, with external erasure flag E (0 = frame received correctly,
non-zero = frame erased; E = 0 when no erasure information is available):

1. **Parse.** If E != 0, nothing is parsed (the received bits, if any, are ignored) and
   the frame is "bad". Otherwise read the frame type T and parse as in 2.7 (active frames)
   or 2.5 (SID). If an active frame fails a validity check of 2.7, the frame is "bad"
   exactly as if E had been non-zero. Set ftype := 1 for T = 0 or 1, 2 for T = 2, 0 for
   T = 3 (good frames only).
2. **Bad frame type substitution.** If the frame is bad: ftype := 1 if past_ftype = 1,
   otherwise ftype := 0. (A bad frame after an active frame is concealed as an active
   frame; a bad frame after a SID or untransmitted frame is treated as untransmitted.)
3. **Branch:**
   * ftype = 1 and frame good → *good active frame*: steps 4.2 to 4.6, then the
     common tail.
   * ftype = 1 and frame bad → *erased active frame*: section 4.9, then the common tail.
   * ftype = 0 or 2 → *comfort-noise frame*: section 4.10, then the common tail.
4. **Common tail.** past_ftype := ftype. Then for each subframe i = 0..3 in order:
   LPC synthesis (4.7) of the 60 excitation samples of subframe i with the LPC set of
   subframe i; then, only if the postfilter switch is ON, the formant postfilter (4.8.1)
   followed by the gain scaling unit (4.8.2). The 240 resulting samples are the output.

The bookkeeping performed in the active branches (both good and erased), in the order given:

* erase_count: if the frame is bad, erase_count := add16(erase_count, 1), else
  erase_count := 0; then if erase_count > 3, erase_count := 3. This is done first.
* LSP decoding (4.2) using prev_lsp and the bad/good status; LSP interpolation and
  conversion (4.3) using the new LSP and prev_lsp; then prev_lsp := new LSP.
* conceal_gain: if erase_count = 0 then conceal_gain :=
  `fixed_gain`[shr16(add16(M(2), M(3)), 1)]; else conceal_gain :=
  mul16r(conceal_gain, 0x6000) (0x6000 = 24576 ≈ 0.75).
* Excitation: good frame → 4.4 to 4.6; bad frame → 4.9.
* After either: prev_exc := the last 145 samples of the 385-sample buffer
  [prev_exc (old) | 240 new excitation samples] **as modified by the step that produced it**
  (see 4.4.4 and 4.9 for which buffer content is meant).
* cng_seed := 12345 (in both the good and the erased active case).

### 4.2 LSP inverse quantization (good active frames, erased active frames, SID frames)

Inputs: index L24 (forced to 0 for bad frames), the reference vector R[10] (= prev_lsp for
active frames; for SID frames also prev_lsp, see 4.10.1), bad flag.
Output: LSP vector p[10] (int16).

1. Parameters: good frame (and SID): predictor b = 12288 (0.375 in Q15), minimum spacing
   S = 256 (0x100). Bad frame: L24 := 0, b = 23552 (≈ 0.71875), S = 512 (0x200).
2. Codebook lookup: band 2 index = L24 AND 0xff, band 1 index = (L24 >> 8) AND 0xff,
   band 0 index = (L24 >> 16) AND 0xff. p[0..2] := `lsp_vq_band0`[band0 index][0..2],
   p[3..5] := `lsp_vq_band1`[band1 index][0..2], p[6..9] := `lsp_vq_band2`[band2 index][0..3].
   (Row 0 of every band is all zeros, which is what a bad frame uses.)
3. Prediction and DC: for j = 0..9:
   d := sub16(R[j], DC[j]) where DC = `lsp_dc`;
   p[j] := add16(p[j], mul16r(d, b));
   p[j] := add16(p[j], DC[j]).
   (R itself must not be changed; the reference temporarily modifies it and restores it,
   which is exact for all reachable values.)
4. Stability enforcement, at most 10 passes. For pass = 1..10:
   a. If p[0] < 384 (0x180) then p[0] := 384. If p[9] > 32256 (0x7e00) then p[9] := 32256.
   b. For j = 1..9 in increasing order: t := sub16(add16(S, p[j-1]), p[j]); if t > 0:
      t := shr16(t, 1); p[j-1] := sub16(p[j-1], t); p[j] := add16(p[j], t).
      (Each j uses the values already modified for smaller j.)
   c. Test: unstable := false; for j = 1..9: if sub16(sub16(add16(p[j-1], S), 4), p[j]) > 0
      then unstable := true.
   d. If not unstable, stop (the vector is final).
5. If after the 10th pass the vector is still unstable, p := R (the previous vector,
   unmodified). (No official test vector reaches this; see section 6.5.)

Recommendation: 2.6 and 3.10.1 (the prose's ∆min = 31.25 Hz corresponds to S = 256,
62.5 Hz to 512; the prose does not mention the clamps of step 4a, the "−4" tolerance in the
test of step 4c, nor the exact integer form of the averaging in 4b).

### 4.3 LSP interpolation and LSP-to-LPC conversion

Inputs: current vector C[10] (new LSP), previous vector R[10]. Output: four LPC sets
A_i[10], i = 0..3 (int16).

**4.3.1 Interpolation.** For i = 0..3, w := -8192*(i+1) (as int16: -8192, -16384, -24576,
-32768). For j = 0..9:
acc := dep_hi(R[j]); acc := mac32(acc, w, R[j]); acc := msu32(acc, w, C[j]);
q_i[j] := round16(acc).
(This is (1 - (i+1)/4)·R + ((i+1)/4)·C with rounding, the weights of equation (8); for
i = 3 it reproduces C exactly.)

**4.3.2 Conversion** of one interpolated vector q[10] to LPC coefficients a[10]:

1. Cosine lookup with linear interpolation, for k = 0..9:
   j := shr16(q[k], 7) (q is positive, so j = q >> 7, 0..255);
   acc := dep_hi(cos[j]); δ := sub16(cos[j+1], cos[j]) where cos = `cosine` table;
   f := add16(shl16(q[k] AND 0x7f, 8), 0x80);
   acc := mac32(acc, δ, f); acc := shl32(acc, 1);
   c[k] := neg16(round16(acc)).
   (c[k] ≈ −2cos(ω_k) in Q14-ish scaling; just apply the operations.)
2. Initial products (int32), using c[0], c[2] for P and c[1], c[3] for Q:
   P[0] := 0x10000000; P[1] := mac32(lmul(c[0], 0x2000), c[2], 0x2000);
   P[2] := add32(shr32(lmul(c[0], c[2]), 1), 0x20000000).
   Q[0] := 0x10000000; Q[1] := mac32(lmul(c[1], 0x2000), c[3], 0x2000);
   Q[2] := add32(shr32(lmul(c[1], c[3]), 1), 0x20000000).
3. For m = 2, 3, 4 (in this order), with x := c[2m] for P and y := c[2m+1] for Q
   (formulas given for P; Q is identical with y and Q). All right-hand sides use the
   values of P at the **start of this step m** unless stated:
   a. P[m+1] := add32(mul32x16(P[m], x), P[m-1]).
   b. For j = m down to 2 (descending): P[j] := add32(add32(mul32x16(P[j-1], x),
      shr32(P[j], 1)), shr32(P[j-2], 1)). (Descending order guarantees that P[j-1] and
      P[j-2] are still the start-of-step values when P[j] is computed.)
   c. P[0] := shr32(P[0], 1).
   d. P[1] := shr32(add32(shr32(dep_hi(x), m), P[1]), 1).
4. Output, for k = 0..4:
   a[k] := neg16(round16(shl32(add32(sub32(add32(P[k], P[k+1]), Q[k]), Q[k+1]), 3)));
   a[9-k] := neg16(round16(shl32(sub32(add32(add32(P[k], P[k+1]), Q[k]), Q[k+1]), 3))).
   (The additions are performed left to right exactly as written: first P[k]+P[k+1], then
   ∓Q[k], then ±Q[k+1].)

The resulting a[k] are used by the synthesis filter as sy[n] = (excitation term) + Σ a[k]
·sy[n-1-k] in the scaled fixed-point form of 4.7 (the sign convention already matches
equation (48); no further negation).

Recommendation: 2.7, 3.3. The fixed-point polynomial recursion is not in the prose.

### 4.4 Excitation decoding (good active frames)

Build a 385-sample int16 buffer X: X[0..144] := prev_exc, X[145..384] to be filled.
Subframe i occupies X[145+60i .. 204+60i].

For i = 0..3 in order:
1. Fixed codebook vector v[0..59] for subframe i (4.5.1 for 6.3, 4.5.2 for 5.3).
2. Adaptive codebook vector u[0..59] for subframe i (4.4.1), computed from X, which at
   this point holds prev_exc and the **final** excitation of subframes 0..i-1.
3. X[145+60i+n] := add16(shl16(v[n], 1), u[n]) for n = 0..59.

#### 4.4.1 Adaptive codebook (pitch) contribution

Inputs: a buffer X and the position s of the first sample of the subframe within X (the
145 samples X[s-145..s-1] are available), even lag Le, lag delta D (0..3), gain index P,
rate. Output u[0..59].

1. Lag: L := Le − 1 + D (so L = Le for even subframes, Le−1..Le+2 for odd ones;
   range 17..143).
2. Delayed excitation r[0..63]: r[0] := X[s−L−2], r[1] := X[s−L−1], and for k = 0..61:
   r[2+k] := X[s − L + (k mod L)]. (When L < 62 the last period is repeated: this is the
   periodic extension of clause 2.18 equation (41.1).)
3. Gain table: the 85-entry table `acb_gain_85` if rate is 6.3 and Le < 58; otherwise
   (6.3 with Le >= 58, or 5.3 always) the 170-entry table `acb_gain_170`.
   g[0..4] := columns 0..4 of row P.
4. For n = 0..59: acc := 0; for j = 0..4: acc := mac32(acc, r[n+j], g[j]);
   acc := shl32(acc, 1); u[n] := round16(acc).

Recommendation: 2.18, 3.4.

#### 4.4.2 Voicing classification and energy memory (every good active frame)

After the four subframes are built:

1. Save E[n] := X[145+n], n = 0..239 (the unfiltered excitation; this is the frame's
   excitation if the postfilter is OFF).
2. Make a normalised copy: Xn := X (all 385 samples); z := vnorm(Xn) over all 385 samples.
   **cur_gain := z.**
3. Lag search: Lc := L2 (the even lag of subframes 2–3, i.e. ACL2+18); if Lc > 142 then
   Lc := 142. best := Lc; Cbest := 0 (int32).
   For t = Lc−3 to Lc+3 (increasing): C := sequential mac32 from 0 over j = 0..119 of
   Xn[265+j]·Xn[265−t+j]. If C > Cbest (strictly): Cbest := C, best := t.
4. Ten := round16(sequential mac32 from 0 over j = 0..119 of Xn[265+j]²).
   **sid_gain := Ten.**
5. Eb := sequential mac32 from 0 over j = 0..119 of Xn[265−best+j]² (int32).
6. Cc := round16(Cbest). If Cc <= 0: conceal_lag := 0. Otherwise:
   En := round16(Eb); acc := shr32(lmul(En, Ten), 3); acc := msu32(acc, Cc, Cc);
   conceal_lag := best if acc < 0, else 0.

(So conceal_lag = 0 means "unvoiced". The pair (sid_gain, cur_gain) now holds the energy
estimate and its scale of this last good active frame; they are consumed only if the next
frame is an untransmitted frame that directly follows an active frame, 4.10.2.)

Recommendation: 3.10.2 (prose: "prediction gain more than 0.58 dB" — this is the
`8·Cc² > En·Ten` test), A.5.2.

#### 4.4.3 Pitch postfilter (good active frames, postfilter ON only)

If the postfilter switch is OFF, the frame excitation is E[0..239] from 4.4.2 step 1 and
this step is skipped entirely.

If ON: for each subframe i = 0..3, compute the postfilter parameters (lag M, gain gl,
scale gp) from the **normalised** buffer Xn of 4.4.2 (using the subframe's even lag
Le(i)), then filter the **original** (unnormalised) buffer X. All four parameter sets are
computed first, then the four subframes are filtered (the order does not matter since
filtering reads only X).

Parameter computation for subframe i, with b := 145 + 60i (start of the subframe in Xn),
Lp := Le(i), and if Lp > 142 then Lp := 142:

1. Backward search: Mb := 0, Cb := 0. For t = Lp−3 to Lp+3 (increasing):
   C := sequential mac32 from 0 over j = 0..59 of Xn[b+j]·Xn[b−t+j];
   if C > Cb (strictly): Cb := C, Mb := −t.
2. Forward search: Mf := 0, Cf := 0. For t = Lp−3 to Lp+3: if 60i + 60 + t <= 240,
   C := sequential mac32 over j = 0..59 of Xn[b+j]·Xn[b+t+j]; otherwise C := 0.
   If C > Cf (strictly): Cf := C, Mf := t.
3. If Mb = 0 and Mf = 0: M := 0, gl := 0, gp := 32767. Done.
4. Correlations (int32 vector V[0..4], each a sequential mac32 from 0 over j = 0..59):
   V[0] := Σ Xn[b+j]²;
   if Mb != 0: V[1] := Σ Xn[b+j]·Xn[b+Mb+j], V[2] := Σ Xn[b+Mb+j]²; else V[1] = V[2] := 0;
   if Mf != 0: V[3] := Σ Xn[b+j]·Xn[b+Mf+j], V[4] := Σ Xn[b+Mf+j]²; else V[3] = V[4] := 0.
   (Note Mb is negative, so Xn[b+Mb+j] lies in the past.)
5. Common scaling: Vmax := max(0, V[0..4]) (start from 0, strict comparisons);
   sh := norm32(Vmax); W[k] := hi16(shl32(V[k], sh)) for k = 0..4 (int16).
6. Choice:
   * Mb != 0 and Mf = 0 → (M, Ccr, Enr) := (Mb, W[1], W[2]).
   * Mb = 0 and Mf != 0 → (M, Ccr, Enr) := (Mf, W[3], W[4]).
   * both non-zero → a := lmul(mul16r(W[1], W[1]), W[4]); c := lmul(mul16r(W[3], W[3]),
     W[2]); if a > c (strictly) choose backward (Mb, W[1], W[2]), else forward
     (Mf, W[3], W[4]).
   Then compute the gains with Ten := W[0] (next step).
7. Gains (M, Ten, Ccr, Enr given):
   a. A := shr32(lmul(Ten, Enr), 2); B := lmul(Ccr, Ccr).
   b. If B > A (strictly):
      * γ := `pitch_postfilter_weight`[0] (6144) if rate is 6.3, [1] (8192) if 5.3.
      * If Ccr >= Enr: g := γ. Else g := mul16(div16(Ccr, Enr), γ).
      * acc := shr32(dep_hi(Ten), 1); acc := mac32(acc, Ccr, g);
        e := mul16(g, g); acc2 := shr32(lmul(Enr, e), 1); acc := add32(acc, acc2);
        den := round16(acc).
      * num := shr32(dep_hi(Ten), 1). If num >= dep_hi(den): q := 32767; else
        q := div32x16(num, den).
      * gp := isqrt(dep_hi(q)).
   c. Else: g := 0, gp := 32767.
   d. gl := mul16(g, gp).
   Return (M, gl, gp).

Filtering of subframe i (writes the frame excitation F[60i..60i+59]):
F[60i+n] := round16(mac32(lmul(X[b+n], gp), X[b+M+n], gl)), n = 0..59,
where X is the **unnormalised** buffer (same contents as before 4.4.2 step 2). When M = 0
this term reads X[b+n] itself (gl is then 0).

The frame excitation passed to synthesis is F (postfilter ON) or E (OFF).

Recommendation: 3.6. [DIFFERS FROM PROSE] in several respects the prose does not state:
the correlations are computed on the normalised excitation while the filter is applied to
the original; the forward lag is only allowed if the whole delayed window lies inside the
current frame (60i+60+t <= 240); when both lags qualify and a = c exactly, the forward
lag wins; the
"1.25 dB" test is B > A (i.e. Ccr² > Ten·Enr/4); and the scaling gain formula is the
integer sequence in step 7b.

#### 4.4.4 End of a good active frame

1. sid_lsp := prev_lsp (which by now holds this frame's decoded LSP vector).
2. prev_exc := X[240..384] of the **original** buffer X (i.e. the last 145 samples of
   [old prev_exc | E]). The pitch-postfiltered signal F is NOT fed back.
3. cng_seed := 12345 (see 4.1).

### 4.5 Fixed codebook

#### 4.5.1 6.3 kbit/s (MP-MLQ)

Inputs for subframe i: pos(i) (int32), sign(i), grid(i), M(i), train(i), even lag Le(i).
Output v[0..59].

1. v[n] := 0 for all n. N := `mpmlq_pulse_count`[i] (6, 5, 6, 5). If pos(i) >=
   `mpmlq_codebook_size`[i] (593775 for even, 142506 for odd subframes), stop: v stays all
   zero, **and no pulse train is applied** even if train(i) = 1.
2. Combinatorial decoding: r := 6 − N; A := pos(i) (int32). For c = 0..29 in increasing
   order:
   A' := sub32(A, T[r][c]) where T = `mpmlq_combinatorial`;
   if A' < 0: (a pulse is at grid slot c) r := r + 1; the sign bit is bit number (6 − r)
   of sign(i) (bit 0 = LSB); the pulse value is −`fixed_gain`[M(i)] if that bit is 1 and
   +`fixed_gain`[M(i)] if it is 0; v[grid(i) + 2c] := that value; if r = 6, stop the loop.
   (A is left unchanged.) Otherwise A := A'.
   The first pulse found (smallest position) uses the most significant bit of the N-bit sign
   field, the last pulse uses bit 0.
3. Pulse train: if train(i) = 1, let L := Le(i) (necessarily < 58), keep a copy v0 := v,
   then for k = 1, 2, ... while k·L < 60: for n = k·L .. 59 in increasing order,
   v[n] := add16(v[n], v0[n − k·L]). (Each pass adds the ORIGINAL vector delayed by k·L;
   the passes are done in increasing k.)

Recommendation: 2.15, 2.17. The prose says "the C code provides the details" for the
MSBPOS packing; the above is that detail.

#### 4.5.2 5.3 kbit/s (ACELP)

Inputs: pos(i) (12 bits), sign(i) (4 bits), grid(i), M(i), adaptive gain index P(i),
even lag Le(i), delta D(i) (D = 1 for even subframes). Output v[0..59].

1. Work vector w[0..63] := 0. g := `fixed_gain`[M(i)].
2. For pulse k = 0..3: slot := (pos(i) >> (3k)) AND 7; n := 8·slot + grid(i) + 2k;
   w[n] := +g if bit k of sign(i) is 1, −g if it is 0. **[Note: the sign convention is the
   opposite of the 6.3 kbit/s one.]** Positions 60..63 are legal codes meaning "no pulse"
   (they land outside the subframe and are discarded).
3. v[n] := w[n] for n = 0..59.
4. Pitch sharpening: T0 := Le(i) − 1 + D(i) + `acelp_pitch_lag_offset`[P(i)];
   β := `acelp_pitch_gain`[P(i)]. If T0 < 58: for n = T0 .. 59 in increasing order,
   v[n] := add16(v[n], mul16(v[n − T0], β)). This is **recursive**: v[n−T0] may itself
   already have been modified in this loop.
   (The placeholder offset 60 always yields T0 >= 77, i.e. no sharpening.)

Recommendation: 2.16 (Table 1 and the last paragraph), 2.17.

### 4.6 (good active frame) — order summary

For clarity, the complete order for a good active frame is: 4.1 bookkeeping (erase_count)
→ 4.2 → 4.3 → prev_lsp update → conceal_gain → 4.4 (with 4.4.1 and 4.5 inside) → 4.4.2 →
4.4.3 → 4.4.4 → past_ftype := 1 → synthesis/postfilter per subframe (4.7, 4.8).

### 4.7 LPC synthesis filter (all frames)

For subframe i with LPC set a[0..9] = A_i and input samples x[0..59] (frame excitation
samples 60i..60i+59), for n = 0..59 in order:

1. acc := shr32(dep_hi(x[n]), 3).
2. For j = 0..9: acc := mac32(acc, a[j], synth_mem[j]) (sequential).
3. Shift the memory: synth_mem[9..1] := synth_mem[8..0].
4. acc := shl32(acc, 2); synth_mem[0] := round16(acc).
5. Output y[n] := synth_mem[0] if the postfilter is ON; := shl16(synth_mem[0], 1) if OFF.

The memory carries over between subframes and frames regardless of frame type.

Recommendation: 3.7 (equation (48); the 1/8 input scaling, ×4 output scaling and the
final ×2 when the postfilter is off are fixed-point details).

### 4.8 Formant postfilter and gain scaling (postfilter ON only)

#### 4.8.1 Formant postfilter

Input: the synthesised subframe y[0..59] (from 4.7) and its LPC set a[0..9].
Output: the postfiltered subframe y'[0..59] and an energy value Sen (int32).

1. Coefficients, j = 0..9: fz[j] := mul16r(a[j], `postfilter_zero_weights`[j]);
   fp[j] := mul16r(a[j], `postfilter_pole_weights`[j]).
2. Copy t := y; z := vnorm(t) (over 60 samples).
3. Autocorrelations: R1 := sequential mac32 from 0 over n = 1..59 of t[n]·t[n−1];
   R0 := lmul(t[0], t[0]) then sequential mac32 over n = 1..59 of t[n]·t[n].
4. Sen := shr32(R0, 2z + 4) (the shift count 2z+4 is in −2..28; a negative count means a
   saturating left shift).
5. Reflection coefficient: h := hi16(R0). If h != 0: R1h := shr32(R1, 1);
   k := div32x16(abs32(R1h), h); if R1h < 0 then k := neg16(k). If h = 0: k := 0.
6. Smoothing: acc := dep_hi(tilt_k); acc := msu32(acc, tilt_k, 0x2000);
   acc := mac32(acc, k, 0x2000); tilt_k := round16(acc).
   (tilt_k := 3/4·tilt_k + 1/4·k with rounding.)
7. Tilt coefficient: μ := mul16(tilt_k, −16384) AND 0xfffc (bitwise AND on the 16-bit
   two's-complement pattern, i.e. floor to a multiple of 4).
8. For n = 0..59 in order:
   a. acc := shr32(dep_hi(y[n]), 2).
   b. For j = 0..9: acc := msu32(acc, fz[j], pf_fir_mem[j]).
   c. Shift pf_fir_mem: [9..1] := [8..0]; pf_fir_mem[0] := y[n] (the INPUT sample).
   d. For j = 0..9: acc := mac32(acc, fp[j], pf_iir_mem[j]).
   e. Shift pf_iir_mem: [9..1] := [8..0].
   f. acc := shl32(acc, 2); pf_iir_mem[0] := round16(acc).
   g. acc := mac32(acc, pf_iir_mem[1], μ) (pf_iir_mem[1] is now the previous ARMA output;
      acc is the 32-bit value of step f, before rounding).
   h. y'[n] := round16(acc).

Recommendation: 3.8. [DIFFERS FROM PROSE]: k is computed from the normalised signal with
the specific divisions above; the tilt filter coefficient is the smoothed k1 times −0.25
truncated to a multiple of 4 in Q15; and the energy Sen returned for the gain unit is the
energy of the **input** y (scaled back), as defined in step 4.

#### 4.8.2 Gain scaling unit

Inputs: y'[0..59] from 4.8.1, Sen. Output: final output samples o[0..59].

1. Sp := energy_q(y') (section 1.3).
2. If Sp != 0 and Sen != 0:
   e1 := norm32(Sp); Sp := shl32(Sp, e1);
   e2 := norm32(Sen); Sn := shl32(Sen, e2); Sn := shr32(Sn, 1);
   e := sub16(6, add16(sub16(e1, e2), 1)); if e < 0 then e := 0;
   q := div32x16(Sn, hi16(Sp));
   gs := isqrt(shr32(dep_hi(q), e)).
   Otherwise gs := 4096 (0x1000).
3. For n = 0..59 in order:
   acc := dep_hi(out_gain); acc := msu32(acc, out_gain, 0x0800);
   acc := mac32(acc, gs, 0x0800); out_gain := round16(acc);
   m := add16(out_gain, shr16(out_gain, 4));
   o[n] := round16(shl32(lmul(y'[n], m), 4)).

(out_gain is the g[n] of equation (51) with α = 1/16; m is g[n]·(1+α) of (52). In this
scaling 4096 means unity gain.)

Recommendation: 3.9 (equations (50)–(52)). The prose gives the real-valued ratio; the
normalisations, the halving of Sen, the clamp of the shift e at 0 and the use of energy_q
(input divided by 4 before squaring) are fixed-point details — implement exactly the
integer steps above.

### 4.9 Erased active frame (concealment)

Performed when ftype = 1 and the frame is bad. The bookkeeping of 4.1 applies
(erase_count incremented and capped at 3; LSP decoded in "bad" mode in 4.2 → 4.3 →
prev_lsp update; conceal_gain := mul16r(conceal_gain, 0x6000)).

Excitation: let X be the 385-sample buffer with X[0..144] := prev_exc.

* If erase_count >= 3: the frame excitation is 240 zeros and the **whole** X becomes zero
  (so prev_exc becomes all zero).
* Else if conceal_lag != 0 (voiced): with L := conceal_lag,
  first for n = 0..239 in increasing order X[145+n] := X[145−L+n] (a forward copy: once
  n >= L it re-reads samples written by this same loop, producing a periodic signal);
  then, as a separate pass, for n = 0..239: X[145+n] := mul16(X[145+n], 0x6000), and the
  frame excitation sample n is that attenuated value.
  (Attenuation applies after the whole copy, never inside it.)
* Else (unvoiced): frame excitation sample n := mul16(conceal_gain, rand16(conceal_seed)),
  n = 0..239 in order (the full signed 16-bit random value is used); and the **whole** X is
  set to zero (prev_exc becomes zero).

Then prev_exc := X[240..384]; cng_seed := 12345. No pitch postfilter is applied; conceal_lag,
sid_gain, cur_gain and sid_lsp are not modified. The common tail (synthesis, formant
postfilter, gain scaling) runs normally with the LPC sets of this frame.

Recommendation: 3.10, 3.10.1, 3.10.2. [DIFFERS FROM PROSE]: the prose says the regenerated
vector is attenuated "by an additional 2.5 dB" for each of the next two frames; the
reference multiplies by 0.75 (mul16 by 0x6000) on every voiced erased frame, including the
first, cumulatively through prev_exc; the unvoiced gain is multiplied by 0.75 (mul16r)
on every erased frame including the first; and on the 3rd consecutive erasure the output
excitation is zero.

### 4.10 Comfort-noise frames (ftype = 0 or 2)

Order of operations:

#### 4.10.1 SID decoding (ftype = 2 only, i.e. a good SID frame)

1. sid_gain := sid_gain_decode(SID gain index) (4.10.4).
2. sid_lsp := LSP inverse quantization (4.2) of L24 in **good** mode (b = 12288, S = 256),
   with reference vector R := prev_lsp.

#### 4.10.2 First SID lost (ftype = 0 and past_ftype = 1)

This happens when a good untransmitted frame directly follows an active frame (good or
erased): the SID frame that should have started the silence period was lost. (A bad frame
can never reach this case: after an active frame a bad frame is concealed as active.)
1. idx := sid_gain_quantize(sid_gain, cur_gain) (4.10.5) — using the energy/shift pair
   stored by the last good active frame (4.4.2); if no good active frame was ever decoded,
   both are still 0.
2. sid_gain := sid_gain_decode(idx).
sid_lsp is NOT changed here: it still holds the LSP of the last good active frame (4.4.4)
(or the initial DC vector).

For ftype = 0 with past_ftype != 1, nothing is decoded: the previous SID parameters are
kept.

#### 4.10.3 Target gain, excitation, LSPs

1. If past_ftype = 1: cur_gain := sid_gain. Else: cur_gain :=
   hi16(add32(lmul(cur_gain, 0x7000), lmul(sid_gain, 0x1000)))
   (= floor((7·cur_gain + sid_gain)/8)).
2. Generate the 240-sample comfort-noise excitation and update prev_exc (4.10.6) using
   cur_gain and cng_seed.
3. LPC sets: interpolation and conversion (4.3) with current vector C := sid_lsp and
   previous vector R := prev_lsp. Then prev_lsp := sid_lsp.
4. Nothing else of the decoder state changes (erase_count, conceal_*, synthesis and
   postfilter memories untouched here; cng_seed is NOT reset). Then the common tail of 4.1
   (past_ftype := ftype, synthesis, formant postfilter + gain scaling if ON). No pitch
   postfilter.

#### 4.10.4 sid_gain_decode(idx) (idx in 0..63)

seg := idx >> 4; if seg = 3 then seg := 2. i := idx − 16·seg.
value := i·2^(seg+1) + base[seg], with base = `cng_gain_segment_base` = (0, 32, 96).
Result := value·32 (int16; at most 11008, never saturates).

(Segment 0: 0, 2, 4, ..., 30; segment 1: 32, 36, ..., 92; segment 2 with idx 32..63:
96, 104, ..., 344. The prose formula (A-14) is garbled; this is the reference behaviour.)

#### 4.10.5 sid_gain_quantize(Ener, sh) — decoder use only (energy restoration)

Inputs: Ener = sid_gain (int16, holding Ten of 4.4.2, >= 0), sh = cur_gain (int16,
holding z of 4.4.2, range −3..11).
1. s := sub16(16, shl16(sh, 1)) (range −8..22).
   acc := shl32(dep_lo(Ener), s) (saturating; a negative s is a right shift).
   x := mul32x16(acc, 273) (273 = `cng_energy_factor`[0]).
2. Bounds B = `cng_gain_segment_bounds` = (2048, 18432, 231233).
   If x >= B[2]: return 63.
3. If x >= B[1]: seg := 2, nb := 4. Else nb := 3 and seg := 1 if x >= B[0], else 0.
4. Binary search: j := 2^nb, step := j/2. Repeat nb times:
   v := base[seg] + j·2^(seg+1); if x >= lmul(v, v) then j := j + step else
   j := j − step; step := step/2 (integer halving).
5. v := base[seg] + j·2^(seg+1); d := sub32(lmul(v, v), x).
   * If d <= 0: j2 := j + 1; v2 := base[seg] + j2·2^(seg+1); d2 := sub32(x, lmul(v2, v2));
     result := 16·seg + j if d > d2 (strictly), else 16·seg + j2.
   * Else: j2 := j − 1; v2 := base[seg] + j2·2^(seg+1); d2 := sub32(x, lmul(v2, v2));
     result := 16·seg + j if d < d2 (strictly), else 16·seg + j2.
   (All the small additions/shifts here cannot overflow; lmul(v,v) = 2v².)
Return result (0..63; note j2 can equal 16 for seg 0 or 1, giving the first index of the
next segment, which is consistent with the decoding table).

Recommendation: A.4.3, A.5.1, A.5.2. [DIFFERS FROM PROSE]: the prose says "using the same
procedure as the average energy in 3.1 except that there is no scaling factor αw"; the
reference uses exactly the steps above (scale factor 273, shift from the normalisation
count).

#### 4.10.6 Comfort-noise excitation

State used: cng_seed (updated by every random draw), prev_exc (updated), target gain
G := cur_gain (int16). Output: 240 excitation samples.

Random draw `rnd(n)` (n = number of possible values, result in 0..n−1):
t := rand16(cng_seed) AND 0x7fff; result := mul16(t, n) (= floor(t·n/32768)).

1. **Parameter draws, in exactly this order** (30 draws per frame):
   1. lagA := rnd(21) + 123 (lag for subframes 0–1).
   2. lagB := rnd(19) + 123 (lag for subframes 2–3).
      **[DIFFERS FROM PROSE]**: the prose says the lag is in [123, 143] for both blocks;
      version 5.2 of the reference (maintenance 2006) uses rnd(19), i.e. [123, 141], for the
      second block. Bit-exactness against the test vectors requires rnd(19).
   3. Four gain indices, subframes 0, 1, 2, 3: Pc(i) := rnd(50) + 1 (range 1..50).
      **[DIFFERS FROM PROSE]**: the prose says "[0, 49]"; the reference adds 1.
   4. Lag deltas are fixed: Dc = (1, 0, 1, 3) for subframes 0..3 (lags lagA, lagA−1, lagB,
      lagB+2).
   5. Grids and signs, for block 0 then block 1: t := rnd(8192) (13 bits).
      grid of the block's first subframe := bit 0 of t;
      grid of the block's second subframe := bit 1 of t; the block's 11 pulse signs:
      sign k (k = 0..10) := +16384 if bit (k+2) of t is 1, −16384 if it is 0.
      Store the grid as an offset within the 120-sample block: off(first subframe) := grid,
      off(second subframe) := 60 + grid.
   6. Positions, for subframes 0, 1, 2, 3 in order (6, 5, 6, 5 pulses = 11 per block):
      list := (0, 1, 2, ..., 29) (30 entries), count := 30. For each pulse:
      j := rnd(count); position := 2·list[j] + off(subframe); count := count − 1;
      list[j] := list[count]. (Drawing without replacement; the moved element is the
      current last one.)
      The positions of a block are numbered 0..10 in draw order (first subframe's pulses
      first) and are relative to the start of the block (0..119); they pair with signs
      0..10 of that block.
2. **Per block b = 0, 1** (block b covers subframes 2b and 2b+1, output samples
   120b..120b+119), in order:
   1. Adaptive excitation: for each of the two subframes, compute u (4.4.1) with:
      even lag Le := lagA (block 0) or lagB (block 1), delta D := Dc of the subframe,
      gain index P := Pc of the subframe, using the 170-entry table (the lags are >= 58 so
      the table is the 170 one at either rate). The history is prev_exc in its state at the
      start of the block; because all lags are >= 122, the delayed samples always lie in
      prev_exc (positions s = 145 for the first subframe and s = 205 for the second, in the
      concatenation [prev_exc | block]; no sample of the current block is read).
      Let e[0..119] := the two u vectors concatenated.
   2. Scaling: mx := max over n of abs16(e[n]) (from 0). If mx = 0: k := 0, else
      k := sub16(4, norm16(mx)), and if k < −2 then k := −2.
   3. te[n] := shr16(e[n], k) for n = 0..119 (a negative k is a saturating left shift);
      Eu := sequential mac32 from 0 over n = 0..119 of te[n]·te[n].
   4. Cross term: acc := sequential mac32 from 0 over the 11 pulses m = 0..10 of
      te[pos_m]·sign_m; ie := hi16(shl32(acc, 1)).
   5. Target energy: acc := shr32(lmul(G, 60), 6); t := lo16(acc);
      acc := lmul(t, G); acc := shr32(acc, add16(shl16(k, 1), 4)).
   6. c := mul32x16(sub32(Eu, acc), 2979) (2979 ≈ 32768/11).
   7. b0 := mul16r(ie, 2979). D := neg32(msu32(c, b0, b0)).
   8. If D <= 0: x := neg16(b0). Else: δ := isqrt(D); x1 := sub16(δ, b0);
      x2 := add16(b0, δ); x := x1, but if abs16(x2) < abs16(x1) (strictly) then
      x := neg16(x2).
   9. k := add16(k, 1); gf := shl16(x, k); clamp gf to [−10000, 10000].
      (Combined with the ±16384 = ±0.5 signs this is the |Gf| <= 5000 bound of the
      prose.)
   10. For m = 0..10: e[pos_m] := add16(e[pos_m], mul16(gf, sign_m)).
       (Positions within a block are all distinct.)
   11. Output samples 120b+n := e[n].
   12. prev_exc update: prev_exc := last 145 samples of [prev_exc | e], i.e.
       prev_exc[0..24] := prev_exc[120..144], prev_exc[25..144] := e[0..119].
       Block 1 uses this updated prev_exc.

Recommendation: A.4.5 (equations A-18 to A-20), A.5.1. The prose describes the gain
solution in floating point; the integer sequence above is what must be implemented.

---------------------------------------------------------------------------------------

## 5. Cross-reference to the Recommendation (G.723.1 05/2006)

| this document | Recommendation clause | agreement |
|---|---|---|
| 2.1–2.6 bitstream | 4 (Tables 5, 6), A.6 (Table A.1), 2.20 (Tables 2–4) | same; the MSBPOS split and the error checks are only in this document |
| 2.7 step 7 gain index split | 2.17 eq (36), 2.18 eqs (39), (40) | same, plus the validity bounds 85/170 [only here] |
| 2.7 step 3/5 forbidden lag codes 124..127 | — | only here |
| 3 state, initial values | 3.11, A.5.3 | same, plus cur_gain = 0, conceal_seed = 0, past_ftype = 1 |
| 4.1 frame flow, erasure → frame-type substitution | 3.10, A.5.2 eq (A-21) | same |
| 4.2 LSP decoding, stability | 2.6, 3.2, 3.10.1 | see notes in 4.2 |
| 4.3 interpolation, LSP→LPC | 2.7, 3.3 eq (8)–(10) | same weights; conversion details only here |
| 4.4.1 adaptive codebook | 2.18 eq (37), (38), (41.1), (41.2), 3.4 | same |
| 4.5.1 MP-MLQ | 2.15, 2.17 | combinatorial decoding and train details only here |
| 4.5.2 ACELP | 2.16 Table 1, 2.17 | sign convention and recursive sharpening only here |
| 4.4.2 classifier | 3.10.2 | integer test only here |
| 4.4.3 pitch postfilter | 3.6 eqs (42)–(47) | see notes |
| 4.7 synthesis | 3.7 eq (48) | same |
| 4.8.1 formant postfilter | 3.8 eqs (49.1)–(49.3) | see notes |
| 4.8.2 gain scaling | 3.9 eqs (50)–(52) | see notes |
| 4.9 concealment | 3.10, 3.10.1, 3.10.2 | see notes |
| 4.10 CNG | A.3, A.4.3, A.4.5, A.4.6, A.5.1–A.5.3 | see notes (lag range of block 1, gain index range) |

---------------------------------------------------------------------------------------

## 6. Test vectors and pass criterion

### 6.1 Location

All official vectors are in the `tv/` folder of the Annex A part of the ITU-T G.723.1
software package (`G.723.1-Annex-A-Software-Release.2LC`, free with the Recommendation on
itu.int), upper-case names. They are not redistributed here.
The Annex A folder contains both the main-body vectors (PATH*, OVER*, INEQ*, TAME*, CODE*)
and the Annex A vectors (DTX*).

### 6.2 File formats

* **Bitstream files** (`.TCO` = decoder test input, `.RCO` = encoder reference output, also
  usable as decoder input): a plain concatenation of frames, no header. Each frame starts
  with an octet whose two least significant bits are the frame type T (2.2); the frame
  length is 24, 20, 4 or 1 octets for T = 0, 1, 2, 3 respectively (a T = 3 frame is that
  single octet). The decoder therefore reads one octet, determines the length, then reads
  the remaining octets of the frame. Processing stops when no further octet can be read.
  Note: a frame flagged as erased by the CRC file is still present in the bitstream file
  with its full length (its type bits still determine how many octets to skip).
* **Erasure files** (`.CRC`): one 16-bit **little-endian** signed integer per frame, in frame
  order; 0 = good, any non-zero value = erased (the files contain 0 and 1). It is read once
  per frame, after the frame's octets. (Reference corner case: if the erasure file runs out
  before the bitstream, the reference keeps using the last value read; the official files
  have exactly one value per frame.)
* **Speech files** (`.ROU` = reference decoder output, `.TIN` = encoder input): 16-bit
  **little-endian** signed PCM, 8000 Hz, 240 samples (480 octets) per frame, no header.

### 6.3 Decoder cases (all verified to be reproduced exactly by the reference, version 5.2)

| case | bitstream input | erasure file | postfilter | expected output | frames |
|---|---|---|---|---|---|
| dtx63 | DTX63.RCO | none | ON | DTX63.ROU | 864 |
| dtx53 | DTX53.RCO | none | ON | DTX53.ROU | 120 |
| dtxmix | DTXMIX.RCO | none | ON | DTXMIX.ROU | 120 |
| dtx63e | DTX63E.TCO | DTX63E.CRC | ON | DTX63E.ROU | 120 |
| dtx63b | DTX63B.TCO | none | ON | DTX63B.ROU | 11 |
| pathd63p | PATHD63P.TCO | PATHD63P.CRC | ON | PATHD63P.ROU | 100 |
| tamed63p | TAMED63P.TCO | TAMED63P.CRC | ON | TAMED63P.ROU | 100 |
| overd63p | OVERD63P.TCO | none | ON | OVERD63P.ROU | 33 |
| pathd53 | PATHD53.TCO | none | **OFF** | PATHD53.ROU | 4 |
| overd53 | OVERD53.TCO | none | **OFF** | OVERD53.ROU | 26 |
| ineqd53 | INEQD53.TCO | none | **OFF** | INEQD53.ROU | 2 |

The "P" suffix in main-body names means postfilter ON; the 5.3 main-body decoder vectors
have no "P" and must be decoded with the postfilter OFF (the ITU test document runs them
with the reference's "-Nop" option). Decoding INEQD53 with the postfilter ON does NOT match.

The rate option of the reference command line ("-r53"/"-r63") has no influence on decoding:
every one of these cases was re-run with the opposite rate option and still matched.

Ignore `.TIN` files, the encoder-only `.RCO` comparisons, `DTXMIX.RAT` (encoder rate
schedule), `CHKLIST.TAV` and `TSTG7231.DOC` (test description, Word 6 format).

### 6.4 Pass criterion

Sample-for-sample identity of the whole output file with the expected `.ROU` file (same
length, every 16-bit sample equal). There is no tolerance.

### 6.5 What the official vectors exercise (measured on the reference)

Exercised: both rates, rate switching (dtxmix), SID and untransmitted frames, first-SID-lost
restoration (dtx63e, 1 occurrence), erasure concealment with voiced and unvoiced
regeneration and erase_count 1, 2 and 3 (pathd63p), first frame erased (tamed63p), forbidden
lag codes (pathd63p, 2 frames), MP-MLQ position index beyond the codebook (pathd63p, 3
subframes), pulse trains, ACELP pitch sharpening, pitch postfilter in all its branches
including the scale-gain saturation branch (overd63p, 1 occurrence), CNG with negative
discriminant and CNG gain clamping (dtx63b, 1 occurrence).

NOT exercised by any official vector: the gain-index validity check (P >= 85 or 170) and the
LSP stability fallback to the previous vector (4.2 step 5). Implement them as described
anyway.

### 6.6 Extra vectors (non-official, generated with the reference decoder)

To cover more, 17 extra cases were produced with the reference decoder version 5.2 (built
for a little-endian machine with a 32-bit int32), same formats. They are not published
here (two of them use random inputs); ask on the DS2-Anywhere issue tracker for a copy:

| input | erasure file | postfilter | expected output |
|---|---|---|---|
| tv/CODEC63.RCO | none | ON / OFF | CODEC63_PF.ROU / CODEC63_NOPF.ROU |
| tv/PATHC63H.RCO | none | ON / OFF | PATHC63H_PF.ROU / PATHC63H_NOPF.ROU |
| tv/PATHC53.RCO | none | ON / OFF | PATHC53_PF.ROU / PATHC53_NOPF.ROU |
| tv/OVERC63.RCO | none | ON / OFF | OVERC63_PF.ROU / OVERC63_NOPF.ROU |
| tv/OVERC53H.RCO | none | ON / OFF | OVERC53H_PF.ROU / OVERC53H_NOPF.ROU |
| tv/INEQC53.RCO | none | ON / OFF | INEQC53_PF.ROU / INEQC53_NOPF.ROU |
| tv/TAMEC63H.RCO | none | ON / OFF | TAMEC63H_PF.ROU / TAMEC63H_NOPF.ROU |
| tv/DTX63.RCO | extra_vectors/DTX63_ERAS.CRC (bursts of 1–5 erasures, ~10 % of frames) | ON | DTX63_ERAS.ROU |
| extra_vectors/RANDOM.TCO (3000 frames of random bits, random frame types) | extra_vectors/RANDOM.CRC (~5 % erased) | ON / OFF | RANDOM_PF.ROU / RANDOM_NOPF.ROU |

RANDOM exercises the gain-index validity check (26 frames), forbidden lag codes (128),
out-of-range MP-MLQ indices (249), first-SID-lost (283) and erasures interleaved with SID
and untransmitted frames. Use the official vectors as the acceptance test and these as
additional regression material.

---------------------------------------------------------------------------------------

## 7. Bit-exactness traps (checklist)

1. **Floor, not truncation**, for every right shift and for mul16 / mul16r: negative values
   round toward minus infinity.
2. **lmul saturates only for (−32768)·(−32768)**; mac32/msu32 saturate the product first,
   then the sum. Accumulations are sequential with saturation at every step.
3. **lo16 wraps, hi16 does not round, round16 saturates** (round16 adds 0x8000 with a
   saturating 32-bit add).
4. **mul32x16** must be implemented exactly as the unsigned-low / signed-high decomposition
   of 1.2 (it is used in the LSP→LPC recursion and in the CNG); a plain 48-bit product
   shifted by 15 gives different results.
5. **div32x16(N, d) = min(32767, floor(N/(2d)))** — note the factor 2.
6. **isqrt never sets bit 0** and compares N >= 2r².
7. **The pitch postfilter analyses the normalised excitation but filters the original one**;
   prev_exc receives the unfiltered excitation.
8. **sid_gain / cur_gain are overwritten by every good active frame** (energy and shift for
   the first-SID-lost case). They are only "real" SID values during comfort noise.
9. **CNG second-block lag range is 123..141 (rnd(19))**, gain indices 1..50, lag deltas
   (1, 0, 1, 3), 30 random draws per frame in the order of 4.10.6, cng_seed reset to 12345
   by every active frame (good or erased), not by comfort-noise frames.
10. **6.3 vs 5.3 sign convention for pulses is opposite** (6.3: bit 1 → negative;
    5.3: bit 1 → positive).
11. **The 6.3 pulse train adds the original vector at every multiple of the lag
    (non-recursive); the 5.3 sharpening is recursive.**
12. **Current rate**: set by every successfully started active-frame parse (before the
    validity checks). It selects the 85/170 adaptive gain table, the train-bit extraction,
    the fixed codebook type and the pitch postfilter weight γ. It is never consumed in an
    erased or comfort-noise frame in a way that matters (CNG lags are >= 122, so both rates
    pick the 170 table), so its initial value does not affect the output.
13. **Even-lag versus subframe lag**: the 85/170 table choice, the train-bit presence, the
    pulse-train period, the pitch postfilter search centre and the ACELP sharpening base all
    use the EVEN lag of the subframe pair (L0 or L2, with the delta added only where stated:
    ACELP adds Le−1+D), not the decoded odd-subframe lag.
14. **The classifier uses L2 only** (and searches 120 samples at the end of the frame).
15. **Concealment**: voiced copy is a forward self-referencing copy, attenuation in a
    separate pass, and prev_exc keeps the attenuated signal; unvoiced and 3rd-erasure cases
    clear prev_exc completely.
16. **Postfilter OFF** changes synthesis output scaling (×2 via shl16) and removes pitch
    postfilter, formant postfilter and gain scaling. All filter memories still exist; only
    synthesis memory is used.
17. **The formant postfilter energy Sen is computed on the synthesis output (input of the
    postfilter), the gain unit compares it with energy_q of the postfilter output.**
18. **Frame type substitution happens before anything else**, and a frame with a forbidden
    lag or gain code is handled exactly like an externally erased frame (including the
    first-SID-lost logic when the previous frame was comfort noise: then it becomes an
    untransmitted frame, keeping the old SID parameters).

---------------------------------------------------------------------------------------

## 8. Table index

All files are in `tables/`. Format: lines starting with `#` are a header (name, description,
dimensions, element type, count); then one line per row, values separated by commas,
decimal, signed. Element type int16 means every value fits in a signed 16-bit integer;
int32 likewise.

| file | dimensions | type | used in |
|---|---|---|---|
| lsp_dc.csv | 1 x 10 | int16 | 3 (initial prev_lsp, sid_lsp), 4.2 |
| lsp_band_layout.csv | 3 x 2 | int16 | 4.2 step 2 (informative: bands start at LSP 0, 3, 6 with 3, 3, 4 values) |
| lsp_vq_band0.csv | 256 x 3 | int16 | 4.2 |
| lsp_vq_band1.csv | 256 x 3 | int16 | 4.2 |
| lsp_vq_band2.csv | 256 x 4 | int16 | 4.2 |
| cosine.csv | 512 x 1 | int16 | 4.3.2 (only entries 0..253 can be reached) |
| postfilter_zero_weights.csv | 1 x 10 | int16 | 4.8.1 |
| postfilter_pole_weights.csv | 1 x 10 | int16 | 4.8.1 |
| mpmlq_pulse_count.csv | 1 x 4 | int16 | 4.5.1, 4.10.6 |
| fixed_gain.csv | 1 x 24 | int16 | 4.1 (conceal_gain), 4.5.1, 4.5.2 |
| mpmlq_codebook_size.csv | 1 x 4 | int32 | 4.5.1 |
| mpmlq_combinatorial.csv | 6 x 30 | int32 | 4.5.1 |
| acb_gain_85.csv | 85 x 20 | int16 | 4.4.1 (columns 0..4 only) |
| acb_gain_170.csv | 170 x 20 | int16 | 4.4.1, 4.10.6 (columns 0..4 only) |
| pitch_postfilter_weight.csv | 1 x 2 | int16 | 4.4.3 |
| acelp_pitch_lag_offset.csv | 1 x 170 | int16 | 4.5.2 |
| acelp_pitch_gain.csv | 1 x 170 | int16 | 4.5.2 |
| cng_energy_factor.csv | 1 x 4 | int16 | 4.10.5 (entry 0 only) |
| cng_gain_segment_bounds.csv | 1 x 3 | int32 | 4.10.5 |
| cng_gain_segment_base.csv | 1 x 3 | int16 | 4.10.4, 4.10.5 |
| norm_multiplier.csv | 1 x 16 | int16 | 1.3 vnorm |

Scalar constants that appear inline in the text (not in tables): LSP predictor 12288 and
23552, LSP spacing 256 and 512, LSP clamps 384 and 32256, interpolation weights −8192·(i+1),
erasure attenuation 0x6000 (24576), CNG smoothing 0x7000/0x1000, CNG reciprocal 2979,
CNG clamp ±10000, CNG seed 12345, random generator 521 and 259, postfilter smoothing 0x2000,
tilt factor −16384 and mask 0xfffc, gain-unit constants 0x0800 and 4096, voicing shift 3,
lag bounds 123 (largest valid 7-bit lag code), 58 (train / 85-table threshold), 142 (lag
clamp in the classifier and pitch postfilter).
