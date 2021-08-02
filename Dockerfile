FROM rust:1.54-slim as builder

WORKDIR /usr/src/pink-proxy
COPY . .

RUN apt update && apt install -y --no-install-recommends \
    # required to find openssl
    pkg-config \
    #libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install --path .

FROM rust:1.54-slim
COPY --from=builder /usr/local/cargo/bin/pink-proxy /usr/local/bin/pink-proxy

# TODO: user

EXPOSE 8000

CMD ["pink-proxy"]