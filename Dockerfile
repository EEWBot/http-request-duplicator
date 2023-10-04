FROM rust:1.72.1-bookworm as builder

WORKDIR /usr/src/http-request-duplicator
COPY . .

RUN cargo install cargo-credits; \
	cargo credits; \
	mkdir -p /usr/share/licenses/http-request-duplicator; \
	cp LICENSE CREDITS /usr/share/licenses/http-request-duplicator/; \
	cargo install --path .

FROM debian:bookworm

COPY --from=builder \
	/usr/share/licenses/http-request-duplicator /usr/share/licenses/http-request-duplicator

COPY --from=builder \
	/usr/local/cargo/bin/http-request-duplicator /usr/local/bin/http-request-duplicator

CMD ["http-request-duplicator"]
