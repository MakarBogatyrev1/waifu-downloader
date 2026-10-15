FROM rust:1-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked
FROM debian:bookworm-slim
RUN mkdir -p /data && chown 10001:10001 /data
COPY --from=build /build/target/release/waifu-sync /usr/local/bin/waifu-sync
USER 10001:10001
WORKDIR /data
EXPOSE 8787
CMD ["waifu-sync"]
