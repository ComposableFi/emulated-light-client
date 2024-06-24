FROM rust:1.71.0-bookworm AS build

WORKDIR /build

COPY . .

RUN    apt-get update -y \
    && apt-get install -y protobuf-compiler libprotobuf-dev

RUN cargo fetch --locked
RUN cargo build --release --bin validator

FROM debian:bookworm

RUN    apt-get update -y \
    && apt-get install -y catatonit openssl ca-certificates

COPY --from=build /build/target/release/validator /usr/local/bin

ENTRYPOINT ["/usr/bin/catatonit", "--"]
CMD ["validator"]
