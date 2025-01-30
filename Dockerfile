FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev build-base

WORKDIR /app

COPY ./Cargo.lock /app/Cargo.lock
COPY ./Cargo.toml /app/Cargo.toml
COPY ./.cargo /app/.cargo
COPY ./src /app/src

RUN cargo build --release --no-default-features --features aws,native


FROM rust:alpine

WORKDIR /app

COPY --from=builder /app/target/release/viewer /app/bevy_r_place_viewer

# QUIC
EXPOSE 4201/udp
# TCP
EXPOSE 4202
# WebSocket
EXPOSE 4203
# Secure WebSocket
EXPOSE 4204
# WebRTC
EXPOSE 4205/udp

CMD ["/app/bevy_r_place_viewer",  "--bootstrap", "--headless"]
