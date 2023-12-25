# syntax=docker/dockerfile:1.6
ARG NAME=http-request-duplicator

FROM --platform=$BUILDPLATFORM messense/rust-musl-cross:${TARGETARCH}-musl as builder

ARG NAME
ARG TARGETARCH
RUN if [ $TARGETARCH = "amd64" ]; then \
      echo "x86_64" > /arch; \
    elif [ $TARGETARCH = "arm64" ]; then \
      echo "aarch64" > /arch; \
    else \
      echo "Unsupported platform: $TARGETARCH"; \
      exit 1; \
    fi

WORKDIR /usr/src/${NAME}/
COPY Cargo.toml .
COPY Cargo.lock .
COPY LICENSE .


RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --target $(cat /arch)-unknown-linux-musl \
    && cargo install cargo-license \
    && cargo license --authors \
        --do-not-bundle \
        --avoid-dev-deps \
        --avoid-build-deps \
        --filter-platform $(cat /arch)-unknown-linux-musl > CREDITS

COPY src src
RUN CARGO_BUILD_INCREMENTAL=true cargo build --release --target $(cat /arch)-unknown-linux-musl \
    && cp target/$(cat /arch)-unknown-linux-musl/release/${NAME} target/release/${NAME}

FROM --platform=$TARGETPLATFORM alpine
ARG NAME
COPY --chown=root:root --from=builder /usr/src/${NAME}/CREDITS /usr/src/${NAME}/LICENSE /usr/share/licenses/${NAME}/
COPY --chown=root:root --from=builder /usr/src/${NAME}/target/release/${NAME} /usr/bin/${NAME}
CMD [ "/usr/bin/${NAME}" ]
