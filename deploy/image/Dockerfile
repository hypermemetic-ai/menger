# RUN-10: menger as an image — a TESTED release's own bytes on a slim runtime,
# never a build from a checkout. `deploy/pipeline image [RELEASE]` stages the
# release's bin/menger, bin/run-menger, app/ and RELEASE into a scratch
# context beside this file and builds it (deploy/README.md "The image"):
#
#   context/release/bin/menger       the binary test ran
#   context/release/bin/run-menger   the release's own launcher (ports from the env)
#   context/release/app/             the Leptos app's build, served by --app-dist
#   context/release/RELEASE          commit, subject, gate results
#
# The diagrams service (menger-diagrams + its Node program) is not in it:
# menger never calls it (README "Diagrams"), and it runs mermaid in a Node
# `vm`, which is no sandbox — it stays a local checker (decided in RUN-10).
#
# Inside the container menger binds 0.0.0.0 (RUN-8's --bind), the WebSocket on
# 8080 and the shell on 8081, the database in the /data volume; the REST
# bridge is off (it carries no identity). Every name it is reached by is an
# --allowed-host (MENGER_ALLOWED_HOSTS), or --public-url's (MENGER_PUBLIC_URL),
# as ACCESS-25 requires: 0.0.0.0 adds no name.
FROM debian:bookworm-slim
# ca-certificates: login providers' keys are fetched over https.
# tini: PID 1 that hands SIGTERM to menger (which keeps the default
# disposition), so `docker stop` stops it at once.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --user-group --home-dir /data --shell /usr/sbin/nologin menger \
 && mkdir -p /data && chown menger:menger /data && chmod 0700 /data
COPY --chmod=0555 release/bin/menger release/bin/run-menger /opt/menger/bin/
COPY --chmod=0444 release/RELEASE /opt/menger/RELEASE
COPY release/app /opt/menger/app
USER menger
WORKDIR /data
VOLUME /data
ENV MENGER_BIND=0.0.0.0 \
    MENGER_WS_PORT=8080 \
    MENGER_SHELL_PORT=8081 \
    MENGER_REST_PORT=8082 \
    MENGER_NO_REST=1 \
    MENGER_DB_PATH=/data/tickets.db \
    PLEXUS_NO_REGISTRY=1 \
    RUST_LOG=info
EXPOSE 8080 8081
ARG MENGER_RELEASE=unknown
ARG MENGER_COMMIT=unknown
LABEL org.opencontainers.image.title="menger" \
      org.opencontainers.image.source="https://github.com/hypermemetic-ai/menger" \
      org.opencontainers.image.revision="${MENGER_COMMIT}" \
      org.opencontainers.image.version="${MENGER_RELEASE}" \
      sh.menger.release="${MENGER_RELEASE}"
ENTRYPOINT ["/usr/bin/tini", "--", "/opt/menger/bin/run-menger"]
