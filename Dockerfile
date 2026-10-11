# Multi-stage build:
#  1. compile dss-decode-native from the vendored Rust crate (vendor/dss-codec)
#  2. assemble the runtime image (slim Debian + node + ffmpeg + the binary + our scripts)
#
# Usage:
#   docker build -t ds2-anywhere .
#   docker run --rm -v $(pwd)/examples:/data -p 8765:8765 ds2-anywhere
#
# Then the HTTP daemon is reachable at http://localhost:8765 .

# ============================================================
# Stage 1 — build the native decoder from vendor/dss-codec
# ============================================================
FROM rust:1-bookworm AS builder

WORKDIR /src

# The same crate the in-browser decoder (web/, built by pages.yml) is compiled
# from, so the native CLI and the web page decode identically: CI decodes the
# sample files both ways and compares the PCM sample for sample. Changing the
# codec means changing vendor/ on purpose, in a reviewed commit, with the bench
# re-run (see docs/15-the-relay-runs-backward.md for why the codec is never
# taken from a floating upstream branch). --locked keeps Cargo.lock authoritative.
COPY vendor/dss-codec/ ./dss-codec/
RUN cd dss-codec && \
    cargo build --release --locked && \
    cp target/release/dss-decode /tmp/dss-decode-native

# ============================================================
# Stage 2 — runtime image
# ============================================================
FROM debian:12-slim

ARG INSTALL_DIR=/opt/conv-dss-ds2-to-mp3

RUN apt-get update && apt-get install -y --no-install-recommends \
        nodejs \
        ffmpeg \
        ca-certificates \
        mariadb-client \
        && rm -rf /var/lib/apt/lists/*

# Native binary from the build stage
COPY --from=builder /tmp/dss-decode-native /usr/local/bin/dss-decode-native
RUN chmod +x /usr/local/bin/dss-decode-native

# Project files
WORKDIR ${INSTALL_DIR}
COPY src/ ./src/
RUN chmod +x src/bin/*

# CLI symlink so `conv-dss-ds2-to-mp3` works from anywhere in the container
RUN ln -sf ${INSTALL_DIR}/src/bin/conv-dss-ds2-to-mp3 /usr/local/bin/conv-dss-ds2-to-mp3

# Default config: no DB (demo mode). Override by mounting a config at /etc/conv-dss-ds2-to-mp3/audio-cron.conf
RUN mkdir -p /etc/conv-dss-ds2-to-mp3 && \
    sed 's/^USE_DB=.*/USE_DB=0/' src/etc/audio-cron.conf.example > /etc/conv-dss-ds2-to-mp3/audio-cron.conf

EXPOSE 8765

# The HTTP daemon (web UI talks to this). The cron is documented separately
# (run with --restart=always or a sidecar in production setups).
CMD ["node", "/opt/conv-dss-ds2-to-mp3/src/bin/http_server.mjs"]
