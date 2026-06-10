"""Grundig DSS-SP (PH9607) native decoder — bit-exact, pure-Python.

    from grundig import decode_dss, write_wav
    pcm = decode_dss("recording.dss")      # list[int], 16 kHz mono 16-bit
    write_wav("recording.wav", pcm)        # 16 kHz mono WAV

CLI: ``grundig-dss recording.dss [out.wav]``.
"""
from .grundig_dss import decode_dss, write_wav, main

__all__ = ["decode_dss", "write_wav", "main"]
__version__ = "1.0.0"
