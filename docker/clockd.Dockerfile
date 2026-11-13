# SPDX-License-Identifier: MIT OR Apache-2.0
# ChronoStack controller image: clockd + preload libraries for the engine's architecture.
# Build from the repository root:
#   docker build -f docker/clockd.Dockerfile -t chronostack/clockd:dev .

# --- clockd (static musl binary); its Linux-only unit tests run here ----------
FROM rust:1-alpine AS clockd
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY shim shim
RUN cargo test --release -p chronostack-clockd \
 && cargo build --release -p chronostack-clockd \
 && cp target/release/clockd /clockd

# --- glibc preload library, linked against glibc 2.17 symbols via zig -------
# (a gcc build on a new distro would require GLIBC_2.34 and fail to load in
#  older images such as Debian bullseye or Ubuntu 20.04)
FROM alpine:3.24 AS shim-gnu
RUN apk add --no-cache zig
WORKDIR /src
COPY shim shim
RUN arch="$(uname -m)" \
 && zig cc -target "${arch}-linux-gnu.2.17" -O2 -fPIC -shared -fvisibility=hidden \
      -o "/libchronoclock-${arch}-gnu.so" shim/chronoclock.c -ldl -lpthread

# --- musl preload library ------------------------------------------------------
FROM alpine:3.24 AS shim-musl
RUN apk add --no-cache gcc musl-dev
WORKDIR /src
COPY shim shim
RUN arch="$(uname -m)" \
 && gcc -O2 -Wall -Wextra -fPIC -shared -fvisibility=hidden \
      -o "/libchronoclock-${arch}-musl.so" shim/chronoclock.c

# --- runtime -------------------------------------------------------------------
FROM alpine:3.24
COPY --from=clockd /clockd /usr/local/bin/clockd
COPY --from=shim-gnu /libchronoclock-*-gnu.so /opt/chronostack/lib/
COPY --from=shim-musl /libchronoclock-*-musl.so /opt/chronostack/lib/
LABEL org.opencontainers.image.source="https://github.com/Asmodeus14/ChronoStack" \
      org.opencontainers.image.description="ChronoStack controller (clockd)" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
ENTRYPOINT ["/usr/local/bin/clockd", "serve"]
