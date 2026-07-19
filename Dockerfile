# Multi-stage build:
#  1. compile dss-decode-native from the upstream Rust crate
#  2. assemble the runtime image (slim Debian + node + ffmpeg + the binary + our scripts)
#
# Usage:
#   docker build -t ds2-anywhere .
#   docker run --rm -v $(pwd)/examples:/data -p 8765:8765 ds2-anywhere
#
# Then the HTTP daemon is reachable at http://localhost:8765 .

# ============================================================
# Stage 1 — build the native decoder from upstream
# ============================================================
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    git ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Pinned to a known-good commit of the codec fork, fetched by SHA, for a
# reproducible build. This commit was shadow-benched against 316 real DSS/DS2
# recordings (0 garbling, no panics) — see docs/15-the-relay-runs-backward.md.
# A floating `--depth=1` of the default branch would let a future upstream
# regression reach this image silently on the next build; that is exactly the
# class of bug docs/15 is about. Bump this on purpose, and re-run the bench.
ARG DSS_CODEC_REV=e16b71c5abdf47e1f6ea2bae3f4428d69afdab31
RUN git init -q . && \
    git remote add origin https://github.com/gaspardpetit/dss-codec.git && \
    git fetch -q --depth=1 origin "${DSS_CODEC_REV}" && \
    git checkout -q FETCH_HEAD && \
    cd dss-codec && \
    cargo build --release && \
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
