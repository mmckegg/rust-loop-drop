FROM debian:jessie
MAINTAINER Damien Lecan <dev@dlecan.com>

#ENV USER root
ENV CHANNEL stable

ENV CC_DIR /opt/gcc-linaro-arm-linux-gnueabihf-raspbian-x64/bin
ENV REAL_CC $CC_DIR/arm-linux-gnueabihf-gcc
ENV CC arm-linux-gnueabihf-gcc-with-link-search
ENV CXX arm-linux-gnueabihf-g++-with-link-search
ENV PATH $CC_DIR:$PATH:/root/.cargo/bin
ENV ROOT_FS /
ENV OBJCOPY $CC_DIR/arm-linux-gnueabihf-objcopy
ENV PKG_CONFIG_ALLOW_CROSS 1

COPY include/config /tmp/.cargo/
COPY include/arm-linux-gnueabihf-gcc-with-link-search /usr/local/sbin/
COPY include/arm-linux-gnueabihf-g++-with-link-search /usr/local/sbin/
COPY include/fixQualifiedLibraryPaths.sh /usr/local/sbin/
COPY include/cargo /usr/local/sbin/
COPY include/sources.list /etc/apt/
#COPY include/sources-armhf.list /etc/apt/sources.list.d/

RUN mv /tmp/.cargo $HOME && \
  dpkg --add-architecture armhf && \
  apt-key adv --recv-keys --keyserver keys.gnupg.net 9165938D90FDDD2E && \
  apt-get update && \
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    file \
    pkg-config \
    curl \
    libssl-dev \
    libssl-dev:armhf && \
  curl https://sh.rustup.rs -sSf | sh /dev/stdin -y && \
  PATH=$PATH:$HOME/.cargo/bin && \
  rustup target add arm-unknown-linux-gnueabihf && \
  curl -sSL https://github.com/raspberrypi/tools/archive/master.tar.gz \
  | tar -zxC /opt tools-master/arm-bcm2708/gcc-linaro-arm-linux-gnueabihf-raspbian-x64 --strip=2 && \
  fixQualifiedLibraryPaths.sh $ROOT_FS $REAL_CC && \
  DEBIAN_FRONTEND=noninteractive apt-get remove --purge -y curl && \
  DEBIAN_FRONTEND=noninteractive apt-get autoremove -y && \
  rm -rf \
    /var/lib/apt/lists/* \
    /tmp/* \
    /var/tmp/* && \
  mkdir -p /source

#FOR LOOP DROP:
RUN apt-get update && \
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    libasound2-dev \
    libasound2-dev:armhf

VOLUME ["/root/.cargo/git", "/root/.cargo/registry"]

VOLUME ["/source"]
WORKDIR /source

CMD ["cargo", "build", "--release"]