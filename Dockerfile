# syntax=docker/dockerfile:1
# Builds Domovoi from source with the embedded web UI.
# Release images are built from prebuilt binaries instead (docker/Dockerfile.release).

FROM node:22-alpine AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.97-alpine AS build
RUN apk add --no-cache musl-dev
# Use the image's toolchain instead of downloading the one pinned in rust-toolchain.toml.
ENV RUSTUP_TOOLCHAIN=1.97.0 CARGO_TERM_COLOR=always
WORKDIR /src
COPY . .
COPY --from=web /src/web/dist web/dist
RUN cargo build --release --locked --package domovoi --features embed-ui
# The data directory, owned by the runtime user (distroless nonroot is 65532).
RUN mkdir -p /out/var/lib/domovoi && chown 65532:65532 /out/var/lib/domovoi && chmod 0750 /out/var/lib/domovoi

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /src/target/release/domovoi /usr/local/bin/domovoi
COPY --from=build --chown=65532:65532 /out/var/lib/domovoi /var/lib/domovoi
# No config file is needed; open the setup link from the logs. A hub.toml mounted at
# /etc/domovoi/hub.toml, and DOMOVOI_* variables, are read when present.
ENV DOMOVOI_WEB__BIND=0.0.0.0:7420 \
    DOMOVOI_AGENT_SERVER__BIND=0.0.0.0:7421
USER nonroot:nonroot
VOLUME /var/lib/domovoi
EXPOSE 7420 7421
ENTRYPOINT ["/usr/local/bin/domovoi"]
CMD ["hub", "run"]
