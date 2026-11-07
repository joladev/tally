default:
    @just --list

test:
    cargo nextest run

lint:
    cargo fmt --check
    cargo clippy --all-targets -- --deny warnings

fmt:
    cargo fmt

dev:
    RUST_LOG=info cargo run --release
