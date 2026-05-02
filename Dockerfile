FROM rust:slim-trixie as builder

WORKDIR /usr/src/rusttp
COPY . .
RUN cargo install --path .

FROM debian:trixie-slim
COPY --from=builder /usr/local/cargo/bin/rusttp /usr/local/bin/rusttp
CMD ["rusttp"]