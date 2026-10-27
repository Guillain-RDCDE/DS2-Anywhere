//! G.723.1 decoder (6.3 / 5.3 kbit/s, SID and untransmitted frames), used by
//! DSS/DS2 "LP" recordings.
//!
//! Port of FFmpeg's g723_1 decoder (libavcodec/g723_1dec.c, g723_1.c and the
//! celp/acelp helpers it calls), LGPL-2.1-or-later.
//! Copyright (c) 2006 Benjamin Larsson, (c) 2010 Mohamed Naufal Basheer.
//!
//! The arithmetic is FFmpeg's fixed point, bit for bit: C `int` overflow is
//! reproduced with wrapping operations, `int16_t` stores with `as i16`
//! truncation, and the av_sat / av_clip helpers keep their saturation. The
//! output is pinned against FFmpeg's decoder (postfilter on, its default).
//!
//! Every index derived from the bitstream is either validated the way FFmpeg
//! validates it or read through a bounds-checked accessor, so a corrupt or
//! truncated packet never panics.

use crate::tables::g723_1::*;

const SUBFRAMES: usize = 4;
const SUBFRAME_LEN: usize = 60;
const FRAME_LEN: usize = SUBFRAME_LEN * 4;
const LPC_ORDER: usize = 10;
const PITCH_MIN: i32 = 18;
const PITCH_MAX: usize = PITCH_MIN as usize + 127;
const PITCH_ORDER: usize = 5;
const GRID_SIZE: usize = 2;
const PULSE_MAX: usize = 6;
const GAIN_LEVELS: i32 = 24;
const CNG_RANDOM_SEED: i32 = 12345;

/// Samples produced by one G.723.1 frame (30 ms at 8000 Hz).
pub const G723_1_FRAME_SAMPLES: usize = FRAME_LEN;
/// Output sample rate.
pub const G723_1_SAMPLE_RATE: u32 = 8000;
/// Frame size in bytes for each frame type (low 2 bits of the first byte):
/// 6.3 kbit/s, 5.3 kbit/s, SID, untransmitted.
pub const G723_1_FRAME_SIZE: [usize; 4] = [24, 20, 4, 1];

const AUDIO_LEN: usize = FRAME_LEN + LPC_ORDER + PITCH_MAX + 4;
const EXC_LEN: usize = PITCH_MAX + FRAME_LEN + 4;

const DC_LSP: [i16; LPC_ORDER] = [
    0x0c3b, 0x1271, 0x1e0a, 0x2a36, 0x3630, 0x406f, 0x4d28, 0x56f4, 0x638c, 0x6c46,
];

const PULSES: [usize; 4] = [6, 5, 6, 5];

/// Postfilter gain weighting factors scaled by 2^15
const PPF_GAIN_WEIGHT: [i16; 2] = [0x1800, 0x2000];

const PITCH_CONTRIB: [i16; 340] = [
    60, 0, 0, 2489, 60, 0, 0, 5217, 1, 6171, 0, 3953, 0, 10364, 1, 9357, -1, 8843, 1, 9396, 0,
    5794, -1, 10816, 2, 11606, -2, 12072, 0, 8616, 1, 12170, 0, 14440, 0, 7787, -1, 13721, 0,
    18205, 0, 14471, 0, 15807, 1, 15275, 0, 13480, -1, 18375, -1, 0, 1, 11194, -1, 13010, 1, 18836,
    -2, 20354, 1, 16233, -1, 0, 60, 0, 0, 12130, 0, 13385, 1, 17834, 1, 20875, 0, 21996, 1, 0, 1,
    18277, -1, 21321, 1, 13738, -1, 19094, -1, 20387, -1, 0, 0, 21008, 60, 0, -2, 22807, 0, 15900,
    1, 0, 0, 17989, -1, 22259, 1, 24395, 1, 23138, 0, 23948, 1, 22997, 2, 22604, -1, 25942, 0,
    26246, 1, 25321, 0, 26423, 0, 24061, 0, 27247, 60, 0, -1, 25572, 1, 23918, 1, 25930, 2, 26408,
    -1, 19049, 1, 27357, -1, 24538, 60, 0, -1, 25093, 0, 28549, 1, 0, 0, 22793, -1, 25659, 0,
    29377, 0, 30276, 0, 26198, 1, 22521, -1, 28919, 0, 27384, 1, 30162, -1, 0, 0, 24237, -1, 30062,
    0, 21763, 1, 30917, 60, 0, 0, 31284, 0, 29433, 1, 26821, 1, 28655, 0, 31327, 2, 30799, 1,
    31389, 0, 32322, 1, 31760, -2, 31830, 0, 26936, -1, 31180, 1, 30875, 0, 27873, -1, 30429, 1,
    31050, 0, 0, 0, 31912, 1, 31611, 0, 31565, 0, 25557, 0, 31357, 60, 0, 1, 29536, 1, 28985, -1,
    26984, -1, 31587, 2, 30836, -2, 31133, 0, 30243, -1, 30742, -1, 32090, 60, 0, 2, 30902, 60, 0,
    0, 30027, 0, 29042, 60, 0, 0, 31756, 0, 24553, 0, 25636, -2, 30501, 60, 0, -1, 29617, 0, 30649,
    60, 0, 0, 29274, 2, 30415, 0, 27480, 0, 31213, -1, 28147, 0, 30600, 1, 31652, 2, 29068, 60, 0,
    1, 28571, 1, 28730, 1, 31422, 0, 28257, 0, 24797, 60, 0, 0, 0, 60, 0, 0, 22105, 0, 27852, 60,
    0, 60, 0, -1, 24214, 0, 24642, 0, 23305, 60, 0, 60, 0, 1, 22883, 0, 21601, 60, 0, 2, 25650, 60,
    0, -2, 31253, -2, 25144, 0, 17998,
];

/// Size of the MP-MLQ fixed excitation codebooks
const MAX_POS: [i32; 4] = [593775, 142506, 593775, 142506];

/// 0.65^i (zero part) and 0.75^i (pole part) scaled by 2^15
const POSTFILTER_TBL: [[i16; LPC_ORDER]; 2] = [
    [21299, 13844, 8999, 5849, 3802, 2471, 1606, 1044, 679, 441],
    [
        24576, 18432, 13824, 10368, 7776, 5832, 4374, 3281, 2460, 1845,
    ],
];

const CNG_ADAPTIVE_CB_LAG: [i32; 4] = [1, 0, 1, 3];
const CNG_FILT: [i32; 4] = [273, 998, 499, 333];
const CNG_BSEG: [i32; 3] = [2048, 18432, 231233];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FrameType {
    Active,
    Sid,
    Untransmitted,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rate {
    R6300 = 0,
    R5300 = 1,
}

#[derive(Clone, Copy, Default, Debug)]
struct Subframe {
    ad_cb_lag: i32,
    ad_cb_gain: i32,
    dirac_train: i32,
    pulse_sign: i32,
    grid_index: i32,
    amp_index: i32,
    pulse_pos: i32,
}

#[derive(Clone, Copy, Default)]
struct PpfParam {
    index: i32,
    opt_gain: i16,
    sc_gain: i16,
}

// ---------------------------------------------------------------------------
// FFmpeg arithmetic helpers
// ---------------------------------------------------------------------------

#[inline]
fn clip_i16(a: i32) -> i16 {
    a.clamp(-32768, 32767) as i16
}

#[inline]
fn clipl_i32(a: i64) -> i32 {
    a.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

#[inline]
fn sat_add32(a: i32, b: i32) -> i32 {
    clipl_i32(a as i64 + b as i64)
}

#[inline]
fn sat_dadd32(a: i32, b: i32) -> i32 {
    sat_add32(a, sat_add32(b, b))
}

/// av_log2 (log2 of 0 is 0).
#[inline]
fn av_log2(v: u32) -> i32 {
    31 - (v | 1).leading_zeros() as i32
}

/// C division where FFmpeg never divides by zero on a valid path; a zero
/// divisor (corrupt input only) yields 0 instead of trapping.
#[inline]
fn div(a: i32, b: i32) -> i32 {
    if b == 0 {
        0
    } else {
        a.wrapping_div(b)
    }
}

/// C `a << s` on int with gcc semantics for the shift range FFmpeg uses.
#[inline]
fn shl(a: i32, s: i32) -> i32 {
    a.wrapping_shl(s as u32)
}

/// C `a >> s` on int (arithmetic).
#[inline]
fn shr(a: i32, s: i32) -> i32 {
    a.wrapping_shr(s as u32)
}

/// ff_sqrt from libavcodec/mathops.h (CONFIG_SMALL off).
fn ff_sqrt(a: u32) -> u32 {
    if a < 255 {
        return (SQRT_TAB[(a + 1) as usize] as u32).wrapping_sub(1) >> 4;
    }
    let b: u32 = if a < (1 << 12) {
        (SQRT_TAB[(a >> 4) as usize] as u32) >> 2
    } else if a < (1 << 14) {
        (SQRT_TAB[(a >> 6) as usize] as u32) >> 1
    } else if a < (1 << 16) {
        SQRT_TAB[(a >> 8) as usize] as u32
    } else {
        let s = av_log2(a >> 16) >> 1;
        let c = a >> (s + 2);
        let t = SQRT_TAB.get((c >> (s + 8)) as usize).copied().unwrap_or(1) as u32;
        let t = t.max(1);
        c / t + (t << s)
    };
    b.wrapping_sub((a < b.wrapping_mul(b)) as u32)
}

/// Bitexact sqrt(val/2), truncated to int16 as FFmpeg's return type does.
#[inline]
fn square_root(val: u32) -> i16 {
    ((ff_sqrt(val.wrapping_shl(1)) >> 1) & !1u32) as i16
}

/// ff_g723_1_normalize_bits
#[inline]
fn normalize_bits(num: i32, width: i32) -> i32 {
    width - av_log2(num as u32) - 1
}

/// ff_g723_1_dot_product over `buf[a..a+len]` and `buf2[b..b+len]`.
/// Out-of-range windows (unreachable from FFmpeg-valid state) give 0.
fn dot_product(x: &[i16], a: isize, y: &[i16], b: isize, len: usize) -> i32 {
    if a < 0 || b < 0 {
        return 0;
    }
    let (a, b) = (a as usize, b as usize);
    let (Some(xs), Some(ys)) = (x.get(a..a + len), y.get(b..b + len)) else {
        return 0;
    };
    let sum: i64 = xs.iter().zip(ys).map(|(&p, &q)| p as i64 * q as i64).sum();
    let sum = sum as i32;
    sat_add32(sum, sum)
}

/// ff_g723_1_scale_vector: returns the scale, writes `dst[..len]`.
fn scale_vector(dst: &mut [i16], vector: &[i16], len: usize) -> i32 {
    let mut max: i32 = 0;
    for &v in &vector[..len] {
        max |= (v as i32).abs();
    }
    let bits = (14 - av_log2(max as u32)).max(0);
    for i in 0..len {
        dst[i] = (shl(vector[i] as i32, bits) >> 3) as i16;
    }
    bits - 3
}

/// ff_acelp_weighted_vector_sum (one element).
#[inline]
fn weighted(a: i16, b: i16, wa: i16, wb: i16, rounder: i16, shift: i32) -> i16 {
    let s = (a as i32)
        .wrapping_mul(wa as i32)
        .wrapping_add((b as i32).wrapping_mul(wb as i32))
        .wrapping_add(rounder as i32);
    clip_i16(s >> shift)
}

/// MULL2: bitexact 2ab scaled by 1/2^16 (C int arithmetic).
#[inline]
fn mull2(a: i32, b: i32) -> i32 {
    (a >> 16)
        .wrapping_mul(b)
        .wrapping_mul(2)
        .wrapping_add(((a & 0xffff).wrapping_mul(b)) >> 15)
}

// ---------------------------------------------------------------------------
// LSP handling (g723_1.c)
// ---------------------------------------------------------------------------

fn lsp2lpc(lpc: &mut [i16]) {
    let mut f1 = [0i32; LPC_ORDER / 2 + 1];
    let mut f2 = [0i32; LPC_ORDER / 2 + 1];

    for v in lpc.iter_mut().take(LPC_ORDER) {
        let index = ((*v >> 7) & 0x1ff) as usize;
        let offset = (*v & 0x7f) as i32;
        let c0 = COS_TAB[index] as i32;
        let c1 = COS_TAB[index + 1] as i32;
        let temp1 = c0.wrapping_mul(1 << 16);
        let temp2 = (c1 - c0).wrapping_mul(((offset << 8) + 0x80) << 1);
        *v = (sat_dadd32(1 << 15, temp1.wrapping_add(temp2)) >> 16).wrapping_neg() as i16;
    }

    let l = |k: usize| lpc[k] as i32;
    f1[0] = 1 << 28;
    f1[1] = (l(0) + l(2)).wrapping_mul(1 << 14);
    f1[2] = l(0).wrapping_mul(l(2)).wrapping_add(2 << 28);
    f2[0] = 1 << 28;
    f2[1] = (l(1) + l(3)).wrapping_mul(1 << 14);
    f2[2] = l(1).wrapping_mul(l(3)).wrapping_add(2 << 28);

    for i in 2..LPC_ORDER / 2 {
        let a = l(2 * i);
        let b = l(2 * i + 1);
        f1[i + 1] = clipl_i32(f1[i - 1] as i64 + mull2(f1[i], a) as i64);
        f2[i + 1] = clipl_i32(f2[i - 1] as i64 + mull2(f2[i], b) as i64);
        let mut j = i;
        while j >= 2 {
            f1[j] = mull2(f1[j - 1], a)
                .wrapping_add(f1[j] >> 1)
                .wrapping_add(f1[j - 2] >> 1);
            f2[j] = mull2(f2[j - 1], b)
                .wrapping_add(f2[j] >> 1)
                .wrapping_add(f2[j - 2] >> 1);
            j -= 1;
        }
        f1[0] >>= 1;
        f2[0] >>= 1;
        f1[1] = (shr(a.wrapping_mul(65536), i as i32).wrapping_add(f1[1])) >> 1;
        f2[1] = (shr(b.wrapping_mul(65536), i as i32).wrapping_add(f2[1])) >> 1;
    }

    for i in 0..LPC_ORDER / 2 {
        let ff1 = f1[i + 1].wrapping_add(f1[i]) as i64;
        let ff2 = f2[i + 1].wrapping_sub(f2[i]) as i64;
        lpc[i] = (clipl_i32((ff1 + ff2) * 8 + (1 << 15)) >> 16) as i16;
        lpc[LPC_ORDER - i - 1] = (clipl_i32((ff1 - ff2) * 8 + (1 << 15)) >> 16) as i16;
    }
}

fn lsp_interpolate(lpc: &mut [i16; SUBFRAMES * LPC_ORDER], cur: &[i16; 10], prev: &[i16; 10]) {
    for i in 0..LPC_ORDER {
        lpc[i] = weighted(cur[i], prev[i], 4096, 12288, 1 << 13, 14);
        lpc[LPC_ORDER + i] = weighted(cur[i], prev[i], 8192, 8192, 1 << 13, 14);
        lpc[2 * LPC_ORDER + i] = weighted(cur[i], prev[i], 12288, 4096, 1 << 13, 14);
    }
    lpc[3 * LPC_ORDER..].copy_from_slice(cur);
    for chunk in lpc.chunks_mut(LPC_ORDER) {
        lsp2lpc(chunk);
    }
}

fn inverse_quant(prev_lsp: &[i16; 10], lsp_index: &mut [u8; 3], bad_frame: bool) -> [i16; 10] {
    let (min_dist, pred) = if !bad_frame {
        (0x100, 12288)
    } else {
        *lsp_index = [0; 3];
        (0x200, 23552)
    };
    let b0 = &LSP_BAND0[lsp_index[0] as usize];
    let b1 = &LSP_BAND1[lsp_index[1] as usize];
    let b2 = &LSP_BAND2[lsp_index[2] as usize];
    let mut cur: [i16; 10] = [
        b0[0], b0[1], b0[2], b1[0], b1[1], b1[2], b2[0], b2[1], b2[2], b2[3],
    ];

    for i in 0..LPC_ORDER {
        let temp = ((prev_lsp[i] as i32 - DC_LSP[i] as i32) * pred + (1 << 14)) >> 15;
        cur[i] = (cur[i] as i32 + DC_LSP[i] as i32 + temp) as i16;
    }

    let mut stable = false;
    for _ in 0..LPC_ORDER {
        cur[0] = cur[0].max(0x180);
        cur[LPC_ORDER - 1] = cur[LPC_ORDER - 1].min(0x7e00);
        for j in 1..LPC_ORDER {
            let mut temp = min_dist + cur[j - 1] as i32 - cur[j] as i32;
            if temp > 0 {
                temp >>= 1;
                cur[j - 1] = (cur[j - 1] as i32 - temp) as i16;
                cur[j] = (cur[j] as i32 + temp) as i16;
            }
        }
        stable = true;
        for j in 1..LPC_ORDER {
            let temp = cur[j - 1] as i32 + min_dist - cur[j] as i32 - 4;
            if temp > 0 {
                stable = false;
                break;
            }
        }
        if stable {
            break;
        }
    }
    if !stable {
        cur = *prev_lsp;
    }
    cur
}

// ---------------------------------------------------------------------------
// Excitation (g723_1.c / g723_1dec.c)
// ---------------------------------------------------------------------------

/// ff_g723_1_gen_acb_excitation; the history is `buf` seen from `base`
/// (FFmpeg's `prev_excitation` pointer). It may read slightly before `base`
/// (comfort noise uses lags up to PITCH_MAX), still inside `buf`.
fn gen_acb_excitation(
    buf: &[i16],
    base: usize,
    pitch_lag: i32,
    subfrm: &Subframe,
    rate: Rate,
) -> [i16; SUBFRAME_LEN] {
    let mut residual = [0i16; SUBFRAME_LEN + PITCH_ORDER - 1];
    let lag = pitch_lag + subfrm.ad_cb_lag - 1;
    let at = |i: isize| -> i16 {
        if i < 0 {
            0
        } else {
            buf.get(i as usize).copied().unwrap_or(0)
        }
    };

    // ff_g723_1_get_residual
    let mut offset =
        base as isize + (PITCH_MAX as isize - (PITCH_ORDER / 2) as isize - lag as isize);
    residual[0] = at(offset);
    residual[1] = at(offset + 1);
    offset += 2;
    let lag_m = lag.max(1) as isize;
    for i in 2..SUBFRAME_LEN + PITCH_ORDER - 1 {
        residual[i] = at(offset + (i as isize - 2) % lag_m);
    }

    let table: &[i16] = if rate == Rate::R6300 && pitch_lag < SUBFRAME_LEN as i32 - 2 {
        &ADAPTIVE_CB_GAIN85
    } else {
        &ADAPTIVE_CB_GAIN170
    };
    let start = (subfrm.ad_cb_gain.max(0) as usize).saturating_mul(20);
    let cb: [i16; PITCH_ORDER] = match table.get(start..start + PITCH_ORDER) {
        Some(s) => [s[0], s[1], s[2], s[3], s[4]],
        None => [0; PITCH_ORDER],
    };

    let mut vector = [0i16; SUBFRAME_LEN];
    for i in 0..SUBFRAME_LEN {
        let mut sum: i64 = 0;
        for k in 0..PITCH_ORDER {
            sum += residual[i + k] as i64 * cb[k] as i64;
        }
        let sum = sum as i32;
        vector[i] = (sat_dadd32(1 << 15, sat_add32(sum, sum)) >> 16) as i16;
    }
    vector
}

/// ff_g723_1_gen_dirac_train
fn gen_dirac_train(buf: &mut [i16], pitch_lag: i32) {
    if pitch_lag <= 0 {
        return;
    }
    let mut vector = [0i16; SUBFRAME_LEN];
    vector.copy_from_slice(&buf[..SUBFRAME_LEN]);
    let mut i = pitch_lag as usize;
    while i < SUBFRAME_LEN {
        for j in 0..SUBFRAME_LEN - i {
            buf[i + j] = buf[i + j].wrapping_add(vector[j]);
        }
        i += pitch_lag as usize;
    }
}

/// gen_fcb_excitation. FFmpeg's 5.3k path may write up to 4 samples past the
/// subframe (into the next one, which is then cleared, or into padding); a
/// 64-sample scratch vector keeps that behavior without touching neighbors.
fn gen_fcb_excitation(subfrm: &Subframe, rate: Rate, pitch_lag: i32, index: usize) -> [i16; 64] {
    let mut vector = [0i16; 64];
    let gain_at = |k: i32| FIXED_CB_GAIN.get(k as usize).copied().unwrap_or(0) as i32;

    if rate == Rate::R6300 {
        if subfrm.pulse_pos >= MAX_POS[index] || subfrm.pulse_pos < 0 {
            return vector;
        }
        let mut j = PULSE_MAX - PULSES[index];
        let mut temp = subfrm.pulse_pos;
        let grid = (subfrm.grid_index & 1) as usize;
        let gain = gain_at(subfrm.amp_index);
        for i in 0..SUBFRAME_LEN / GRID_SIZE {
            temp -= COMBINATORIAL_TABLE[j][i];
            if temp >= 0 {
                continue;
            }
            temp += COMBINATORIAL_TABLE[j][i];
            j += 1;
            let v = if subfrm.pulse_sign & (1 << (PULSE_MAX - j)) != 0 {
                -gain
            } else {
                gain
            };
            vector[grid + GRID_SIZE * i] = v as i16;
            if j == PULSE_MAX {
                break;
            }
        }
        if subfrm.dirac_train == 1 {
            gen_dirac_train(&mut vector, pitch_lag);
        }
    } else {
        let cb_gain = gain_at(subfrm.amp_index);
        let cb_shift = (subfrm.grid_index & 1) as usize;
        let mut cb_sign = subfrm.pulse_sign;
        let mut cb_pos = subfrm.pulse_pos;
        let mut i = 0;
        while i < 8 {
            let offset = (((cb_pos & 7) as usize) << 3) + cb_shift + i;
            vector[offset] = if cb_sign & 1 != 0 { cb_gain } else { -cb_gain } as i16;
            cb_pos >>= 3;
            cb_sign >>= 1;
            i += 2;
        }

        // Enhance harmonic components
        let gi = (subfrm.ad_cb_gain.max(0) as usize) << 1;
        let contrib = |k: usize| PITCH_CONTRIB.get(k).copied().unwrap_or(60) as i32;
        let lag = contrib(gi) + pitch_lag + subfrm.ad_cb_lag - 1;
        let beta = contrib(gi + 1);
        if lag > 0 && lag < SUBFRAME_LEN as i32 - 2 {
            let lag = lag as usize;
            for i in lag..SUBFRAME_LEN {
                let add = beta.wrapping_mul(vector[i - lag] as i32) >> 15;
                vector[i] = (vector[i] as i32).wrapping_add(add) as i16;
            }
        }
    }
    vector
}

/// autocorr_max over `buf` seen from `pos`.
fn autocorr_max(
    buf: &[i16],
    pos: usize,
    offset: i32,
    ccr_max: &mut i32,
    pitch_lag: i32,
    length: usize,
    dir: i32,
) -> i32 {
    let mut lag = 0;
    let pitch_lag = pitch_lag.min(PITCH_MAX as i32 - 3);
    let limit = if dir > 0 {
        (FRAME_LEN as i32 + PITCH_MAX as i32 - offset - length as i32).min(pitch_lag + 3)
    } else {
        pitch_lag + 3
    };
    let mut i = pitch_lag - 3;
    while i <= limit {
        let ccr = dot_product(
            buf,
            pos as isize,
            buf,
            pos as isize + (dir * i) as isize,
            length,
        );
        if ccr > *ccr_max {
            *ccr_max = ccr;
            lag = i;
        }
        i += 1;
    }
    lag
}

fn comp_ppf_gains(lag: i32, rate: Rate, tgt_eng: i32, ccr: i32, res_eng: i32) -> PpfParam {
    let w = PPF_GAIN_WEIGHT[rate as usize] as i32;
    let mut ppf = PpfParam {
        index: lag,
        ..Default::default()
    };

    let temp1 = tgt_eng.wrapping_mul(res_eng) >> 1;
    let temp2 = shl(ccr.wrapping_mul(ccr), 1);

    if temp2 > temp1 {
        if ccr >= res_eng {
            ppf.opt_gain = w as i16;
        } else {
            ppf.opt_gain = (div(shl(ccr, 15), res_eng).wrapping_mul(w) >> 15) as i16;
        }
        let og = ppf.opt_gain as i32;
        let temp1 = shl(tgt_eng, 15).wrapping_add(shl(ccr.wrapping_mul(og), 1));
        let temp2 = (og.wrapping_mul(og) >> 15).wrapping_mul(res_eng);
        let pf_residual = sat_add32(temp1, temp2.wrapping_add(1 << 15)) >> 16;

        let t = if tgt_eng >= shl(pf_residual, 1) {
            0x7fff
        } else {
            div(shl(tgt_eng, 14), pf_residual)
        };
        ppf.sc_gain = square_root(shl(t, 16) as u32);
    } else {
        ppf.opt_gain = 0;
        ppf.sc_gain = 0x7fff;
    }

    ppf.opt_gain = clip_i16((ppf.opt_gain as i32).wrapping_mul(ppf.sc_gain as i32) >> 15);
    ppf
}

fn sid_gain_to_lsp_index(gain: i32) -> i32 {
    if gain < 0x10 {
        gain << 6
    } else if gain < 0x20 {
        (gain - 8) << 7
    } else {
        (gain - 20) << 8
    }
}

#[inline]
fn cng_rand(state: &mut i32, base: i32) -> i32 {
    *state = (state.wrapping_mul(521).wrapping_add(259)) & 0xFFFF;
    ((*state & 0x7FFF) * base) >> 15
}

/// Minimal LSB-first bit reader (FFmpeg BITSTREAM_READER_LE); reads past the
/// end return zero bits.
struct BitReaderLe<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> BitReaderLe<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn get(&mut self, n: u32) -> i32 {
        let mut v: u32 = 0;
        for i in 0..n {
            let p = self.pos + i as usize;
            let bit = self
                .buf
                .get(p >> 3)
                .map(|b| (b >> (p & 7)) & 1)
                .unwrap_or(0);
            v |= (bit as u32) << i;
        }
        self.pos += n as usize;
        v as i32
    }
}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

/// G.723.1 decoder, mono, bit-exact with FFmpeg's `g723_1` decoder.
#[derive(Clone)]
pub struct G7231Decoder {
    postfilter: bool,

    subframe: [Subframe; 4],
    cur_frame_type: FrameType,
    past_frame_type: FrameType,
    cur_rate: Rate,
    lsp_index: [u8; 3],
    pitch_lag: [i32; 2],
    erased_frames: i32,

    prev_lsp: [i16; LPC_ORDER],
    sid_lsp: [i16; LPC_ORDER],
    prev_excitation: [i16; PITCH_MAX],
    excitation: [i16; EXC_LEN],
    synth_mem: [i16; LPC_ORDER],
    fir_mem: [i16; LPC_ORDER],
    iir_mem: [i32; LPC_ORDER],

    random_seed: i32,
    cng_random_seed: i32,
    interp_index: i32,
    interp_gain: i32,
    sid_gain: i32,
    cur_gain: i32,
    reflection_coef: i32,
    pf_gain: i32,
    audio: [i16; AUDIO_LEN],
}

impl Default for G7231Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl G7231Decoder {
    /// New decoder with FFmpeg's defaults (postfilter enabled).
    pub fn new() -> Self {
        let mut d = Self {
            postfilter: true,
            subframe: [Subframe::default(); 4],
            cur_frame_type: FrameType::Active,
            past_frame_type: FrameType::Sid,
            cur_rate: Rate::R6300,
            lsp_index: [0; 3],
            pitch_lag: [0; 2],
            erased_frames: 0,
            prev_lsp: DC_LSP,
            sid_lsp: DC_LSP,
            prev_excitation: [0; PITCH_MAX],
            excitation: [0; EXC_LEN],
            synth_mem: [0; LPC_ORDER],
            fir_mem: [0; LPC_ORDER],
            iir_mem: [0; LPC_ORDER],
            random_seed: 0,
            cng_random_seed: CNG_RANDOM_SEED,
            interp_index: 0,
            interp_gain: 0,
            sid_gain: 0,
            cur_gain: 0,
            reflection_coef: 0,
            pf_gain: 1 << 12,
            audio: [0; AUDIO_LEN],
        };
        d.reset();
        d
    }

    /// Enable or disable the postfilter (FFmpeg option `postfilter`, default on).
    pub fn set_postfilter(&mut self, on: bool) {
        self.postfilter = on;
    }

    /// Flush all state (FFmpeg's flush callback, e.g. after a seek).
    pub fn reset(&mut self) {
        let postfilter = self.postfilter;
        *self = Self {
            postfilter,
            subframe: [Subframe::default(); 4],
            cur_frame_type: FrameType::Active,
            past_frame_type: FrameType::Sid,
            cur_rate: Rate::R6300,
            lsp_index: [0; 3],
            pitch_lag: [0; 2],
            erased_frames: 0,
            prev_lsp: DC_LSP,
            sid_lsp: DC_LSP,
            prev_excitation: [0; PITCH_MAX],
            excitation: [0; EXC_LEN],
            synth_mem: [0; LPC_ORDER],
            fir_mem: [0; LPC_ORDER],
            iir_mem: [0; LPC_ORDER],
            random_seed: 0,
            cng_random_seed: CNG_RANDOM_SEED,
            interp_index: 0,
            interp_gain: 0,
            sid_gain: 0,
            cur_gain: 0,
            reflection_coef: 0,
            pf_gain: 1 << 12,
            audio: [0; AUDIO_LEN],
        };
    }

    /// Size in bytes of the frame whose first byte is `first_byte`.
    pub fn frame_size(first_byte: u8) -> usize {
        G723_1_FRAME_SIZE[(first_byte & 3) as usize]
    }

    /// Decode one packet (one G.723.1 frame). Returns 240 samples at 8000 Hz,
    /// or an empty vector when the packet is shorter than its frame type
    /// requires (FFmpeg skips such packets without output).
    pub fn decode_frame(&mut self, pkt: &[u8]) -> Vec<i16> {
        let Some(&first) = pkt.first() else {
            return Vec::new();
        };
        if pkt.len() < Self::frame_size(first) {
            return Vec::new();
        }
        let mut out = vec![0i16; FRAME_LEN];
        self.decode_into(pkt, &mut out);
        out
    }

    fn unpack_bitstream(&mut self, buf: &[u8]) -> Result<(), ()> {
        let mut gb = BitReaderLe::new(buf);
        let info_bits = gb.get(2);

        if info_bits == 3 {
            self.cur_frame_type = FrameType::Untransmitted;
            return Ok(());
        }

        self.lsp_index[2] = gb.get(8) as u8;
        self.lsp_index[1] = gb.get(8) as u8;
        self.lsp_index[0] = gb.get(8) as u8;

        if info_bits == 2 {
            self.cur_frame_type = FrameType::Sid;
            self.subframe[0].amp_index = gb.get(6);
            return Ok(());
        }

        self.cur_rate = if info_bits != 0 {
            Rate::R5300
        } else {
            Rate::R6300
        };
        self.cur_frame_type = FrameType::Active;

        self.pitch_lag[0] = gb.get(7);
        if self.pitch_lag[0] > 123 {
            return Err(());
        }
        self.pitch_lag[0] += PITCH_MIN;
        self.subframe[1].ad_cb_lag = gb.get(2);

        self.pitch_lag[1] = gb.get(7);
        if self.pitch_lag[1] > 123 {
            return Err(());
        }
        self.pitch_lag[1] += PITCH_MIN;
        self.subframe[3].ad_cb_lag = gb.get(2);
        self.subframe[0].ad_cb_lag = 1;
        self.subframe[2].ad_cb_lag = 1;

        for i in 0..SUBFRAMES {
            let mut temp = gb.get(12);
            let mut ad_cb_len = 170;
            self.subframe[i].dirac_train = 0;
            if self.cur_rate == Rate::R6300 && self.pitch_lag[i >> 1] < SUBFRAME_LEN as i32 - 2 {
                self.subframe[i].dirac_train = temp >> 11;
                temp &= 0x7ff;
                ad_cb_len = 85;
            }
            self.subframe[i].ad_cb_gain = temp / GAIN_LEVELS;
            if self.subframe[i].ad_cb_gain < ad_cb_len {
                self.subframe[i].amp_index = temp - self.subframe[i].ad_cb_gain * GAIN_LEVELS;
            } else {
                return Err(());
            }
        }

        for i in 0..SUBFRAMES {
            self.subframe[i].grid_index = gb.get(1);
        }

        if self.cur_rate == Rate::R6300 {
            gb.get(1); // reserved bit

            let mut temp = gb.get(13);
            self.subframe[0].pulse_pos = temp / 810;
            temp -= self.subframe[0].pulse_pos * 810;
            self.subframe[1].pulse_pos = temp / 90;
            temp -= self.subframe[1].pulse_pos * 90;
            self.subframe[2].pulse_pos = temp / 9;
            self.subframe[3].pulse_pos = temp - self.subframe[2].pulse_pos * 9;

            self.subframe[0].pulse_pos = (self.subframe[0].pulse_pos << 16) + gb.get(16);
            self.subframe[1].pulse_pos = (self.subframe[1].pulse_pos << 14) + gb.get(14);
            self.subframe[2].pulse_pos = (self.subframe[2].pulse_pos << 16) + gb.get(16);
            self.subframe[3].pulse_pos = (self.subframe[3].pulse_pos << 14) + gb.get(14);

            self.subframe[0].pulse_sign = gb.get(6);
            self.subframe[1].pulse_sign = gb.get(5);
            self.subframe[2].pulse_sign = gb.get(6);
            self.subframe[3].pulse_sign = gb.get(5);
        } else {
            for i in 0..SUBFRAMES {
                self.subframe[i].pulse_pos = gb.get(12);
            }
            for i in 0..SUBFRAMES {
                self.subframe[i].pulse_sign = gb.get(4);
            }
        }
        Ok(())
    }

    fn comp_ppf_coeff(&self, offset: usize, pitch_lag: i32, rate: Rate) -> PpfParam {
        let audio = &self.audio[..];
        let mut energy = [0i32; 5];
        let pos = LPC_ORDER + offset;
        let p = pos as isize;
        let fwd_lag = autocorr_max(
            audio,
            pos,
            offset as i32,
            &mut energy[1],
            pitch_lag,
            SUBFRAME_LEN,
            1,
        );
        let back_lag = autocorr_max(
            audio,
            pos,
            offset as i32,
            &mut energy[3],
            pitch_lag,
            SUBFRAME_LEN,
            -1,
        );

        let mut ppf = PpfParam {
            index: 0,
            opt_gain: 0,
            sc_gain: 0x7fff,
        };
        if back_lag == 0 && fwd_lag == 0 {
            return ppf;
        }

        energy[0] = dot_product(audio, p, audio, p, SUBFRAME_LEN);
        if fwd_lag != 0 {
            let q = p + fwd_lag as isize;
            energy[2] = dot_product(audio, q, audio, q, SUBFRAME_LEN);
        }
        if back_lag != 0 {
            let q = p - back_lag as isize;
            energy[4] = dot_product(audio, q, audio, q, SUBFRAME_LEN);
        }

        let mut temp1 = 0;
        for &e in &energy {
            temp1 = temp1.max(e);
        }
        let scale = normalize_bits(temp1, 31) as i16 as i32;
        for e in energy.iter_mut() {
            *e = shl(*e, scale) >> 16;
        }

        if fwd_lag != 0 && back_lag == 0 {
            ppf = comp_ppf_gains(fwd_lag, rate, energy[0], energy[1], energy[2]);
        } else if fwd_lag == 0 {
            ppf = comp_ppf_gains(-back_lag, rate, energy[0], energy[3], energy[4]);
        } else {
            let t1 = energy[4]
                .wrapping_mul((energy[1].wrapping_mul(energy[1]).wrapping_add(1 << 14)) >> 15);
            let t2 = energy[2]
                .wrapping_mul((energy[3].wrapping_mul(energy[3]).wrapping_add(1 << 14)) >> 15);
            ppf = if t1 >= t2 {
                comp_ppf_gains(fwd_lag, rate, energy[0], energy[1], energy[2])
            } else {
                comp_ppf_gains(-back_lag, rate, energy[0], energy[3], energy[4])
            };
        }
        ppf
    }

    /// comp_interp_index: returns (index, exc_eng, scale).
    fn comp_interp_index(&mut self, pitch_lag: i32) -> (i32, i32, i32) {
        let offset = PITCH_MAX + 2 * SUBFRAME_LEN;
        let n = FRAME_LEN + PITCH_MAX;
        let scale = {
            let (dst, src) = (
                &mut self.audio[LPC_ORDER..LPC_ORDER + n],
                &self.excitation[..n],
            );
            scale_vector(dst, src, n)
        };
        let audio = &self.audio[..];
        let pos = LPC_ORDER + offset;
        let p = pos as isize;

        let mut ccr = 0;
        let index = autocorr_max(
            audio,
            pos,
            offset as i32,
            &mut ccr,
            pitch_lag,
            SUBFRAME_LEN * 2,
            -1,
        );
        let ccr = sat_add32(ccr, 1 << 15) >> 16;

        let tgt_eng = dot_product(audio, p, audio, p, SUBFRAME_LEN * 2);
        let exc_eng = sat_add32(tgt_eng, 1 << 15) >> 16;

        if ccr <= 0 {
            return (0, exc_eng, scale);
        }

        let q = p - index as isize;
        let best_eng = dot_product(audio, q, audio, q, SUBFRAME_LEN * 2);
        let best_eng = sat_add32(best_eng, 1 << 15) >> 16;

        let temp = best_eng.wrapping_mul(exc_eng) >> 3;
        if temp < ccr.wrapping_mul(ccr) {
            (index, exc_eng, scale)
        } else {
            (0, exc_eng, scale)
        }
    }

    /// residual_interp, output into audio[LPC_ORDER..].
    fn residual_interp(&mut self) {
        let lag = self.interp_index;
        let gain = self.interp_gain;
        let out = &mut self.audio[LPC_ORDER..LPC_ORDER + FRAME_LEN];
        if lag > 0 && (lag as usize) <= PITCH_MAX {
            let lag = lag as usize;
            for i in 0..lag {
                out[i] = ((self.excitation[PITCH_MAX + i - lag] as i32 * 3) >> 2) as i16;
            }
            for k in lag..FRAME_LEN {
                out[k] = out[k - lag];
            }
        } else {
            for o in out.iter_mut() {
                self.random_seed =
                    self.random_seed.wrapping_mul(521).wrapping_add(259) as i16 as i32;
                *o = (gain.wrapping_mul(self.random_seed) >> 15) as i16;
            }
            self.excitation[..FRAME_LEN + PITCH_MAX].fill(0);
        }
    }

    fn estimate_sid_gain(&self) -> i32 {
        let sid_gain = self.sid_gain;
        let mut shift = 16i32.wrapping_sub(self.cur_gain.wrapping_mul(2));
        let t: i32 = if shift > 0 {
            if sid_gain == 0 {
                0
            } else if shift >= 31 || (((sid_gain as u32) << shift) as i32) >> shift != sid_gain {
                if sid_gain < 0 {
                    i32::MIN
                } else {
                    i32::MAX
                }
            } else {
                sid_gain.wrapping_mul(1 << shift)
            }
        } else if shift < -31 {
            if sid_gain < 0 {
                -1
            } else {
                0
            }
        } else {
            sid_gain >> (-shift)
        };
        let x = clipl_i32((t as i64 * CNG_FILT[0] as i64) >> 16);

        if x >= CNG_BSEG[2] {
            return 0x3F;
        }

        let seg;
        if x >= CNG_BSEG[1] {
            shift = 4;
            seg = 3;
        } else {
            shift = 3;
            seg = (x >= CNG_BSEG[0]) as i32;
        }
        let seg2 = seg.min(3);

        let mut val = 1i32 << shift;
        let mut val_add = val >> 1;
        for _ in 0..shift {
            let mut t = seg * 32 + (val << seg2);
            t = t.wrapping_mul(t);
            if x >= t {
                val += val_add;
            } else {
                val -= val_add;
            }
            val_add >>= 1;
        }

        let t = seg * 32 + (val << seg2);
        let y = t.wrapping_mul(t).wrapping_sub(x);
        if y <= 0 {
            let t = seg * 32 + ((val + 1) << seg2);
            let t = t.wrapping_mul(t).wrapping_sub(x);
            val += (seg2 - 1) * 16;
            if t >= y {
                val += 1;
            }
        } else {
            let t = seg * 32 + ((val - 1) << seg2);
            let t = t.wrapping_mul(t).wrapping_sub(x);
            val += (seg2 - 1) * 16;
            if t >= y {
                val -= 1;
            }
        }
        val
    }

    fn generate_noise(&mut self) {
        let mut off = [0usize; SUBFRAMES];
        let mut signs = [0i32; SUBFRAMES / 2 * 11];
        let mut pos = [0usize; SUBFRAMES / 2 * 11];
        let mut tmp = [0i32; SUBFRAME_LEN * 2];
        let seed = &mut self.cng_random_seed;

        self.pitch_lag[0] = cng_rand(seed, 21) + 123;
        self.pitch_lag[1] = cng_rand(seed, 19) + 123;

        for i in 0..SUBFRAMES {
            self.subframe[i].ad_cb_gain = cng_rand(seed, 50) + 1;
            self.subframe[i].ad_cb_lag = CNG_ADAPTIVE_CB_LAG[i];
        }

        for i in 0..SUBFRAMES / 2 {
            let mut t = cng_rand(seed, 1 << 13);
            off[i * 2] = (t & 1) as usize;
            off[i * 2 + 1] = ((t >> 1) & 1) as usize + SUBFRAME_LEN;
            t >>= 2;
            for j in 0..11 {
                signs[i * 11 + j] = ((t & 1) * 2 - 1) * (1 << 14);
                t >>= 1;
            }
        }

        let mut idx = 0;
        for i in 0..SUBFRAMES {
            for (j, v) in tmp.iter_mut().take(SUBFRAME_LEN / 2).enumerate() {
                *v = j as i32;
            }
            let mut t = SUBFRAME_LEN as i32 / 2;
            for _ in 0..PULSES[i] {
                let idx2 = cng_rand(seed, t) as usize;
                pos[idx] = (tmp[idx2] * 2) as usize + off[i];
                t -= 1;
                tmp[idx2] = tmp[t as usize];
                idx += 1;
            }
        }

        let mut vp = LPC_ORDER;
        self.audio[vp..vp + PITCH_MAX].copy_from_slice(&self.prev_excitation);
        let mut i = 0;
        while i < SUBFRAMES {
            let lag = self.pitch_lag[i >> 1];
            let v = gen_acb_excitation(&self.audio, vp, lag, &self.subframe[i], self.cur_rate);
            self.audio[vp..vp + SUBFRAME_LEN].copy_from_slice(&v);
            let v = gen_acb_excitation(
                &self.audio,
                vp + SUBFRAME_LEN,
                lag,
                &self.subframe[i + 1],
                self.cur_rate,
            );
            self.audio[vp + SUBFRAME_LEN..vp + 2 * SUBFRAME_LEN].copy_from_slice(&v);

            let vec = &mut self.audio[vp..vp + 2 * SUBFRAME_LEN];
            let mut t = 0i32;
            for &s in vec.iter() {
                t |= (s as i32).abs();
            }
            t = t.min(0x7FFF);
            let mut shift = if t == 0 {
                0
            } else {
                (-10 + av_log2(t as u32)).max(-2)
            };
            let mut sum: i64 = 0;
            if shift < 0 {
                for (j, &s) in vec.iter().enumerate() {
                    let t = (s as i32) * (1 << -shift);
                    sum += t as i64 * t as i64;
                    tmp[j] = t;
                }
            } else {
                for (j, &s) in vec.iter().enumerate() {
                    let t = (s as i32) >> shift;
                    sum += t as i64 * t as i64;
                    tmp[j] = t;
                }
            }

            let base = (i / 2) * 11;
            let mut b0: i32 = 0;
            for j in 0..11 {
                b0 = b0.wrapping_add(tmp[pos[base + j]].wrapping_mul(signs[base + j]));
            }
            b0 = ((b0 as i64 * 2 * 2979 + (1 << 29)) >> 30) as i32;

            let mut c = self
                .cur_gain
                .wrapping_mul(self.cur_gain.wrapping_mul(SUBFRAME_LEN as i32) >> 5);
            if shift * 2 + 3 >= 0 {
                c = shr(c, shift * 2 + 3);
            } else {
                c = shl(c, -(shift * 2 + 3));
            }
            c = ((clipl_i32(sum << 1).wrapping_sub(c) as i64 * 2979) >> 15) as i32;

            let delta = b0.wrapping_mul(b0).wrapping_mul(2).wrapping_sub(c);
            let mut x;
            if delta <= 0 {
                x = b0.wrapping_neg();
            } else {
                let d = square_root(delta as u32) as i32;
                x = d - b0;
                let t = d + b0;
                if t.wrapping_abs() < x.wrapping_abs() {
                    x = -t;
                }
            }
            shift += 1;
            if shift < 0 {
                x >>= -shift;
            } else {
                x = x.wrapping_mul(1 << shift);
            }
            x = x.clamp(-10000, 10000);

            for j in 0..11 {
                let k = base + j;
                let p = pos[k];
                vec[p] = clip_i16(vec[p] as i32 + ((x * signs[k]) >> 15));
            }

            self.audio
                .copy_within(vp..vp + 2 * SUBFRAME_LEN, vp + PITCH_MAX);
            vp += SUBFRAME_LEN * 2;
            i += 2;
        }
        self.prev_excitation
            .copy_from_slice(&self.audio[LPC_ORDER + FRAME_LEN..LPC_ORDER + FRAME_LEN + PITCH_MAX]);
    }

    fn gain_scale(&mut self, buf: &mut [i16], energy: i32) {
        let mut num = energy;
        let mut denom: i32 = 0;
        for &b in buf.iter().take(SUBFRAME_LEN) {
            let t = (b >> 2) as i32;
            denom = sat_dadd32(denom, t * t);
        }

        let gain: i32 = if num != 0 && denom != 0 {
            let bits1 = normalize_bits(num, 31);
            let mut bits2 = normalize_bits(denom, 31);
            num = shl(num, bits1) >> 1;
            denom = shl(denom, bits2);
            bits2 = 5 + bits1 - bits2;
            // av_clip_uintp2(bits2, 5)
            if bits2 & !31 != 0 {
                bits2 = (((!bits2) as u32 >> 31) & 31) as i32;
            }
            let g = div(num >> 1, denom >> 16);
            square_root(shr(shl(g, 16), bits2) as u32) as i32
        } else {
            1 << 12
        };

        for b in buf.iter_mut().take(SUBFRAME_LEN) {
            self.pf_gain = (15i32
                .wrapping_mul(self.pf_gain)
                .wrapping_add(gain)
                .wrapping_add(1 << 3))
                >> 4;
            let m = self.pf_gain.wrapping_add(self.pf_gain >> 4);
            *b = clip_i16(((*b as i32).wrapping_mul(m).wrapping_add(1 << 10)) >> 11);
        }
    }

    fn formant_postfilter(&mut self, lpc: &[i16; SUBFRAMES * LPC_ORDER], dst: &mut [i16]) {
        let mut filter_signal = [0i32; LPC_ORDER + FRAME_LEN];

        self.audio[..LPC_ORDER].copy_from_slice(&self.fir_mem);
        filter_signal[..LPC_ORDER].copy_from_slice(&self.iir_mem);

        for j in 0..SUBFRAMES {
            let i = LPC_ORDER + j * SUBFRAME_LEN;
            let mut fir = [0i16; LPC_ORDER];
            let mut iir = [0i16; LPC_ORDER];
            for k in 0..LPC_ORDER {
                let l = -(lpc[j * LPC_ORDER + k] as i32);
                fir[k] = ((l * POSTFILTER_TBL[0][k] as i32 + (1 << 14)) >> 15) as i16;
                iir[k] = ((l * POSTFILTER_TBL[1][k] as i32 + (1 << 14)) >> 15) as i16;
            }
            // iir_filter(width = 1): res_shift 0, in_shift 16
            for m in 0..SUBFRAME_LEN {
                let mut filter: i64 = 0;
                for n in 1..=LPC_ORDER {
                    let a = (fir[n - 1] as i32).wrapping_mul(self.audio[i + m - n] as i32);
                    let b = (iir[n - 1] as i32).wrapping_mul(filter_signal[i + m - n] >> 16);
                    filter -= a.wrapping_sub(b) as i64;
                }
                filter_signal[i + m] =
                    clipl_i32((self.audio[i + m] as i64) * 65536 + filter * 8 + (1 << 15));
            }
        }

        self.fir_mem
            .copy_from_slice(&self.audio[FRAME_LEN..FRAME_LEN + LPC_ORDER]);
        self.iir_mem
            .copy_from_slice(&filter_signal[FRAME_LEN..FRAME_LEN + LPC_ORDER]);

        for i in 0..SUBFRAMES {
            let bi = LPC_ORDER + i * SUBFRAME_LEN;
            let d = &mut dst[i * SUBFRAME_LEN..(i + 1) * SUBFRAME_LEN];

            let scale = scale_vector(d, &self.audio[bi..bi + SUBFRAME_LEN], SUBFRAME_LEN);

            let ac0 = dot_product(d, 0, d, 1, SUBFRAME_LEN - 1);
            let ac1 = dot_product(d, 0, d, 0, SUBFRAME_LEN);

            let mut temp = ac1 >> 16;
            if temp != 0 {
                temp = div(ac0 >> 2, temp);
            }
            self.reflection_coef = (3i32
                .wrapping_mul(self.reflection_coef)
                .wrapping_add(temp)
                .wrapping_add(2))
                >> 2;
            let temp = (self.reflection_coef.wrapping_neg() >> 1) & !3;

            for j in 0..SUBFRAME_LEN {
                let s = filter_signal[bi + j];
                let sp = filter_signal[bi + j - 1];
                d[j] = (sat_dadd32(s, (sp >> 16).wrapping_mul(temp)) >> 16) as i16;
            }

            let t = 2 * scale + 4;
            let energy = if t < 0 {
                clipl_i32((ac1 as i64) << (-t))
            } else {
                ac1 >> t
            };

            self.gain_scale(d, energy);
        }
    }

    fn decode_into(&mut self, pkt: &[u8], out: &mut [i16]) {
        let mut bad_frame = false;
        let mut ppf = [PpfParam::default(); SUBFRAMES];
        let mut lpc = [0i16; SUBFRAMES * LPC_ORDER];
        // Synthesis input: the excitation buffer (postfilter off) or audio.
        let mut synth_from_exc = false;

        if self.unpack_bitstream(pkt).is_err() {
            bad_frame = true;
            self.cur_frame_type = if self.past_frame_type == FrameType::Active {
                FrameType::Active
            } else {
                FrameType::Untransmitted
            };
        }

        if self.cur_frame_type == FrameType::Active {
            if !bad_frame {
                self.erased_frames = 0;
            } else if self.erased_frames != 3 {
                self.erased_frames += 1;
            }

            let cur_lsp = inverse_quant(&self.prev_lsp, &mut self.lsp_index, bad_frame);
            lsp_interpolate(&mut lpc, &cur_lsp, &self.prev_lsp);
            self.prev_lsp = cur_lsp;

            self.excitation[..PITCH_MAX].copy_from_slice(&self.prev_excitation);
            if self.erased_frames == 0 {
                let a2 = self.subframe[2].amp_index;
                let a3 = self.subframe[3].amp_index;
                self.interp_gain = FIXED_CB_GAIN
                    .get(((a2 + a3) >> 1) as usize)
                    .copied()
                    .unwrap_or(0) as i32;

                for i in 0..SUBFRAMES {
                    let lag = self.pitch_lag[i >> 1];
                    let fcb = gen_fcb_excitation(&self.subframe[i], self.cur_rate, lag, i);
                    let acb = gen_acb_excitation(
                        &self.excitation,
                        SUBFRAME_LEN * i,
                        lag,
                        &self.subframe[i],
                        self.cur_rate,
                    );
                    let v = &mut self.excitation
                        [PITCH_MAX + SUBFRAME_LEN * i..PITCH_MAX + SUBFRAME_LEN * (i + 1)];
                    for j in 0..SUBFRAME_LEN {
                        let t = clip_i16(fcb[j] as i32 * 2) as i32;
                        v[j] = clip_i16(t + acb[j] as i32);
                    }
                }

                let (idx, eng, scale) = self.comp_interp_index(self.pitch_lag[1]);
                self.interp_index = idx;
                self.sid_gain = eng;
                self.cur_gain = scale;

                if self.postfilter {
                    for j in 0..SUBFRAMES {
                        ppf[j] = self.comp_ppf_coeff(
                            PITCH_MAX + j * SUBFRAME_LEN,
                            self.pitch_lag[j >> 1],
                            self.cur_rate,
                        );
                    }
                    for j in 0..SUBFRAMES {
                        let i = j * SUBFRAME_LEN;
                        let p = ppf[j];
                        for k in 0..SUBFRAME_LEN {
                            let a = self.excitation[PITCH_MAX + i + k];
                            let bi = (PITCH_MAX + i + k) as isize + p.index as isize;
                            let b = if bi < 0 {
                                0
                            } else {
                                self.excitation.get(bi as usize).copied().unwrap_or(0)
                            };
                            self.audio[LPC_ORDER + i + k] =
                                weighted(a, b, p.sc_gain, p.opt_gain, 1 << 14, 15);
                        }
                    }
                } else {
                    synth_from_exc = true;
                }

                self.prev_excitation
                    .copy_from_slice(&self.excitation[FRAME_LEN..FRAME_LEN + PITCH_MAX]);
            } else {
                self.interp_gain = (self.interp_gain.wrapping_mul(3).wrapping_add(2)) >> 2;
                if self.erased_frames == 3 {
                    // Mute output (FFmpeg also clears the output frame, which
                    // the synthesis below then overwrites).
                    self.excitation[..FRAME_LEN + PITCH_MAX].fill(0);
                    self.prev_excitation.fill(0);
                } else {
                    self.residual_interp();
                    let s = LPC_ORDER + FRAME_LEN - PITCH_MAX;
                    self.prev_excitation
                        .copy_from_slice(&self.audio[s..s + PITCH_MAX]);
                }
            }
            self.cng_random_seed = CNG_RANDOM_SEED;
        } else {
            if self.cur_frame_type == FrameType::Sid {
                self.sid_gain = sid_gain_to_lsp_index(self.subframe[0].amp_index);
                self.sid_lsp = inverse_quant(&self.prev_lsp, &mut self.lsp_index, false);
            } else if self.past_frame_type == FrameType::Active {
                self.sid_gain = self.estimate_sid_gain();
            }

            if self.past_frame_type == FrameType::Active {
                self.cur_gain = self.sid_gain;
            } else {
                self.cur_gain = (self.cur_gain.wrapping_mul(7).wrapping_add(self.sid_gain)) >> 3;
            }
            self.generate_noise();
            let sid = self.sid_lsp;
            lsp_interpolate(&mut lpc, &sid, &self.prev_lsp);
            self.prev_lsp = sid;
        }

        self.past_frame_type = self.cur_frame_type;

        // LP synthesis (ff_celp_lp_synthesis_filter, shift 1, rounder 1<<12)
        self.audio[..LPC_ORDER].copy_from_slice(&self.synth_mem);
        for j in 0..SUBFRAMES {
            let i = LPC_ORDER + j * SUBFRAME_LEN;
            let coef = &lpc[j * LPC_ORDER..(j + 1) * LPC_ORDER];
            for n in 0..SUBFRAME_LEN {
                let mut sum: i32 = 1 << 12;
                for k in 1..=LPC_ORDER {
                    sum = sum.wrapping_sub(
                        (coef[k - 1] as i32).wrapping_mul(self.audio[i + n - k] as i32),
                    );
                }
                let input = if synth_from_exc {
                    // FFmpeg: audio = vector_ptr - LPC_ORDER
                    self.excitation[PITCH_MAX - LPC_ORDER + i + n]
                } else {
                    self.audio[i + n]
                };
                let sum1 = ((sum >> 12) + input as i32) >> 1;
                self.audio[i + n] = clip_i16(sum1);
            }
        }
        self.synth_mem
            .copy_from_slice(&self.audio[FRAME_LEN..FRAME_LEN + LPC_ORDER]);

        if self.postfilter {
            self.formant_postfilter(&lpc, out);
        } else {
            for i in 0..FRAME_LEN {
                out[i] = clip_i16(2 * self.audio[LPC_ORDER + i] as i32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small deterministic xorshift generator for the robustness tests.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn frame_sizes_by_type() {
        assert_eq!(G7231Decoder::frame_size(0x00), 24);
        assert_eq!(G7231Decoder::frame_size(0x01), 20);
        assert_eq!(G7231Decoder::frame_size(0x02), 4);
        assert_eq!(G7231Decoder::frame_size(0x03), 1);
        assert_eq!(G7231Decoder::frame_size(0xfc), 24);
        assert_eq!(G7231Decoder::frame_size(0xff), 1);
    }

    #[test]
    fn short_packets_produce_nothing() {
        let mut d = G7231Decoder::new();
        assert!(d.decode_frame(&[]).is_empty());
        assert!(d.decode_frame(&[0u8; 23]).is_empty());
        assert!(d.decode_frame(&[1u8; 19]).is_empty());
        assert!(d.decode_frame(&[2u8; 3]).is_empty());
        assert_eq!(d.decode_frame(&[3u8]).len(), 240);
        assert_eq!(d.decode_frame(&[2u8, 0, 0, 0]).len(), 240);
        assert_eq!(d.decode_frame(&[1u8; 20]).len(), 240);
        assert_eq!(d.decode_frame(&[0u8; 24]).len(), 240);
    }

    #[test]
    fn untransmitted_from_reset_is_deterministic() {
        let mut a = G7231Decoder::new();
        let mut b = G7231Decoder::new();
        for _ in 0..10 {
            assert_eq!(a.decode_frame(&[3]), b.decode_frame(&[3]));
        }
        b.decode_frame(&[0u8; 24]);
        b.reset();
        let mut c = G7231Decoder::new();
        assert_eq!(b.decode_frame(&[2, 1, 2, 3]), c.decode_frame(&[2, 1, 2, 3]));
    }

    #[test]
    fn random_packets_never_panic() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for postfilter in [true, false] {
            let mut d = G7231Decoder::new();
            d.set_postfilter(postfilter);
            for n in 0..40_000u32 {
                let len = (rng.next() % 30) as usize;
                let mut pkt: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
                // Bias towards well-formed sizes so every frame type is exercised.
                if n % 2 == 0 && !pkt.is_empty() {
                    pkt.truncate(G7231Decoder::frame_size(pkt[0]));
                }
                let out = d.decode_frame(&pkt);
                assert!(out.is_empty() || out.len() == 240);
                if n % 5000 == 0 {
                    d.reset();
                }
            }
        }
    }

    #[test]
    fn extreme_patterns_never_panic() {
        let mut d = G7231Decoder::new();
        for fill in [0x00u8, 0xff, 0x55, 0xaa, 0x7f, 0x80] {
            for t in 0..4u8 {
                let mut pkt = vec![fill; 24];
                pkt[0] = (fill & !3) | t;
                for _ in 0..50 {
                    let _ = d.decode_frame(&pkt);
                }
            }
        }
    }
}
