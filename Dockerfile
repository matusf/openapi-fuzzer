FROM messense/rust-musl-cross:x86_64-musl as builder

WORKDIR /openapi-fuzzer
COPY . /openapi-fuzzer
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM scratch
COPY --from=builder openapi-fuzzer/target/x86_64-unknown-linux-musl/release/openapi-fuzzer /openapi-fuzzer
ENTRYPOINT ["/openapi-fuzzer"]
