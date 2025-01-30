FROM debian:latest

RUN apt-get update && \
    apt-get install -y awscli && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY target/release/viewer /app/bevy_r_place_viewer

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
