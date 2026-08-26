//! DSS SP decoder — f64 floating-point arithmetic matching DssDecoder.dll.
//!
//! Architecture: CELP with 14 reflection coefficients, Levinson recursion,
//! pitch-adaptive excitation, 7-pulse fixed codebook, cascaded LPC synthesis +
//! error correction, noise modulation, and 11:12 sinc resampling (12000->11025 Hz).
//!
//! All internal state and arithmetic uses f64 (double precision).
//! Tables store normalized floats (reflection coefficients in [-1,1], gains as
//! true multipliers). Output converts to i16 at the final step only.

use crate::bitstream::BitstreamReader;
use crate::tables::dss_sp::*;

const SUBFRAMES: usize = 4;
const SUBFRAME_SIZE: usize = 72;
const OUTPUT_SAMPLES: usize = 264;

struct SubframeParams {
    combined_pulse_pos: i64,
    gain: usize,
    pulse_val: [usize; 7],
    pulse_pos: [usize; 7],
}

pub struct DssSpDecoder {
    excitation: Vec<f64>,
    history: Vec<f64>,
    working_buffer: [[f64; SUBFRAME_SIZE]; SUBFRAMES],
    audio_buf: [f64; 15],
    err_buf1: [f64; 15],
    err_buf2: [f64; 15],
    lpc_filter: [f64; 14],
    filter: [f64; 15],
    vector_buf: [f64; SUBFRAME_SIZE],
    noise_state: f64,
    pulse_dec_mode: bool,
}

impl Default for DssSpDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl DssSpDecoder {
    pub fn new() -> Self {
        Self {
            excitation: vec![0.0; 288 + 6],
            history: vec![0.0; 187],
            working_buffer: [[0.0; SUBFRAME_SIZE]; SUBFRAMES],
            audio_buf: [0.0; 15],
            err_buf1: [0.0; 15],
            err_buf2: [0.0; 15],
            lpc_filter: [0.0; 14],
            filter: [0.0; 15],
            vector_buf: [0.0; SUBFRAME_SIZE],
            noise_state: 0.0,
            pulse_dec_mode: true,
        }
    }

    pub fn decode_frame(&mut self, pkt: &[u8]) -> Vec<i16> {
        let (filter_idx, sf_adaptive_gain, pitch_lag, subframes) = self.unpack_coeffs(pkt);

        self.unpack_filter(&filter_idx);
        self.convert_coeffs();

        for j in 0..SUBFRAMES {
            self.gen_exc(pitch_lag[j], ADAPTIVE_GAIN[sf_adaptive_gain[j]]);
            self.add_pulses(&subframes[j]);
            self.update_buf();

            for i in 0..SUBFRAME_SIZE {
                self.vector_buf[i] = self.history[SUBFRAME_SIZE - i];
            }

            // shift_sq_sub with err_buf2 -- LPC error correction filter
            {
                for a in 0..SUBFRAME_SIZE {
                    let mut tmp = self.vector_buf[a] * self.filter[0];
                    for i in (1..=14).rev() {
                        tmp -= self.err_buf2[i] * self.filter[i];
                    }
                    for i in (1..=14).rev() {
                        self.err_buf2[i] = self.err_buf2[i - 1];
                    }
                    self.err_buf2[1] = tmp;
                    self.vector_buf[a] = tmp;
                }
            }

            self.sf_synthesis(self.lpc_filter[0], j);

            // AGC: prevent filter instability from producing distorted output
            // The Olympus DLL uses f64 arithmetic which is inherently stable.
            // Our Q15 integer arithmetic accumulates truncation errors that can
            // cause the LPC filter to resonate. This AGC caps the subframe energy
            // to match the DLL's typical output range.
            {
                let sum_sq: f64 = self.working_buffer[j][..SUBFRAME_SIZE].iter()
                    .take(SUBFRAME_SIZE)
                    .map(|&v| (v as f64) * (v as f64))
                    .sum();
                let rms = (sum_sq / SUBFRAME_SIZE as f64).sqrt();
                if rms > 0.183 {
                    let scale = 0.183 / rms;
                    for i in 0..SUBFRAME_SIZE {
                        self.working_buffer[j][i] = self.working_buffer[j][i] * scale;
                    }
                }
            }
        }

        // Flatten working buffer
        let mut working_flat = [0.0f64; 288];
        for j in 0..SUBFRAMES {
            working_flat[j * SUBFRAME_SIZE..(j + 1) * SUBFRAME_SIZE]
                .copy_from_slice(&self.working_buffer[j]);
        }

        self.update_state(&working_flat)
    }

    fn unpack_coeffs(
        &mut self,
        pkt: &[u8],
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, Vec<SubframeParams>) {
        let mut reader = BitstreamReader::new(pkt);

        // Reflection coefficient indices: 2x5 + 6x4 + 6x3 = 52 bits
        let mut filter_idx = Vec::with_capacity(14);
        for _ in 0..2 {
            filter_idx.push(reader.read_bits(5) as usize);
        }
        for _ in 0..6 {
            filter_idx.push(reader.read_bits(4) as usize);
        }
        for _ in 0..6 {
            filter_idx.push(reader.read_bits(3) as usize);
        }

        // Per-subframe: 5 + 31 + 6 + 7*3 = 63 bits x 4 = 252 bits
        let mut sf_adaptive_gain = Vec::with_capacity(SUBFRAMES);
        let mut subframes = Vec::with_capacity(SUBFRAMES);

        for _ in 0..SUBFRAMES {
            let ag = reader.read_bits(5) as usize;
            sf_adaptive_gain.push(ag);

            let combined_pulse_pos = reader.read_bits(31) as i64;
            let gain = reader.read_bits(6) as usize;
            let mut pulse_val = [0usize; 7];
            for pv in &mut pulse_val {
                *pv = reader.read_bits(3) as usize;
            }
            subframes.push(SubframeParams {
                combined_pulse_pos,
                gain,
                pulse_val,
                pulse_pos: [0; 7],
            });
        }

        // Decode pulse positions using combinatorial table
        for j in 0..SUBFRAMES {
            let combined = subframes[j].combined_pulse_pos;
            if combined < C72_BINOMIALS[7] {
                if self.pulse_dec_mode {
                    let mut pulse = 7usize;
                    let mut pulse_idx = 71usize;
                    let mut cp = combined;
                    for i in 0..7 {
                        while cp < COMBINATORIAL_TABLE[pulse][pulse_idx] {
                            if pulse_idx == 0 {
                                break;
                            }
                            pulse_idx -= 1;
                        }
                        cp -= COMBINATORIAL_TABLE[pulse][pulse_idx];
                        pulse -= 1;
                        subframes[j].pulse_pos[i] = pulse_idx;
                    }
                }
            } else {
                self.pulse_dec_mode = false;
                let mut c72 = C72_BINOMIALS;
                subframes[j].pulse_pos[6] = 0;
                let mut index = 6usize;
                let mut cp = combined;
                for i in (0..=71i32).rev() {
                    if c72[index] <= cp {
                        cp -= c72[index];
                        subframes[j].pulse_pos[6 - index] = i as usize;
                        if index == 0 {
                            break;
                        }
                        index -= 1;
                    }
                    c72[0] -= 1;
                    if index > 0 {
                        for a in 0..index {
                            c72[a + 1] -= c72[a];
                        }
                    }
                }
            }
        }

        // Combined pitch (24 bits)
        let combined_pitch = reader.read_bits(24) as u64;

        let mut pitch_lag = vec![0usize; SUBFRAMES];
        pitch_lag[0] = ((combined_pitch % 151) + 36) as usize;
        let mut cp = combined_pitch / 151;

        for i in 1..SUBFRAMES - 1 {
            pitch_lag[i] = (cp % 48) as usize;
            cp /= 48;
        }
        pitch_lag[SUBFRAMES - 1] = cp.min(47) as usize;

        // Convert delta pitch to absolute
        let mut pl = pitch_lag[0];
        for i in 1..SUBFRAMES {
            if pl > 162 {
                pitch_lag[i] += 162 - 23;
            } else {
                let tmp = pl.saturating_sub(23);
                let tmp = tmp.max(36);
                pitch_lag[i] += tmp;
            }
            pl = pitch_lag[i];
        }

        (filter_idx, sf_adaptive_gain, pitch_lag, subframes)
    }

    fn unpack_filter(&mut self, filter_idx: &[usize]) {
        // FILTER_CB values are already f64 in [-1, 1]
        for i in 0..14 {
            self.lpc_filter[i] = FILTER_CB[i][filter_idx[i]];
        }
    }

    fn convert_coeffs(&mut self) {
        // In f64: filter[0] = 1.0, filter[a+1] = lpc_filter[a]
        // formula(c1, lpc, c2) = c1 + lpc * c2
        // No overflow check needed in float.
        self.filter[0] = 1.0;

        for a in 0..14 {
            let a_plus = a + 1;
            self.filter[a_plus] = self.lpc_filter[a];
            for i in 1..=(a_plus / 2) {
                let coeff_1 = self.filter[i];
                let coeff_2 = self.filter[a_plus - i];
                self.filter[i] = coeff_1 + self.lpc_filter[a] * coeff_2;
                self.filter[a_plus - i] = coeff_2 + self.lpc_filter[a] * coeff_1;
            }
        }
    }

    fn gen_exc(&mut self, pitch_lag: usize, gain: f64) {
        // Adaptive codebook: copy from history with pitch lag
        if pitch_lag < SUBFRAME_SIZE {
            for i in 0..SUBFRAME_SIZE {
                self.vector_buf[i] = self.history[pitch_lag - i % pitch_lag];
            }
        } else {
            for i in 0..SUBFRAME_SIZE {
                self.vector_buf[i] = self.history[pitch_lag - i];
            }
        }

        // Scale by adaptive gain (already normalized, no shift needed)
        for i in 0..SUBFRAME_SIZE {
            self.vector_buf[i] = gain * self.vector_buf[i];
        }
    }

    fn add_pulses(&mut self, sf: &SubframeParams) {
        // Fixed codebook: add pulse contributions
        for i in 0..7 {
            let pos = sf.pulse_pos[i];
            let val = FIXED_CB_GAIN[sf.gain] * PULSE_VAL[sf.pulse_val[i]];
            self.vector_buf[pos] += val;
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

    fn sf_synthesis(&mut self, lpc_filter_0: f64, subframe_idx: usize) {
        let size = SUBFRAME_SIZE;

        // Pre-synthesis energy
        let vsum_1: f64 = self.vector_buf[..size].iter().map(|v| v.abs()).sum();

        // Normalize for precision (same logic as integer version)
        let normalize_bits = {
            let mut max_val: f64 = 0.0;
            for v in &self.vector_buf[..size] {
                let a = v.abs();
                if a > max_val {
                    max_val = a;
                }
            }
            if max_val < 1e-30 {
                0i32
            } else {
                // In float equivalent: how many doublings to normalize
                // Target: max_val << nb should be close to 0.5 (= 16384/32768 in Q15)
                let nb = (0.5f64 / max_val).log2().floor() as i32;
                nb.clamp(-20, 20)
            }
        };

        // Scale up
        let scale_vec_factor = (2.0f64).powi(normalize_bits - 3);
        let scale_buf_factor = (2.0f64).powi(normalize_bits);
        for v in self.vector_buf[..size].iter_mut() {
            *v *= scale_vec_factor;
        }
        for v in self.audio_buf.iter_mut() {
            *v *= scale_buf_factor;
        }
        for v in self.err_buf1.iter_mut() {
            *v *= scale_buf_factor;
        }

        let v36 = self.err_buf1[1];

        // shift_sq_add with BINARY_DECREASING
        {
            let tmp_buf = vec_mult_f(&self.filter, &BINARY_DECREASING);
            for a in 0..size {
                self.audio_buf[0] = self.vector_buf[a];
                let mut tmp: f64 = 0.0;
                for i in (0..=14).rev() {
                    tmp += self.audio_buf[i] * tmp_buf[i];
                }
                for i in (1..=14).rev() {
                    self.audio_buf[i] = self.audio_buf[i - 1];
                }
                self.vector_buf[a] = tmp;
            }
        }

        // shift_sq_sub with UNC_DECREASING
        {
            let tmp_buf = vec_mult_f(&self.filter, &UNC_DECREASING);
            for a in 0..size {
                let mut tmp = self.vector_buf[a] * tmp_buf[0];
                for i in (1..=14).rev() {
                    tmp -= self.err_buf1[i] * tmp_buf[i];
                }
                for i in (1..=14).rev() {
                    self.err_buf1[i] = self.err_buf1[i - 1];
                }
                self.err_buf1[1] = tmp;
                self.vector_buf[a] = tmp;
            }
        }

        // Noise modulation LPC
        let lf = {
            let half = lpc_filter_0 / 2.0;
            if half >= 0.0 { 0.0 } else { half }
        };

        if size > 1 {
            for i in (1..size).rev() {
                self.vector_buf[i] = self.vector_buf[i] + lf * self.vector_buf[i - 1];
            }
        }
        self.vector_buf[0] = self.vector_buf[0] + lf * v36;

        // Scale back down
        let unscale_vec = (2.0f64).powi(-(normalize_bits - 3));
        let unscale_buf = (2.0f64).powi(-normalize_bits);
        for v in self.vector_buf[..size].iter_mut() {
            *v *= unscale_vec;
        }
        for v in self.audio_buf.iter_mut() {
            *v *= unscale_buf;
        }
        for v in self.err_buf1.iter_mut() {
            *v *= unscale_buf;
        }

        // Post-synthesis energy
        let vsum_2: f64 = self.vector_buf[..size].iter().map(|v| v.abs()).sum();

        // Energy ratio and noise generation
        let t = if vsum_2 > 1e-10 {
            vsum_1 / vsum_2
        } else {
            0.0
        };

        let bias = (409.0 / 32768.0) * t;
        let decay = 32358.0 / 32768.0;

        let mut noise = [0.0f64; SUBFRAME_SIZE];
        noise[0] = bias + decay * self.noise_state;
        for i in 1..size {
            noise[i] = bias + decay * noise[i - 1];
        }
        self.noise_state = noise[size - 1];

        // Apply noise modulation
        for i in 0..size {
            self.working_buffer[subframe_idx][i] = self.vector_buf[i] * noise[i];
        }
    }

    fn update_state(&mut self, working_flat: &[f64]) -> Vec<i16> {
        for i in 0..6 {
            self.excitation[i] = self.excitation[288 + i];
        }
        for i in 0..288 {
            self.excitation[6 + i] = working_flat[i];
        }

        // Sinc resampling 12000 -> 11025 Hz
        let mut output = Vec::with_capacity(OUTPUT_SAMPLES);
        let mut offset = 6usize;
        let mut a = 0usize;

        while offset < self.excitation.len() {
            let mut tmp: f64 = 0.0;
            for i in 0..6 {
                let idx = offset.wrapping_sub(i);
                if idx < self.excitation.len() {
                    tmp += self.excitation[idx] * SINC[a + i * 11];
                }
            }
            offset += 1;

            // Convert to i16: sinc coefficients sum to ~1.0,
            // so tmp is in signal range. Scale to i16.
            let sample = (tmp * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
            output.push(sample);

            a = (a + 1) % 11;
            if a == 0 {
                offset += 1;
            }
        }

        output.truncate(OUTPUT_SAMPLES);
        output
    }
}

/// Multiply filter coefficients by decreasing weights (f64)
fn vec_mult_f(src: &[f64; 15], mult: &[f64; 15]) -> [f64; 15] {
    let mut dst = [0.0f64; 15];
    dst[0] = src[0];
    for i in 1..15 {
        dst[i] = src[i] * mult[i];
    }
    dst
}
