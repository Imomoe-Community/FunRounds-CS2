FROM registry.gitlab.steamos.cloud/steamrt/sniper/sdk@sha256:9ae8083ee9ae6298361a62f289e62f0a2b0779d23fc8c276304157f0c06010c5

RUN apt-get update \
    && apt-get install --yes --no-install-recommends gcc-14-monolithic=1:14.2.0-19~steamrt3.1+bsrt3.1 \
    && test "$(g++-14 -dumpfullversion)" = 14.2.0 \
    && rm -rf /var/lib/apt/lists/*

ENV CC=/usr/bin/gcc-14 CXX=/usr/bin/g++-14
