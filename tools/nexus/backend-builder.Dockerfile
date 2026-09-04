FROM debian:trixie

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      curl \
      devscripts \
      equivs \
      git \
      gnupg \
      lintian \
 && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://enterprise.proxmox.com/debian/proxmox-archive-keyring-trixie.gpg \
      -o /usr/share/keyrings/proxmox-archive-keyring.gpg \
 && printf '%s\n' \
      'Types: deb' \
      'URIs: http://download.proxmox.com/debian/devel/' \
      'Suites: trixie' \
      'Components: main' \
      'Signed-By: /usr/share/keyrings/proxmox-archive-keyring.gpg' \
      > /etc/apt/sources.list.d/proxmox-devel.sources \
 && printf '%s\n' \
      'Types: deb' \
      'URIs: http://download.proxmox.com/debian/pdm' \
      'Suites: trixie' \
      'Components: pdm-no-subscription' \
      'Signed-By: /usr/share/keyrings/proxmox-archive-keyring.gpg' \
      > /etc/apt/sources.list.d/proxmox-pdm.sources

COPY debian/control /tmp/nexus-source/debian/control

RUN apt-get update \
 && cd /tmp/nexus-source \
 && DEB_BUILD_PROFILES=nodoc mk-build-deps \
      --build-dep \
      --build-profiles nodoc \
      --install \
      --remove \
      --tool 'apt-get -y --no-install-recommends' \
      debian/control \
 && rm -rf /tmp/nexus-source /var/lib/apt/lists/*

RUN useradd --system --home-dir /nonexistent --shell /usr/sbin/nologin nexus-build

ENV NEXUS_BACKEND_BUILDER_READY=1
ENV DEB_BUILD_PROFILES=nodoc
