.PHONY: build install fmt lint test all

build:
	cargo build --release

install: build
	install -m 755 target/release/loom ~/.local/bin/loom

fmt:
	cargo fmt

lint:
	cargo clippy -- -D warnings

test:
	cargo test

all: fmt lint test
