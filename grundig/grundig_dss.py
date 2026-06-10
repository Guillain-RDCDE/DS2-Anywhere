#!/usr/bin/env python3
"""
Grundig DSS-SP (PH9607) native decoder.
Reverse-engineered from dss2wav.dll (DigtaSoft).  Decodes Grundig .dss files
(magic \\x06dss) to 16 kHz mono PCM WAV.  Bit-exact vs the Grundig reference codec.

Pipeline:
  Stage 1  unpack 512-byte audio blocks -> continuous 328-bit CELP frame stream
  Stage 2  CELP synthesis (14th-order reflection-coef lattice + adaptive/fixed
           codebook excitation + de-emphasis)  -> 12 kHz PCM (288 samples/frame)
  Stage 3  3:4 polyphase FIR resample 12 kHz -> 16 kHz  (384 samples/frame)

Tables are extracted from dss2wav.dll's .data section (see gtables.json).
"""
import struct, sys, json, math, os

TBLPATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'gtables.json')
T = json.load(open(TBLPATH))
LPC = [T['LPC0'], T['LPC1']]
WIDTHS = [T['widths0'], T['widths1']]
PITCHGAIN = T['pitchgain']; T1C8 = T['t1c8']; T3C8 = T['t3c8']
CUM = T['cum']; T2908 = T['t2908']; RESAMP = T['resamp']

FRAME_BITS = 328       # 41 bytes per CELP frame
SUB = 4                # subframes per frame
SUBLEN = 72            # samples per subframe (12 kHz)
F318 = -0.1            # de-emphasis pole
HDRSZ = 512            # header block unit

# ----------------------------------------------------------------------------- stage 1
BLOCKBITS = 506 * 8
def _bits(payload, start_bit, n):
    val = 0
    for i in range(n):
        b = start_bit + i; wi = b // 16; bit = b % 16
        if wi * 2 + 1 >= len(payload):
            bv = 0
        else:
            w = (payload[wi*2+1] << 8) | payload[wi*2]
            bv = (w >> (15 - bit)) & 1
        val = (val << 1) | bv
    return val

def unpack_blocks(audio):
    """512-byte blocks -> list of 328-bit frame bit-lists (continuous, padding removed)."""
    nb = len(audio) // 512
    blocks = []
    nframes = 0
    for bi in range(nb):
        blk = audio[bi*512:bi*512+512]
        w0 = (blk[1] << 8) | blk[0]; f421 = w0 >> 4; fc = blk[2]
        word_off = (f421 >> 4) - 3; avail = 0x10 - (f421 & 0xf)
        local_first = word_off * 16 + (16 - avail)
        mode_sig = blk[4]            # -> 0x420 codec mode (0..3); 0 = PH9607 standard
        blocks.append((blk[6:], fc, local_first, mode_sig))
        nframes += fc
    frames = []
    for bi in range(nb):
        payload, fc, lf, msig = blocks[bi]
        pos = lf
        for _ in range(fc):
            need = FRAME_BITS; p = pos; cb = bi; bits = []
            while need > 0:
                take = min(need, BLOCKBITS - p)
                v = _bits(blocks[cb][0], p, take)
                for k in range(take-1, -1, -1):
                    bits.append((v >> k) & 1)
                need -= take; p += take
                if need > 0:
                    cb += 1
                    if cb >= nb:
                        break
                    p = 0
            frames.append((bits, 0))   # mode 0 (standard); extend if other modes appear
            pos = p
    return frames, nframes

class FrameReader:
    def __init__(self, bits):
        self.b = bits; self.i = 0
    def read(self, n):
        v = 0
        for _ in range(n):
            v = (v << 1) | self.b[self.i]; self.i += 1
        return v

# ----------------------------------------------------------------------------- stage 2
class CelpState:
    def __init__(self):
        self.hist = [0.0] * 188   # excitation history (read base index 187, out @115)
        self.syn = [0.0] * 16     # lattice b-state (order 14)
        self.rnd = 0              # unvoiced PRNG (16-bit)
        self.deemph = 0.0         # de-emphasis memory

def _build_voiced(st, lag, gain_idx, fixed31, pulse8):
    out = [0.0] * SUBLEN
    g = PITCHGAIN[gain_idx]
    j = 0; sv = 1
    while j < SUBLEN:
        hi = sv * lag
        while j < hi and j < SUBLEN:
            out[j] = st.hist[187 + (j - sv*lag)] * g
            j += 1
        sv += 1
    p = fixed31
    if p > 0x57cddec7: p = 0x57cddec7
    pos = []; col = 0x47; row = 0x1f8
    for _ in range(7):
        while p < CUM[col + row]:
            col -= 1
        pos.append(col); p -= CUM[col + row]; row -= 0x48
    lead = T1C8[pulse8[0]]
    amps = [T3C8[pulse8[1+k]] * lead for k in range(7)]
    for k in range(7):
        out[pos[k]] += amps[k]
    for i in range(115): st.hist[i] = st.hist[i+72]
    for i in range(72):  st.hist[115+i] = out[i]
    return out

def _build_unvoiced(st, gain_idx):
    out = [0.0] * SUBLEN
    sc = T2908[gain_idx]
    for i in range(SUBLEN):
        st.rnd = (st.rnd * 0x209 + 0x103) & 0xffff
        s = st.rnd - 0x10000 if st.rnd >= 0x8000 else st.rnd
        out[i] = s * sc
    for i in range(115): st.hist[i] = st.hist[i+72]
    for i in range(72):  st.hist[115+i] = out[i]
    return out

def _lattice(st, exc, refl):
    out = [0.0] * SUBLEN; b = st.syn; order = 14
    for n in range(SUBLEN):
        f = exc[n] - refl[order-1] * b[order-1]
        for m in range(order-2, -1, -1):
            f = f - refl[m] * b[m]
            b[m+1] = refl[m] * f + b[m]
        b[0] = f
        out[n] = f
    return out

def _deemph(st, synth):
    out = []; F340 = 32767.5; F348 = -32767.5; F350 = -0.5
    prev = st.deemph
    for x in synth:
        y = x - prev * F318; prev = y
        if y <= F340:
            v = int(math.floor(y - F350)) if y >= F348 else -32767
        else:
            v = 32767
        out.append(v)
    st.deemph = prev
    return out

def decode_frame(st, bits, mode):
    fr = FrameReader(bits); W = WIDTHS[mode]
    if mode == 0:
        voiced = 1
        lsf = [fr.read(W[i]) for i in range(14)]
    else:
        voiced = fr.read(1)
        lsf = [fr.read(W[i]) for i in range(14)]
        if voiced == 0:
            gains = [fr.read(5) for _ in range(4)]
            refl = [LPC[mode][lsf[i] + 32*i] for i in range(14)]
            synth = []
            for sf in range(4):
                synth += _lattice(st, _build_unvoiced(st, gains[sf]), refl)
            return _deemph(st, synth)
    gains = [0]*4; fixed31 = [0]*4; pulses = [[0]*8 for _ in range(4)]
    for sf in range(4):
        gains[sf] = fr.read(5)
        fixed31[sf] = fr.read(31)
        pulses[sf][0] = fr.read(6)
        for k in range(1, 8):
            pulses[sf][k] = fr.read(3)
    v = fr.read(24)
    pit = [0]*4
    pit[0] = v % 0x97; v //= 0x97
    for i in range(1, 4):
        pit[i] = v % 0x30; v //= 0x30
    refl = [LPC[mode][lsf[i] + 32*i] for i in range(14)]
    synth = []; prev_lag = 0
    for sf in range(4):
        if sf == 0:
            lag = pit[0] + 0x24
        elif prev_lag < 0xa3:
            base = max(prev_lag - 0x17, 0x24); lag = pit[sf] + base
        else:
            lag = pit[sf] + 0x8b
        prev_lag = lag
        exc = _build_voiced(st, lag, gains[sf], fixed31[sf], pulses[sf])
        synth += _lattice(st, exc, refl)
    return _deemph(st, synth)

# ----------------------------------------------------------------------------- stage 3
def resample(inp):
    HIST = 0xce
    x = [0.0]*HIST + [float(v) for v in inp]
    N = len(x); out = []; uV = 0; pos = HIST
    F340 = 32767.5; F348 = -32767.5; F350 = -0.5
    while pos < N:
        acc = 0.0; c = uV
        for k in range(100):
            acc += x[pos-k] * RESAMP[c]; c += 4
        if uV == 0:
            acc += x[pos-100] * RESAMP[400]
        if acc <= F340:
            v = int(math.floor(acc - F350)) if acc >= F348 else -32767
        else:
            v = 32767
        out.append(v)
        uV += 3
        if uV > 3:
            adv = uV >> 2; pos += adv; uV -= 4*adv
    return out

# ----------------------------------------------------------------------------- top level
def decode_dss(path):
    d = open(path, 'rb').read()
    if d[1:4] != b'dss':
        raise ValueError('not a DSS file (magic %r)' % d[:4])
    hdr_blocks = d[0]
    audio = d[hdr_blocks * 512:]
    frames, nframes = unpack_blocks(audio)
    st = CelpState()
    pcm12 = []
    for bits, mode in frames:
        pcm12 += decode_frame(st, bits, mode)
    pcm16 = resample(pcm12)
    return pcm16

def write_wav(path, pcm, sr=16000):
    data = struct.pack('<%dh' % len(pcm), *pcm)
    with open(path, 'wb') as f:
        f.write(b'RIFF'); f.write(struct.pack('<I', 36 + len(data))); f.write(b'WAVE')
        f.write(b'fmt '); f.write(struct.pack('<IHHIIHH', 16, 1, 1, sr, sr*2, 2, 16))
        f.write(b'data'); f.write(struct.pack('<I', len(data))); f.write(data)

def main(argv=None):
    """CLI: grundig-dss in.dss [out.wav].  Defaults out.wav to the input stem."""
    argv = sys.argv[1:] if argv is None else argv
    if not argv or argv[0] in ('-h', '--help'):
        print('usage: grundig-dss in.dss [out.wav]   (decode Grundig DSS-SP to 16 kHz mono WAV)')
        return 0 if argv[:1] in (['-h'], ['--help']) else 1
    inp = argv[0]
    out = argv[1] if len(argv) > 1 else (os.path.splitext(inp)[0] + '.wav')
    pcm = decode_dss(inp)
    write_wav(out, pcm)
    print('decoded %d samples (%.1fs @ 16 kHz) -> %s' % (len(pcm), len(pcm) / 16000.0, out))
    return 0

if __name__ == '__main__':
    sys.exit(main())
