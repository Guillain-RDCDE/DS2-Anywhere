//! DSS SP decoder: a port of FFmpeg's libavcodec/dss_sp.c (Copyright (c) 2014
//! Oleksij Rempel), as Patrick Domack's FFmpeg fork has it in n9.0.2b,
//! variable rate SP (format 1) included. LGPL-2.1-or-later, like its source.
//!
//! CELP with 14 reflection coefficients turned into a direct-form filter, a
//! pitch-adaptive excitation, a 7-pulse fixed codebook, an error-shaping
//! filter, noise modulation, and 11:12 sinc decimation from 12000 to 11000 Hz.
//!
//! The arithmetic is FFmpeg's, 32-bit and wrapping where its C wraps, so the
//! output is the same sample for sample. Measured against the source audio of
//! Olympus encodes, this synthesis scores level with Olympus's 2007 dss32.dll
//! and above its R8 DssDecoder.dll, and above the lattice synthesis this
//! project tried in 2026 (docs/THE-LATTICE-HUNT.md).

use crate::bitstream::BitstreamReader;
use crate::tables::dss_sp::*;

const SUBFRAMES: usize = 4;
const SUBFRAME_SIZE: usize = 72;
/// Bytes of a frame as the demuxer hands it over.
const FRAME_SIZE: usize = 42;
/// Samples out per frame: 288 at 12000 Hz, decimated 11:12.
const OUTPUT_SAMPLES: usize = 264;

/// Variable rate SP (format 1): the 14th filter index has 2 bits and its own
/// table. From Olympus's R8 DssDecoder.dll and the 2007 dss32.dll, which agree
/// on it (Patrick Domack, FFmpeg fork commit de1e1d4).
const VR_FILTER_CB13: [i32; 4] = [-8166, -2419, 1997, 6705];

/// Noise gains of the short frames of variable rate SP: the dss32.dll table,
/// which fits this fixed-point synthesis better than R8's.
const VR_NOISE_GAIN: [i32; 32] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 26, 28, 30, 34,
    40, 46, 52, 60, 72, 84,
];

fn clip16(x: i32) -> i32 {
    x.clamp(-32768, 32767)
}

/// DSS_SP_FORMULA: ((a << 15) + b * c + 0x4000) >> 15, with FFmpeg's casts.
fn formula(a: i32, b: i32, c: i32) -> i32 {
    let sum = (a.wrapping_mul(1 << 15) as u32)
        .wrapping_add((b as u32).wrapping_mul(c as u32))
        .wrapping_add(0x4000);
    (sum as i32) >> 15
}

#[derive(Clone, Copy, Default)]
struct Subframe {
    gain: usize,
    combined_pulse_pos: u32,
    pulse_pos: [usize; 7],
    pulse_val: [usize; 7],
}

/// The parameters of the last frame read. Fields a frame does not carry keep
/// their value from the frame before, as in FFmpeg.
#[derive(Clone, Copy, Default)]
struct FrameParams {
    filter_idx: [usize; 14],
    sf_adaptive_gain: [usize; SUBFRAMES],
    pitch_lag: [usize; SUBFRAMES],
    sf: [Subframe; SUBFRAMES],
}

pub struct DssSpDecoder {
    excitation: [i32; 288 + 6],
    history: [i32; 187],
    fparam: FrameParams,
    working_buffer: [[i32; SUBFRAME_SIZE]; SUBFRAMES],
    audio_buf: [i32; 15],
    err_buf1: [i32; 15],
    lpc_filter: [i32; 14],
    filter: [i32; 15],
    vector_buf: [i32; SUBFRAME_SIZE],
    noise_state: i32,
    err_buf2: [i32; 15],
    pulse_dec_mode: bool,
    /// Format 1: every frame starts with a type bit, 0 for a short frame that
    /// carries only the filter and four noise gains.
    variable_rate: bool,
    noise_frame: bool,
    noise_seed: u16,
}

impl Default for DssSpDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl DssSpDecoder {
    pub fn new() -> Self {
        Self {
            excitation: [0; 288 + 6],
            history: [0; 187],
            fparam: FrameParams::default(),
            working_buffer: [[0; SUBFRAME_SIZE]; SUBFRAMES],
            audio_buf: [0; 15],
            err_buf1: [0; 15],
            lpc_filter: [0; 14],
            filter: [0; 15],
            vector_buf: [0; SUBFRAME_SIZE],
            noise_state: 0,
            err_buf2: [0; 15],
            pulse_dec_mode: true,
            variable_rate: false,
            noise_frame: false,
            noise_seed: 0,
        }
    }

    /// A decoder for variable rate SP, format 1.
    pub fn new_variable_rate() -> Self {
        Self {
            variable_rate: true,
            ..Self::new()
        }
    }

    /// Start afresh, as after a seek.
    pub fn reset(&mut self) {
        *self = if self.variable_rate {
            Self::new_variable_rate()
        } else {
            Self::new()
        };
    }

    /// Decode one 42-byte packet into 264 samples at 11000 Hz. A shorter
    /// packet gives no samples, as in FFmpeg.
    pub fn decode_frame(&mut self, pkt: &[u8]) -> Vec<i16> {
        if pkt.len() < FRAME_SIZE {
            return Vec::new();
        }
        self.unpack_coeffs(&pkt[..FRAME_SIZE]);
        self.unpack_filter();
        self.convert_coeffs();

        for j in 0..SUBFRAMES {
            if self.noise_frame {
                // The noise does not enter the pitch history.
                self.gen_noise(VR_NOISE_GAIN[self.fparam.sf_adaptive_gain[j]]);
            } else {
                self.gen_exc(
                    self.fparam.pitch_lag[j],
                    ADAPTIVE_GAIN[self.fparam.sf_adaptive_gain[j]],
                );
                self.add_pulses(j);
                self.update_buf();
                for i in 0..SUBFRAME_SIZE {
                    self.vector_buf[i] = self.history[SUBFRAME_SIZE - i];
                }
            }

            let filter = self.filter;
            shift_sq_sub(&filter, &mut self.err_buf2, &mut self.vector_buf);
            self.sf_synthesis(self.lpc_filter[0], j);
        }

        self.update_state()
    }

    fn unpack_coeffs(&mut self, src: &[u8]) {
        let mut gb = BitstreamReader::new(src);
        let p = &mut self.fparam;

        self.noise_frame = self.variable_rate && gb.read_bits(1) == 0;

        for i in 0..2 {
            p.filter_idx[i] = gb.read_bits(5) as usize;
        }
        for i in 2..8 {
            p.filter_idx[i] = gb.read_bits(4) as usize;
        }
        for i in 8..13 {
            p.filter_idx[i] = gb.read_bits(3) as usize;
        }
        p.filter_idx[13] = gb.read_bits(if self.variable_rate { 2 } else { 3 }) as usize;

        if self.noise_frame {
            for g in p.sf_adaptive_gain.iter_mut() {
                *g = gb.read_bits(5) as usize;
            }
            return;
        }

        for j in 0..SUBFRAMES {
            p.sf_adaptive_gain[j] = gb.read_bits(5) as usize;
            p.sf[j].combined_pulse_pos = gb.read_bits(31);
            p.sf[j].gain = gb.read_bits(6) as usize;
            for v in p.sf[j].pulse_val.iter_mut() {
                *v = gb.read_bits(3) as usize;
            }
        }

        for j in 0..SUBFRAMES {
            let mut c72: [u32; 8] = [
                72, 2556, 59640, 1028790, 13991544, 156238908, 1473109704, 3379081753,
            ];
            let mut combined = p.sf[j].combined_pulse_pos;

            // A 31-bit value is always below C(72, 8), so FFmpeg always takes
            // this branch; the other is kept as FFmpeg has it.
            if combined < c72[7] {
                if self.pulse_dec_mode {
                    let mut pulse = 7usize;
                    let mut pulse_idx = 71usize;
                    for i in 0..7 {
                        while combined < COMBINATORIAL_TABLE[pulse][pulse_idx] as u32 {
                            if pulse_idx == 0 {
                                break;
                            }
                            pulse_idx -= 1;
                        }
                        combined =
                            combined.wrapping_sub(COMBINATORIAL_TABLE[pulse][pulse_idx] as u32);
                        pulse -= 1;
                        p.sf[j].pulse_pos[i] = pulse_idx;
                    }
                }
            } else {
                self.pulse_dec_mode = false;
                p.sf[j].pulse_pos[6] = 0;
                let mut index = 6usize;
                for i in (0..72usize).rev() {
                    if c72[index] <= combined {
                        combined -= c72[index];
                        p.sf[j].pulse_pos[6 - index] = i;
                        if index == 0 {
                            break;
                        }
                        index -= 1;
                    }
                    c72[0] = c72[0].wrapping_sub(1);
                    for a in 0..index {
                        c72[a + 1] = c72[a + 1].wrapping_sub(c72[a]);
                    }
                }
            }
        }

        let mut combined_pitch = gb.read_bits(24);
        p.pitch_lag[0] = (combined_pitch % 151 + 36) as usize;
        combined_pitch /= 151;
        for i in 1..SUBFRAMES - 1 {
            p.pitch_lag[i] = (combined_pitch % 48) as usize;
            combined_pitch /= 48;
        }
        if combined_pitch > 47 {
            combined_pitch = 0;
        }
        p.pitch_lag[SUBFRAMES - 1] = combined_pitch as usize;

        let mut pitch_lag = p.pitch_lag[0];
        for i in 1..SUBFRAMES {
            if pitch_lag > 162 {
                p.pitch_lag[i] += 162 - 23;
            } else {
                p.pitch_lag[i] += (pitch_lag - 23).max(36);
            }
            pitch_lag = p.pitch_lag[i];
        }
    }

    fn unpack_filter(&mut self) {
        for i in 0..14 {
            self.lpc_filter[i] = FILTER_CB[i][self.fparam.filter_idx[i]] as i32;
        }
        if self.variable_rate {
            self.lpc_filter[13] = VR_FILTER_CB13[self.fparam.filter_idx[13]];
        }
    }

    fn convert_coeffs(&mut self) {
        let lpc = &self.lpc_filter;
        let c = &mut self.filter;
        c[0] = 0x2000;
        for a in 0..14 {
            let a_plus = a + 1;
            c[a_plus] = lpc[a] >> 2;
            for i in 1..=a_plus / 2 {
                let coeff_1 = c[i];
                let coeff_2 = c[a_plus - i];
                c[i] = clip16(formula(coeff_1, lpc[a], coeff_2));
                c[a_plus - i] = clip16(formula(coeff_2, lpc[a], coeff_1));
            }
        }
    }

    fn add_pulses(&mut self, j: usize) {
        let sf = &self.fparam.sf[j];
        for i in 0..7 {
            self.vector_buf[sf.pulse_pos[i]] +=
                (FIXED_CB_GAIN[sf.gain] * PULSE_VAL[sf.pulse_val[i]] + 0x4000) >> 15;
        }
    }

    /// Excitation of a short frame: a linear congruential noise, scaled by the
    /// subframe's gain. The seed runs on from frame to frame.
    fn gen_noise(&mut self, gain: i32) {
        for v in self.vector_buf.iter_mut() {
            self.noise_seed = self.noise_seed.wrapping_mul(0x209).wrapping_add(0x103);
            *v = (self.noise_seed as i16 as i32 * gain) >> 14;
        }
    }

    fn gen_exc(&mut self, pitch_lag: usize, gain: i32) {
        for i in 0..SUBFRAME_SIZE {
            self.vector_buf[i] = if pitch_lag < SUBFRAME_SIZE {
                self.history[pitch_lag - i % pitch_lag]
            } else {
                self.history[pitch_lag - i]
            };
        }
        for v in self.vector_buf.iter_mut() {
            *v = clip16(gain.wrapping_mul(*v) >> 11);
        }
    }

    fn update_buf(&mut self) {
        for i in (1..=114).rev() {
            self.history[i + SUBFRAME_SIZE] = self.history[i];
        }
        for i in 0..SUBFRAME_SIZE {
            self.history[SUBFRAME_SIZE - i] = self.vector_buf[i];
        }
    }

    fn sf_synthesis(&mut self, lpc_filter: i32, j: usize) {
        let size = SUBFRAME_SIZE;

        let vsum_1 = vector_sum(&self.vector_buf).min(0xFFFFF);
        let normalize_bits = normalize_bits(&self.vector_buf);

        scale_vector(&mut self.vector_buf, normalize_bits - 3);
        scale_vector(&mut self.audio_buf, normalize_bits);
        scale_vector(&mut self.err_buf1, normalize_bits);

        let v36 = self.err_buf1[1];

        let tmp_buf = vec_mult(&self.filter, &BINARY_DECREASING);
        shift_sq_add(&tmp_buf, &mut self.audio_buf, &mut self.vector_buf);

        let tmp_buf = vec_mult(&self.filter, &UNC_DECREASING);
        shift_sq_sub(&tmp_buf, &mut self.err_buf1, &mut self.vector_buf);

        // lpc_filter can be negative
        let lpc_filter = (lpc_filter >> 1).min(0);

        for i in (1..size).rev() {
            self.vector_buf[i] = clip16(formula(
                self.vector_buf[i],
                lpc_filter,
                self.vector_buf[i - 1],
            ));
        }
        self.vector_buf[0] = clip16(formula(self.vector_buf[0], lpc_filter, v36));

        scale_vector(&mut self.vector_buf, -normalize_bits);
        scale_vector(&mut self.audio_buf, -normalize_bits);
        scale_vector(&mut self.err_buf1, -normalize_bits);

        let vsum_2 = vector_sum(&self.vector_buf);
        let t = if vsum_2 >= 0x40 {
            (vsum_1 << 11) / vsum_2
        } else {
            1
        };

        let bias = (409i32.wrapping_mul(t) >> 15) << 15;
        let mut noise = [0i32; SUBFRAME_SIZE];
        noise[0] = clip16(bias.wrapping_add(32358i32.wrapping_mul(self.noise_state)) >> 15);
        for i in 1..size {
            noise[i] = clip16(bias.wrapping_add(32358 * noise[i - 1]) >> 15);
        }
        self.noise_state = noise[size - 1];

        for i in 0..size {
            self.working_buffer[j][i] = clip16(self.vector_buf[i].wrapping_mul(noise[i]) >> 11);
        }
    }

    fn update_state(&mut self) -> Vec<i16> {
        for i in 0..6 {
            self.excitation[i] = self.excitation[288 + i];
        }
        for j in 0..SUBFRAMES {
            self.excitation[6 + j * SUBFRAME_SIZE..6 + (j + 1) * SUBFRAME_SIZE]
                .copy_from_slice(&self.working_buffer[j]);
        }

        let mut out = Vec::with_capacity(OUTPUT_SAMPLES);
        let mut offset = 6usize;
        let mut a = 0usize;
        while offset < self.excitation.len() {
            let mut tmp = 0i32;
            for i in 0..6 {
                tmp = tmp.wrapping_add(self.excitation[offset - i].wrapping_mul(SINC[a + i * 11]));
            }
            offset += 1;
            out.push(clip16(tmp >> 15) as i16);
            a = (a + 1) % 11;
            if a == 0 {
                offset += 1;
            }
        }
        out.truncate(OUTPUT_SAMPLES);
        out
    }
}

fn shift_sq_sub(filter: &[i32; 15], error_buf: &mut [i32; 15], dst: &mut [i32; SUBFRAME_SIZE]) {
    for v in dst.iter_mut() {
        let mut tmp = v.wrapping_mul(filter[0]);
        for i in (1..=14).rev() {
            tmp = (tmp as u32).wrapping_sub((error_buf[i] as u32).wrapping_mul(filter[i] as u32))
                as i32;
        }
        for i in (1..=14).rev() {
            error_buf[i] = error_buf[i - 1];
        }
        let tmp = ((tmp as u32).wrapping_add(4096) as i32) >> 13;
        error_buf[1] = tmp;
        *v = clip16(tmp);
    }
}

fn shift_sq_add(filter: &[i32; 15], audio_buf: &mut [i32; 15], dst: &mut [i32; SUBFRAME_SIZE]) {
    for v in dst.iter_mut() {
        audio_buf[0] = *v;
        let mut tmp = 0i32;
        for i in (0..=14).rev() {
            tmp = tmp.wrapping_add(audio_buf[i].wrapping_mul(filter[i]));
        }
        for i in (1..=14).rev() {
            audio_buf[i] = audio_buf[i - 1];
        }
        *v = clip16(tmp.wrapping_add(4096) >> 13);
    }
}

fn vec_mult(src: &[i32; 15], mult: &[i32; 15]) -> [i32; 15] {
    let mut dst = [0i32; 15];
    dst[0] = src[0];
    for i in 1..15 {
        dst[i] = src[i].wrapping_mul(mult[i]).wrapping_add(0x4000) >> 15;
    }
    dst
}

fn vector_sum(v: &[i32]) -> i32 {
    v.iter().fold(0i32, |s, x| s.wrapping_add(x.wrapping_abs()))
}

fn normalize_bits(v: &[i32]) -> i32 {
    let mut val: u32 = 1;
    for x in v {
        val |= x.unsigned_abs();
    }
    let mut bits = 0;
    while val <= 0x4000 {
        val *= 2;
        bits += 1;
    }
    bits
}

fn scale_vector(v: &mut [i32], bits: i32) {
    if bits < 0 {
        for x in v.iter_mut() {
            *x >>= -bits;
        }
    } else {
        for x in v.iter_mut() {
            *x = x.wrapping_mul(1 << bits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_gives_264_samples() {
        let mut d = DssSpDecoder::new();
        assert_eq!(d.decode_frame(&[0x5a; FRAME_SIZE]).len(), OUTPUT_SAMPLES);
        assert!(d.decode_frame(&[0; 10]).is_empty());
    }

    #[test]
    fn never_panics_on_random_frames() {
        let mut seed = 0x9e37_79b9u32;
        for vr in [false, true] {
            let mut d = if vr {
                DssSpDecoder::new_variable_rate()
            } else {
                DssSpDecoder::new()
            };
            for _ in 0..3000 {
                let pkt: Vec<u8> = (0..FRAME_SIZE)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        seed as u8
                    })
                    .collect();
                assert_eq!(d.decode_frame(&pkt).len(), OUTPUT_SAMPLES);
            }
        }
    }

    #[test]
    fn reset_repeats_the_output() {
        let pkt = [0x37u8; FRAME_SIZE];
        let mut d = DssSpDecoder::new_variable_rate();
        let first = d.decode_frame(&pkt);
        d.decode_frame(&pkt);
        d.reset();
        assert_eq!(d.decode_frame(&pkt), first);
    }
}
