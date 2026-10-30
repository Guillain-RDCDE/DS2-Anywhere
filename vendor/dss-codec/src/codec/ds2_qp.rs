//! DS2 QP decoder - f64 lattice synthesis, 16000 Hz output.
//!
//! Supports classic QP (format 6, 56-byte frames) and QP7 (format 7): QP with
//! a type bit at the start of every frame. A QP7 frame with the bit set is a
//! 448-bit QP frame whose last reflection coefficient has 2 bits and its own
//! 4-entry codebook; with the bit clear it is a 96-bit noise frame (type bit,
//! 75 coefficient bits, four 5-bit noise gains).
//!
//! This follows the ds2 decoder of Patrick Domack's FFmpeg fork
//! (libavcodec/ds2.c), which matches Olympus's R8 DssDecoder.dll within 1 LSB.
//! State carries over every frame; noise never enters the pitch history.

use crate::bitstream::BitstreamReader;
use crate::codec::common::{decode_combinatorial_index, lattice_synthesis};
use crate::demux::ds2::{Ds2Qp7Segment, Ds2QpSegment};
use crate::tables::ds2_qp::{qp_codebook_lookup, QP7_CODEBOOK_LAST, QP7_NOISE_GAIN};
use crate::tables::ds2_quant::{QP_EXCITATION_GAIN, QP_PITCH_GAIN, QP_PULSE_AMP};

const NUM_COEFFS: usize = 16;
const NUM_SUBFRAMES: usize = 4;
const SUBFRAME_SIZE: usize = 64;
const SAMPLES_PER_FRAME: usize = NUM_SUBFRAMES * SUBFRAME_SIZE;
const MIN_PITCH: usize = 45;
const MAX_PITCH: usize = 300;
const PITCH_MEM_LEN: usize = MAX_PITCH + SUBFRAME_SIZE;
const EXCITATION_PULSES: usize = 11;
const FRAME_SIZE: usize = 56;
const QP7_NOISE_FRAME_SIZE: usize = 12;
/// C(64, 11): the number of ways to place 11 pulses among 64 samples. A
/// 40-bit fixed-codebook index can be larger and then names no set of
/// positions. Olympus's DssDecoder.dll 1.1.1.0 and Philips's PSPGRU.ACM
/// clamp it to C(64, 11) - 1, which puts the pulses at 63 down to 53, and so
/// do we. Real files carry such indices only in blocks that hold no speech.
const QP_COMB_MAX: u64 = 743_595_781_824;
const REFL_BIT_ALLOC: [u32; NUM_COEFFS] = [7, 7, 6, 6, 5, 5, 5, 5, 5, 4, 4, 4, 4, 3, 3, 3];
const QP7_LAST_REFL_BITS: u32 = 2;
const QP7_NOISE_GAIN_BITS: u32 = 5;
const PITCH_GAIN_BITS: u32 = 6;
const GAIN_BITS: u32 = 6;
const PULSE_BITS: u32 = 3;
const PITCH_BITS: u32 = 8;
const CB_BITS: u32 = 40;
const DEEMPH_COEFF: f64 = 0.1;

pub struct Ds2QpDecoder {
    lattice_state: [f64; NUM_COEFFS],
    pitch_memory: [f64; PITCH_MEM_LEN],
    deemph_state: f64,
    noise_seed: u16,
}

impl Default for Ds2QpDecoder {
    fn default() -> Self {
        Self::new()
    }
}

struct Subframe {
    pitch_lag: usize,
    pitch_gain: f64,
    exc_gain: f64,
    positions: Vec<usize>,
    pulses: [usize; EXCITATION_PULSES],
}

impl Ds2QpDecoder {
    pub fn new() -> Self {
        Self {
            lattice_state: [0.0; NUM_COEFFS],
            pitch_memory: [0.0; PITCH_MEM_LEN],
            deemph_state: 0.0,
            noise_seed: 0,
        }
    }

    /// Reset the state carried from frame to frame to that of a new decoder.
    pub fn reset_state(&mut self) {
        *self = Self::new();
    }

    /// Decode one packet as the demuxer returns it: 56 bytes, or in QP7 12
    /// bytes for a noise frame. Returns 256 samples (de-emphasized, before
    /// conversion to integers), or `None` for a packet too short for its frame,
    /// which the reference decoder skips.
    pub fn decode_packet(&mut self, pkt: &[u8], qp7: bool) -> Option<Vec<f64>> {
        // The type bit is the top bit of the first byte in 16-bit word order
        let frame_size = if qp7 && pkt.len() >= 2 && pkt[1] & 0x80 == 0 {
            QP7_NOISE_FRAME_SIZE
        } else {
            FRAME_SIZE
        };
        if pkt.len() < frame_size {
            return None;
        }
        let mut reader = BitstreamReader::new(&pkt[..frame_size]);
        let noise_frame = qp7 && reader.read_bits(1) == 0;

        let mut coeffs = [0.0f64; NUM_COEFFS];
        for (i, coeff) in coeffs.iter_mut().enumerate() {
            if qp7 && i == NUM_COEFFS - 1 {
                *coeff = QP7_CODEBOOK_LAST[reader.read_bits(QP7_LAST_REFL_BITS) as usize];
            } else {
                *coeff = qp_codebook_lookup(i, reader.read_bits(REFL_BIT_ALLOC[i]) as usize);
            }
        }

        let mut frame = Vec::with_capacity(SAMPLES_PER_FRAME);
        if noise_frame {
            let mut gains = [0usize; NUM_SUBFRAMES];
            for g in &mut gains {
                *g = reader.read_bits(QP7_NOISE_GAIN_BITS) as usize;
            }
            for &g in &gains {
                let gain = QP7_NOISE_GAIN[g] as i32;
                let mut excitation = [0.0f64; SUBFRAME_SIZE];
                for e in &mut excitation {
                    self.noise_seed = self.noise_seed.wrapping_mul(0x209).wrapping_add(0x103);
                    *e = (self.noise_seed as i16 as i32 * gain) as f64 / 32768.0;
                }
                // Noise does not enter the pitch memory
                frame.extend(lattice_synthesis(
                    &excitation,
                    &coeffs,
                    &mut self.lattice_state,
                ));
            }
        } else {
            let subframes: Vec<Subframe> = (0..NUM_SUBFRAMES)
                .map(|_| {
                    let pitch_lag = reader.read_bits(PITCH_BITS) as usize + MIN_PITCH;
                    let pg_idx = reader.read_bits(PITCH_GAIN_BITS) as usize;
                    let cb_index = reader.read_bits_u64(CB_BITS);
                    let eg_idx = reader.read_bits(GAIN_BITS) as usize;
                    let mut pulses = [0usize; EXCITATION_PULSES];
                    for p in &mut pulses {
                        *p = reader.read_bits(PULSE_BITS) as usize;
                    }
                    Subframe {
                        pitch_lag,
                        pitch_gain: QP_PITCH_GAIN[pg_idx],
                        exc_gain: QP_EXCITATION_GAIN[eg_idx],
                        positions: decode_combinatorial_index(
                            cb_index.min(QP_COMB_MAX - 1),
                            SUBFRAME_SIZE,
                            EXCITATION_PULSES,
                        ),
                        pulses,
                    }
                })
                .collect();

            for sf in &subframes {
                let excitation = self.subframe_excitation(sf);
                self.update_pitch_memory(&excitation);
                frame.extend(lattice_synthesis(
                    &excitation,
                    &coeffs,
                    &mut self.lattice_state,
                ));
            }
        }

        // De-emphasis, once per frame
        frame[0] += DEEMPH_COEFF * self.deemph_state;
        for i in 1..frame.len() {
            frame[i] += DEEMPH_COEFF * frame[i - 1];
        }
        self.deemph_state = frame[SAMPLES_PER_FRAME - 1];
        Some(frame)
    }

    fn subframe_excitation(&self, sf: &Subframe) -> [f64; SUBFRAME_SIZE] {
        let lag = sf.pitch_lag;
        let mut excitation = [0.0f64; SUBFRAME_SIZE];
        let mut fixed = [0.0f64; SUBFRAME_SIZE];
        for (i, &pos) in sf.positions.iter().enumerate() {
            if pos < SUBFRAME_SIZE {
                fixed[pos] += QP_PULSE_AMP[sf.pulses[i]] * sf.exc_gain;
            }
        }
        for (i, e) in excitation.iter_mut().enumerate() {
            let adaptive = if lag < SUBFRAME_SIZE {
                self.pitch_memory[PITCH_MEM_LEN - lag + (i % lag)]
            } else {
                self.pitch_memory[PITCH_MEM_LEN - lag + i]
            };
            *e = sf.pitch_gain * adaptive + fixed[i];
        }
        excitation
    }

    fn update_pitch_memory(&mut self, excitation: &[f64; SUBFRAME_SIZE]) {
        self.pitch_memory.copy_within(SUBFRAME_SIZE.., 0);
        self.pitch_memory[MAX_PITCH..].copy_from_slice(excitation);
    }

    /// Decode a sequence of demuxed packets.
    pub fn decode_packets(&mut self, packets: &[Vec<u8>], qp7: bool) -> Vec<f64> {
        let mut all = Vec::with_capacity(packets.len() * SAMPLES_PER_FRAME);
        for pkt in packets {
            if let Some(frame) = self.decode_packet(pkt, qp7) {
                all.extend(frame);
            }
        }
        all
    }

    /// Decode `total_frames` consecutive 56-byte QP frames from `stream`; a
    /// frame the stream ends inside is zero-filled.
    pub fn decode_all_frames(&mut self, stream: &[u8], total_frames: usize) -> Vec<f64> {
        let mut all = Vec::with_capacity(total_frames * SAMPLES_PER_FRAME);
        for f in 0..total_frames {
            all.extend(self.decode_frame(stream.get(f * FRAME_SIZE..).unwrap_or(&[])));
        }
        all
    }

    /// Decode one QP (format 6) frame; missing bytes read as zeros.
    pub fn decode_frame(&mut self, frame_bytes: &[u8]) -> Vec<f64> {
        let mut pkt = [0u8; FRAME_SIZE];
        let n = frame_bytes.len().min(FRAME_SIZE);
        pkt[..n].copy_from_slice(&frame_bytes[..n]);
        self.decode_packet(&pkt, false).unwrap_or_default()
    }

    pub fn decode_qp_segments(&mut self, segments: &[Ds2QpSegment]) -> Vec<f64> {
        let mut all = Vec::new();
        for seg in segments {
            if seg.reset_before {
                self.reset_state();
            }
            all.extend(self.decode_all_frames(&seg.stream, seg.total_frames));
        }
        all
    }

    pub fn decode_qp7_segments(&mut self, segments: &[Ds2Qp7Segment]) -> Vec<f64> {
        let mut all = Vec::new();
        for seg in segments {
            if seg.reset_before {
                self.reset_state();
            }
            all.extend(self.decode_packets(&seg.records, true));
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_or_garbage_packets_never_panic() {
        let mut dec = Ds2QpDecoder::new();
        assert!(dec.decode_packet(&[], false).is_none());
        assert!(dec.decode_packet(&[0u8; 11], true).is_none());
        assert_eq!(dec.decode_packet(&[0u8; 12], true).unwrap().len(), 256);
        assert!(dec.decode_packet(&[0u8, 0x80, 0, 0], true).is_none());
        for b in [0x00u8, 0x55, 0xaa, 0xff] {
            assert_eq!(dec.decode_packet(&[b; 56], false).unwrap().len(), 256);
            assert_eq!(dec.decode_packet(&[b; 56], true).unwrap().len(), 256);
        }
    }

    #[test]
    fn noise_frames_leave_pitch_memory_alone() {
        let mut dec = Ds2QpDecoder::new();
        let mut noise = [0u8; 12];
        noise[0] = 0xff; // gains non-zero
        noise[10] = 0xff;
        noise[11] = 0x7f;
        let out = dec.decode_packet(&noise, true).unwrap();
        assert!(out.iter().any(|&s| s != 0.0));
        assert!(dec.pitch_memory.iter().all(|&s| s == 0.0));
    }
}
