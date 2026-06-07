#!/bin/bash
# Grundig DSS-SP native decoder (no Wine, no DLL).
python3 "$(dirname "$0")/grundig_dss.py" "$1" "$2"
