# syntax=docker/dockerfile:1
# Builds Domovoi from source with the embedded web UI.
# Release images are built from prebuilt binaries instead (docker/Dockerfile.release).

FROM node:22-alpine AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# The toolchain image with cargo-chef, which splits the build in two: the dependencies are
# compiled from a recipe that only changes when a Cargo.toml or Cargo.lock does, so that layer is
# reused while the workspace sources change.
FROM rust:1.97-alpine AS chef
RUN apk add --no-cache musl-dev
# Use the image's toolchain instead of downloading the one pinned in rust-toolchain.toml.
ENV RUSTUP_TOOLCHAIN=1.97.0 CARGO_TERM_COLOR=always
RUN cargo install cargo-chef --locked --version 0.1.78
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
# The cargo profile to build with. CI passes `smoke`, which skips link-time optimisation.
ARG PROFILE=release
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --profile "$PROFILE" --locked --package domovoi --features embed-ui --recipe-path recipe.json
COPY . .
COPY --from=web /src/web/dist web/dist
RUN cargo build --profile "$PROFILE" --locked --package domovoi --features embed-ui \
    && mkdir -p /out/bin && cp "target/$PROFILE/domovoi" /out/bin/domovoi
# The data directory, owned by the runtime user (distroless nonroot is 65532).
RUN mkdir -p /out/var/lib/domovoi && chown 65532:65532 /out/var/lib/domovoi && chmod 0750 /out/var/lib/domovoi

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /out/bin/domovoi /usr/local/bin/domovoi
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
