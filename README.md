# bevy_r_place 🖼️

[![GitHub License](https://img.shields.io/github/license/mosure/bevy_r_place)](https://raw.githubusercontent.com/mosure/bevy_r_place/main/LICENSE-MIT)
[![crates.io](https://img.shields.io/crates/v/bevy_r_place.svg)](https://crates.io/crates/bevy_r_place)


p2p r/place clone, view the [web demo on mainnet](https://mosure.github.io/bevy_r_place)

![Alt text](docs/r_place.webp)


## features

- [X] local libp2p
- [X] headless bootstrap node
- [X] default mainnet and network selection
- [X] published image
- [X] LAN auto-discovery
- [ ] prometheus/opentelemetry metrics /w grafana frontend
- [ ] swarm visualization
- [ ] solana implementation


## native client

```bash
git clone https://github.com/mosure/bevy_r_place
cd bevy_r_place
cargo run
```


## host a node

new nodes will automatically connect to mainnet, to host your own network, specify `--bootstrap-node` flag

```bash
docker run ghcr.io/mosure/bevy_r_place:main
```


## metrics

### opentelemetry

see: https://libp2p.github.io/rust-libp2p/metrics_example/index.html
> TODO: native client `metrics` feature flag


### graph visualization

> TODO: swarm topology viewer


## TLS

required for proper WSS and WebRTC function

### local setup

- `mkcert -install`
- `mkcert 127.0.0.1`
- `openssl x509 -in ./127.0.0.1.pem -outform der -out ./certs/certificate.der`
- `openssl rsa -in ./127.0.0.1-key.pem -outform der -out ./certs/private_key.der`
- `cargo run -- --bootstrap --headless --certificate-chain-path ./certs/certificate.der --private-key-path ./certs/private_key.der --webrtc-pem-certificate-path ./certs/webrtc.pem`
