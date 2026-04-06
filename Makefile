.PHONY: build test clippy lint release clean install run

# Default target
all: build

# Build for development
build:
	cargo build

# Run tests
test:
	cargo test

# Run clippy lints
clippy:
	cargo clippy --all-features

# Run all lints (clippy + fmt)
lint: clippy
	cargo fmt --check

# Build release binary
release:
	cargo build --release --manifest-path Cargo.toml
	@echo "Binary: target/release/dusk"

# Create distributable release
dist:
	@mkdir -p dist
	cargo build --release --manifest-path Cargo.toml --target x86_64-unknown-linux-musl
	cd target/x86_64-unknown-linux-musl/release && tar czf ../../../../dist/dusk-x86_64-unknown-linux-musl.tar.gz dusk
	@echo "Created: dist/dusk-x86_64-unknown-linux-musl.tar.gz"

# Clean build artifacts
clean:
	cargo clean

# Install to ~/.cargo/bin
install-local:
	cargo install --path .

# Run the application
run:
	cargo run

# Run with custom directory
run-dir:
	@dir=$$(pwd) && cargo run -- "$$dir"

# Format code
fmt:
	cargo fmt

# Audit dependencies
audit:
	cargo audit || true

# Build with all optimizations
optimized:
	cargo build --release -j 1
