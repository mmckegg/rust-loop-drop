FROM dlecan/rust-crosscompiler-arm:stable

RUN apt-get update && \
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    libasound2-dev \
    libasound2-dev:armhf
