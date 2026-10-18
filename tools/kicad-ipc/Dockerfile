# K11.1: KiCad's nightly (master) from the kicad-dev-nightly PPA, headless.
#   docker build -t freerouting-rs/kicad-nightly tools/kicad-ipc
# The PPA serves only the latest nightly; `kicad-cli-nightly version` names the build.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends software-properties-common gpg-agent ca-certificates \
 && add-apt-repository -y ppa:kicad/kicad-dev-nightly && apt-get update \
 && apt-get install -y --no-install-recommends kicad-nightly \
 && rm -rf /var/lib/apt/lists/*
