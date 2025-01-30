#!/bin/sh
set -e

mkdir -p /app/certs

aws ssm get-parameter \
  --name "/bevy_r_place/certs/certificate" \
  --with-decryption \
  --output text \
  --query "Parameter.Value" \
  | base64 -d > /app/certs/certificate.der

aws ssm get-parameter \
  --name "/bevy_r_place/certs/private_key" \
  --with-decryption \
  --output text \
  --query "Parameter.Value" \
  | base64 -d > /app/certs/private_key.der

aws ssm get-parameter \
  --name "/bevy_r_place/certs/webrtc_pem" \
  --with-decryption \
  --output text \
  --query "Parameter.Value" \
  > /app/certs/webrtc.pem


/app/bevy_r_place_viewer --bootstrap \
  --headless \
  --certificate-chain-path /app/certs/certificate.der \
  --private-key-path /app/certs/private_key.der \
  --webrtc-pem-certificate-path /app/certs/webrtc.pem
