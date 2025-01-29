FROM alpine:latest

COPY target/release/viewer /usr/local/bin/bevy_r_place_viewer


EXPOSE 4201/udp     # QUIC
EXPOSE 4202         # TCP
EXPOSE 4203         # WebSocket
EXPOSE 4204         # Secure WebSocket

CMD ["bevy_r_place_viewer"]
