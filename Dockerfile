FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
# Dépendances compilées à part : cette étape reste en cache tant que
# Cargo.lock ne change pas, et les reconstructions prennent une minute.
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

FROM debian:trixie-slim
RUN useradd --system --uid 10001 navette
WORKDIR /app
COPY --from=build /src/target/release/navette /usr/local/bin/navette
COPY config.toml route.geojson ./
COPY static ./static
RUN mkdir data && chown navette data
USER navette
EXPOSE 8080
# Visible dans Portainer : « healthy » quand le serveur répond.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["navette", "--healthcheck"]
CMD ["navette", "config.toml"]
