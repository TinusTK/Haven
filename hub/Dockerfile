# Haven Hub: Haven without a screen, for a Raspberry Pi 4/5 or a small
# server. Build from the top of the repository:
#
#   docker build -f hub/Dockerfile -t haven-hub .
#
# Works for arm64 (Pi, Oracle Ampere) and x86-64 alike.

FROM rust:1-bookworm AS build
# Haven's own Trakt and Simkl app ids (the same ones built into every copy
# of Haven), so the Hub can keep your linked accounts in step.
ARG HAVEN_TRAKT_CLIENT_ID=""
ARG HAVEN_SIMKL_CLIENT_ID=""
ENV HAVEN_TRAKT_CLIENT_ID=$HAVEN_TRAKT_CLIENT_ID \
    HAVEN_SIMKL_CLIENT_ID=$HAVEN_SIMKL_CLIENT_ID
WORKDIR /src
COPY app/ app/
COPY LICENSE THIRD-PARTY.md ./
RUN cd app && cargo build --release -p haven-hub \
    && cp target/release/haven-hub /haven-hub

FROM debian:bookworm-slim
# Certificate authorities, so the Hub can reach add-ons and servers over https.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /data haven \
    && mkdir -p /data && chown haven /data
COPY --from=build /haven-hub /usr/local/bin/haven-hub
COPY LICENSE /usr/share/doc/haven-hub/LICENSE
USER haven
VOLUME /data
ENV HAVEN_HUB_DATA=/data
EXPOSE 8590
CMD ["haven-hub"]
