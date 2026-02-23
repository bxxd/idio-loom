.PHONY: build install deploy-dev deploy-prod fmt lint test all

build:
	cargo build --release

# Install for current user
install: build
	install -m 755 target/release/loom ~/.local/bin/loom

# Deploy to dev tenant(s)
deploy-dev: build
	sudo install -m 755 target/release/loom /home/idio-dev-ibook/.local/bin/loom

# Deploy to prod tenant(s)
deploy-prod: build
	sudo install -m 755 target/release/loom /home/idio-prod-trawler/.local/bin/loom

fmt:
	cargo fmt

lint:
	cargo clippy -- -D warnings

test:
	cargo test

all: fmt lint test
