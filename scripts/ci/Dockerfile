FROM registry.gitlab.steamos.cloud/steamrt/sniper/sdk@sha256:bb30053d3cd19d35c76ae69b2790dbaa1430448006f8240cb6d7e723f2d1940e

RUN apt-get update \
    && apt-get install --yes --no-install-recommends gcc-14-monolithic=1:14.2.0-19~steamrt3.1+bsrt3.1 \
    && test "$(g++-14 -dumpfullversion)" = 14.2.0 \
    && rm -rf /var/lib/apt/lists/*

ENV CC=/usr/bin/gcc-14 CXX=/usr/bin/g++-14
