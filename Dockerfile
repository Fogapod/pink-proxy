FROM rust:1.54-alpine as builder

ARG FEATURES="error_reporting"

WORKDIR /build

COPY ./Cargo.lock .
COPY ./Cargo.toml .

# twilight-rs/http-proxy build trick:
# https://github.com/twilight-rs/http-proxy/blob/f7ea681fa4c47b59692827974cd3a7ceb2125161/Dockerfile#L40-L75
RUN mkdir src \
    && echo 'fn main() {}' > src/main.rs \
    && apk update && apk add \
    # required for compiling a lot of crates
    musl-dev \
    # init system
    dumb-init \
    && cargo build --release --features="$FEATURES" \
    && rm -f target/release/deps/pink_proxy*

COPY src src

RUN cargo build --release --features="$FEATURES" \
    && cp target/release/pink-proxy pink-proxy \
    && strip pink-proxy

FROM scratch

COPY --from=builder /usr/bin/dumb-init /bin/dumb-init
COPY --from=builder /build/pink-proxy /usr/bin/pink-proxy

WORKDIR /app

# TODO: user

EXPOSE 8000

ENTRYPOINT ["/bin/dumb-init", "--", "pink-proxy"]