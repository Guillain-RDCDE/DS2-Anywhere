//! DSS SP decoder — matching DssDecoder.dll's exact pipeline.
//!
//! DLL pipeline (discovered 27/08/2026 by full RE):
//!   1. Dequantize reflection coefficients from codebook
//!   2. Per subframe: excitation (adaptive CB + pulses + history update)
//!   3. Per subframe: lattice IIR 1/A(z) with RAW reflection coefficients
//!   4. Tilt filter: y = 0.1 * y_prev + x
//!   5. Sinc resampling 12000 -> 11025 Hz → int16
//!
//! NO error correction filter, NO FIR pre-filter, NO bandwidth expansion,
//! NO noise modulation, NO normalize_bits scaling.

use crate::bitstream::BitstreamReader;
use crate::tables::dss_sp::*;

const SUBFRAMES: usize = 4;
const SUBFRAME_SIZE: usize = 72;
const OUTPUT_SAMPLES: usize = 264;
const LPC_ORDER: usize = 14;

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
    lattice_state: [f64; LPC_ORDER],
    lpc_filter: [f64; LPC_ORDER],
    vector_buf: [f64; SUBFRAME_SIZE],
    tilt_state: f64,
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
            lattice_state: [0.0; LPC_ORDER],
            lpc_filter: [0.0; LPC_ORDER],
            vector_buf: [0.0; SUBFRAME_SIZE],
            tilt_state: 0.0,
            pulse_dec_mode: true,
        }
    }

    pub fn decode_frame(&mut self, pkt: &[u8]) -> Vec<i16> {
        let (filter_idx, sf_adaptive_gain, pitch_lag, subframes) = self.unpack_coeffs(pkt);
        self.unpack_filter(&filter_idx);

        for j in 0..SUBFRAMES {
            self.gen_exc(pitch_lag[j], ADAPTIVE_GAIN[sf_adaptive_gain[j]]);
            self.add_pulses(&subframes[j]);
            self.update_buf();

            for i in 0..SUBFRAME_SIZE {
                self.vector_buf[i] = self.history[SUBFRAME_SIZE - i];
            }

            // Lattice IIR synthesis 1/A(z) — raw reflection coefficients, no expansion
            for n in 0..SUBFRAME_SIZE {
                let mut f = self.vector_buf[n] - self.lpc_filter[LPC_ORDER - 1] * self.lattice_state[LPC_ORDER - 1];
                for i in (0..LPC_ORDER - 1).rev() {
                    let f_new = f - self.lpc_filter[i] * self.lattice_state[i];
                    self.lattice_state[i + 1] = self.lattice_state[i] + self.lpc_filter[i] * f_new;
                    f = f_new;
                }
                self.lattice_state[0] = f;
                self.working_buffer[j][n] = f;
            }
        }

        let mut working_flat = [0.0f64; 288];
        for j in 0..SUBFRAMES {
            working_flat[j * SUBFRAME_SIZE..(j + 1) * SUBFRAME_SIZE]
                .copy_from_slice(&self.working_buffer[j]);
        }

        // AGC on working_flat: compensate for accumulated codebook error
        for j in 0..SUBFRAMES {
            let start = j * SUBFRAME_SIZE;
            let sum_sq: f64 = working_flat[start..start + SUBFRAME_SIZE].iter()
                .map(|&v| v * v)
                .sum();
            let rms = (sum_sq / SUBFRAME_SIZE as f64).sqrt();
            if rms > 0.15 {
                let scale = 0.15 / rms;
                for i in start..start + SUBFRAME_SIZE {
                    working_flat[i] *= scale;
                }
            }
        }

        // Tilt filter (DLL func_177F0) + int16 quantization
        // The DLL converts to int16 BETWEEN the lattice and sinc resampler.
        // This quantization step is critical for stability — it prevents
        // unbounded energy accumulation in the sinc resampler input.
        for i in 0..288 {
            let y = 0.1 * self.tilt_state + working_flat[i];
            // Int16 quantization matching DLL: round, clamp to [-32767,32767], back to f64
            let quantized = (y * 32768.0 + 0.5).floor().clamp(-32767.0, 32767.0);
            working_flat[i] = quantized / 32768.0;
            self.tilt_state = working_flat[i];
        }

        self.update_state(&working_flat)
    }

    fn unpack_coeffs(
        &mut self,
        pkt: &[u8],
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, Vec<SubframeParams>) {
        let mut reader = BitstreamReader::new(pkt);

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

        for j in 0..SUBFRAMES {
            let combined = subframes[j].combined_pulse_pos;
            if combined < C72_BINOMIALS[7] {
                if self.pulse_dec_mode {
                    let mut pulse = 7usize;
                    let mut pulse_idx = 71usize;
                    let mut cp = combined;
                    for _i in 0..7 {
                        while cp < COMBINATORIAL_TABLE[pulse][pulse_idx] {
                            if pulse_idx == 0 { break; }
                            pulse_idx -= 1;
                        }
                        cp -= COMBINATORIAL_TABLE[pulse][pulse_idx];
                        pulse -= 1;
                        subframes[j].pulse_pos[_i] = pulse_idx;
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
                        if index == 0 { break; }
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

        let combined_pitch = reader.read_bits(24) as u64;
        let mut pitch_lag = vec![0usize; SUBFRAMES];
        pitch_lag[0] = ((combined_pitch % 151) + 36) as usize;
        let mut cp = combined_pitch / 151;
        for i in 1..SUBFRAMES - 1 {
            pitch_lag[i] = (cp % 48) as usize;
            cp /= 48;
        }
        pitch_lag[SUBFRAMES - 1] = cp.min(47) as usize;

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
        for i in 0..LPC_ORDER {
            self.lpc_filter[i] = FILTER_CB[i][filter_idx[i]];
        }
    }

    fn gen_exc(&mut self, pitch_lag: usize, gain: f64) {
        if pitch_lag < SUBFRAME_SIZE {
            for i in 0..SUBFRAME_SIZE {
                self.vector_buf[i] = self.history[pitch_lag - i % pitch_lag];
            }
        } else {
            for i in 0..SUBFRAME_SIZE {
                self.vector_buf[i] = self.history[pitch_lag - i];
            }
        }
        for i in 0..SUBFRAME_SIZE {
            self.vector_buf[i] = gain * self.vector_buf[i];
        }
    }

    fn add_pulses(&mut self, sf: &SubframeParams) {
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

    fn update_state(&mut self, working_flat: &[f64]) -> Vec<i16> {
        for i in 0..6 {
            self.excitation[i] = self.excitation[288 + i];
        }
        for i in 0..288 {
            self.excitation[6 + i] = working_flat[i];
        }

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
