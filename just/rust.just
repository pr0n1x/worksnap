# Rust workspace hygiene: check / lint / fmt / machete / test, and the
# `prepare` chain every commit must pass (aliased to `just p`).

lint: lint-fmt lint-clippy machete

lint-clippy:
    @echo "Running clippy..."
    cargo clippy --no-deps --all-targets --workspace -- -D warnings

lint-fmt:
    @echo "Checking code format..."
    cargo fmt --all -- --check

machete:
    @echo "Running machete..."
    cargo machete

fmt:
    @echo "Formatting code..."
    cargo fmt --all

check:
    @echo "Running cargo check..."
    cargo check --all-targets --workspace

test:
    @echo "Running cargo test"
    cargo test

prepare: check lint test

clean:
    @echo "Running cargo clean"
    cargo clean

# Build the release binary and install it into ~/.cargo/bin
install:
    cargo install --path "{{ justfile_directory() }}"
