.PHONY: build test unit e2e all clean fmt clippy

build:
	cargo build

test: unit

unit:
	cargo test --workspace

e2e: build
	./scripts/e2e.sh

all: unit e2e

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

clean:
	cargo clean
	rm -rf .dev-servers/e2e .dev-servers/e2e-unpacked .dev-servers/bundle.zip
