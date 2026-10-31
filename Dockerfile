# syntax=docker/dockerfile:1
# Builds Domovoi from source with the embedded web UI.
# Release images are built from prebuilt binaries instead (docker/Dockerfile.release).

FROM node:25-alpine AS web
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

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /src/target/release/domovoi /usr/local/bin/domovoi
USER nonroot:nonroot
EXPOSE 7420 7421
ENTRYPOINT ["/usr/local/bin/domovoi"]
CMD ["hub", "run", "--config", "/etc/domovoi/hub.toml"]
