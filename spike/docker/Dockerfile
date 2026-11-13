# SPDX-License-Identifier: MIT OR Apache-2.0
# M0 spike images. Build context: spike/
#   target clockd     : alpine, holds chronoctl + both .so variants, seeds the volume
#   target tools-gnu  : debian bookworm-slim with cstest (glibc) + shim
#   target tools-musl : alpine with cstest (musl), static + Go negative-test binaries + shim

# glibc build on bookworm (glibc 2.36). Older-glibc compatibility is an M5 concern (zig cc targeting old glibc).
FROM debian:bookworm-slim AS build-gnu
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY shim/ shim/
COPY tests/c/ tests/c/
RUN mkdir -p /out \
 && gcc -O2 -Wall -Wextra -fPIC -shared -fvisibility=hidden -o /out/libchronoclock-gnu.so shim/chronoclock.c -ldl \
 && gcc -O2 -Wall -o /out/cstest tests/c/cstest.c -pthread -ldl

FROM alpine:3.20 AS build-musl
RUN apk add --no-cache gcc musl-dev
WORKDIR /src
COPY shim/ shim/
COPY tests/c/ tests/c/
RUN mkdir -p /out \
 && gcc -O2 -Wall -Wextra -fPIC -shared -fvisibility=hidden -o /out/libchronoclock-musl.so shim/chronoclock.c \
 && gcc -O2 -Wall -o /out/chronoctl shim/chronoctl.c \
 && gcc -O2 -Wall -o /out/cstest tests/c/cstest.c -pthread \
 && gcc -O2 -static -o /out/staticnow tests/c/staticnow.c

FROM golang:1.23-alpine AS build-go
WORKDIR /src
COPY tests/go/main.go .
RUN CGO_ENABLED=0 go build -o /out/gonow main.go

FROM alpine:3.20 AS clockd
COPY --from=build-gnu /out/libchronoclock-gnu.so /opt/chronostack/lib/
COPY --from=build-musl /out/libchronoclock-musl.so /out/chronoctl /opt/chronostack/lib/
COPY docker/clockd-entrypoint.sh /usr/local/bin/clockd-entrypoint.sh
RUN mv /opt/chronostack/lib/chronoctl /usr/local/bin/chronoctl && chmod +x /usr/local/bin/clockd-entrypoint.sh
HEALTHCHECK --interval=1s --timeout=2s --retries=30 CMD test -f /chronostack/ready
ENTRYPOINT ["/usr/local/bin/clockd-entrypoint.sh"]

FROM debian:bookworm-slim AS tools-gnu
COPY --from=build-gnu /out/cstest /usr/local/bin/cstest
COPY --from=build-gnu /out/libchronoclock-gnu.so /opt/chronostack/lib/
COPY --from=build-musl /out/chronoctl /opt/chronostack/chronoctl-musl
CMD ["sleep", "infinity"]

FROM alpine:3.20 AS tools-musl
COPY --from=build-musl /out/cstest /out/staticnow /out/chronoctl /usr/local/bin/
COPY --from=build-musl /out/libchronoclock-musl.so /opt/chronostack/lib/
COPY --from=build-go /out/gonow /usr/local/bin/gonow
CMD ["sleep", "infinity"]
