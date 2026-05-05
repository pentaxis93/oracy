FROM docker.io/library/rust:1.95-bookworm AS builder

WORKDIR /workspace
COPY backend ./backend
WORKDIR /workspace/backend
RUN cargo build --release --locked --bin oracy-backend

FROM docker.io/library/debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /workspace/backend/target/release/oracy-backend /usr/local/bin/oracy-backend

ENTRYPOINT ["/usr/local/bin/oracy-backend"]
