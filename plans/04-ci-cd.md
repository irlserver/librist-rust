# CI/CD Pipeline Design

## Overview

This document describes the CI/CD pipeline for the librist-rust project, ensuring code quality, cross-platform compatibility, and security.

## GitHub Actions Workflow

### Main CI Pipeline

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

# Cancel in-progress runs for the same branch
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

jobs:
  # Format check (fast, runs first)
  fmt:
    name: Format
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all -- --check

  # Clippy linting
  clippy:
    name: Clippy
    runs-on: ubuntu-latest
    needs: fmt
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --workspace --all-targets --all-features -- -D warnings

  # Build and test matrix
  build:
    name: Build (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    needs: fmt
    strategy:
      fail-fast: false
      matrix:
        include:
          # Linux x86_64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            
          # Linux ARM64 (cross-compile)
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            cross: true
            
          # macOS x86_64
          - os: macos-13
            target: x86_64-apple-darwin
            
          # macOS ARM64
          - os: macos-14
            target: aarch64-apple-darwin
            
          # Windows x86_64
          - os: windows-latest
            target: x86_64-pc-windows-msvc

    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      # Install dependencies based on OS
      - name: Install dependencies (Linux)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
          if [ "${{ matrix.cross }}" = "true" ]; then
            sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
          fi

      - name: Install dependencies (macOS)
        if: runner.os == 'macOS'
        run: brew install meson ninja pkg-config

      - name: Install dependencies (Windows)
        if: runner.os == 'Windows'
        run: |
          choco install meson ninja pkgconfiglite

      # Install cross for cross-compilation
      - name: Install cross
        if: matrix.cross
        run: cargo install cross --git https://github.com/cross-rs/cross

      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}

      # Build
      - name: Build
        if: ${{ !matrix.cross }}
        run: cargo build --workspace --target ${{ matrix.target }} --release

      - name: Build (cross)
        if: matrix.cross
        run: cross build --workspace --target ${{ matrix.target }} --release

      # Test (only on native targets)
      - name: Test
        if: ${{ !matrix.cross }}
        run: cargo test --workspace --target ${{ matrix.target }} --release

  # Documentation build
  docs:
    name: Documentation
    runs-on: ubuntu-latest
    needs: fmt
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@stable
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
      - uses: Swatinem/rust-cache@v2
      - name: Build docs
        run: cargo doc --workspace --no-deps --document-private-items
        env:
          RUSTDOCFLAGS: -D warnings

  # Security audit
  audit:
    name: Security Audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}

  # Minimum Supported Rust Version check
  msrv:
    name: MSRV
    runs-on: ubuntu-latest
    needs: fmt
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@1.75.0  # MSRV
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --workspace

  # Feature combinations
  features:
    name: Feature Combinations
    runs-on: ubuntu-latest
    needs: fmt
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@stable
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
      - uses: Swatinem/rust-cache@v2
      - name: Install cargo-hack
        run: cargo install cargo-hack
      - name: Check feature combinations
        run: |
          cargo hack check --workspace --feature-powerset \
            --skip default \
            --exclude-features experimental

  # Integration tests (requires running RIST endpoints)
  integration:
    name: Integration Tests
    runs-on: ubuntu-latest
    needs: build
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@stable
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
      - uses: Swatinem/rust-cache@v2
      - name: Run integration tests
        run: cargo test --workspace --test '*' --release
        env:
          RUST_LOG: debug

  # Coverage report
  coverage:
    name: Coverage
    runs-on: ubuntu-latest
    needs: build
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
      - uses: Swatinem/rust-cache@v2
      - name: Install cargo-llvm-cov
        run: cargo install cargo-llvm-cov
      - name: Generate coverage
        run: cargo llvm-cov --workspace --lcov --output-path lcov.info
      - name: Upload coverage
        uses: codecov/codecov-action@v4
        with:
          files: lcov.info
          fail_ci_if_error: false
```

### Release Pipeline

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write

jobs:
  build-release:
    name: Build Release (${{ matrix.target }})
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: librist.so
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            artifact: librist.so
            cross: true
          - os: macos-13
            target: x86_64-apple-darwin
            artifact: librist.dylib
          - os: macos-14
            target: aarch64-apple-darwin
            artifact: librist.dylib
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: rist.dll

    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install dependencies (Linux)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
          if [ "${{ matrix.cross }}" = "true" ]; then
            sudo apt-get install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
          fi

      - name: Install dependencies (macOS)
        if: runner.os == 'macOS'
        run: brew install meson ninja pkg-config

      - name: Install dependencies (Windows)
        if: runner.os == 'Windows'
        run: choco install meson ninja pkgconfiglite

      - name: Install cross
        if: matrix.cross
        run: cargo install cross --git https://github.com/cross-rs/cross

      - uses: Swatinem/rust-cache@v2
        with:
          key: release-${{ matrix.target }}

      - name: Build
        if: ${{ !matrix.cross }}
        run: cargo build --workspace --target ${{ matrix.target }} --release

      - name: Build (cross)
        if: matrix.cross
        run: cross build --workspace --target ${{ matrix.target }} --release

      - name: Package
        shell: bash
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/*.rlib dist/ || true
          cp target/${{ matrix.target }}/release/*.a dist/ || true
          cp target/${{ matrix.target }}/release/*.so dist/ || true
          cp target/${{ matrix.target }}/release/*.dylib dist/ || true
          cp target/${{ matrix.target }}/release/*.dll dist/ || true
          tar -czvf librist-rust-${{ matrix.target }}.tar.gz -C dist .

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: librist-rust-${{ matrix.target }}
          path: librist-rust-${{ matrix.target }}.tar.gz

  publish:
    name: Publish to crates.io
    runs-on: ubuntu-latest
    needs: build-release
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@stable
      - name: Install dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y meson ninja-build pkg-config
      - name: Publish librist-sys
        run: cargo publish -p librist-sys --token ${{ secrets.CRATES_TOKEN }}
      - name: Wait for crates.io
        run: sleep 30
      - name: Publish librist
        run: cargo publish -p librist --token ${{ secrets.CRATES_TOKEN }}

  create-release:
    name: Create GitHub Release
    runs-on: ubuntu-latest
    needs: build-release
    steps:
      - uses: actions/checkout@v4
      - name: Download all artifacts
        uses: actions/download-artifact@v4
        with:
          path: artifacts
      - name: Create release
        uses: softprops/action-gh-release@v1
        with:
          files: artifacts/**/*.tar.gz
          generate_release_notes: true
```

## Makefile

```makefile
# Makefile
.PHONY: all build test check clean fmt clippy doc install

# Default target
all: build

# Build all crates
build:
	cargo build --workspace

# Build in release mode
release:
	cargo build --workspace --release

# Run all tests
test:
	cargo test --workspace

# Run tests with verbose output
test-verbose:
	cargo test --workspace -- --nocapture

# Check compilation without building
check:
	cargo check --workspace --all-targets --all-features

# Format code
fmt:
	cargo fmt --all

# Check formatting
fmt-check:
	cargo fmt --all -- --check

# Run clippy
clippy:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

# Build documentation
doc:
	cargo doc --workspace --no-deps --document-private-items

# Open documentation in browser
doc-open:
	cargo doc --workspace --no-deps --document-private-items --open

# Clean build artifacts
clean:
	cargo clean

# Run security audit
audit:
	cargo audit

# Check feature combinations
feature-check:
	cargo hack check --workspace --feature-powerset

# Generate coverage report
coverage:
	cargo llvm-cov --workspace --html

# Run benchmarks
bench:
	cargo bench --workspace

# Update dependencies
update:
	cargo update

# Install development tools
install-tools:
	cargo install cargo-hack cargo-audit cargo-llvm-cov

# Full CI check (run before pushing)
ci: fmt-check clippy test doc
	@echo "All CI checks passed!"
```

## Development Container

```dockerfile
# .devcontainer/Dockerfile
FROM rust:1.75-bookworm

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    pkg-config \
    meson \
    ninja-build \
    cmake \
    nasm \
    yasm \
    && rm -rf /var/lib/apt/lists/*

# Install Rust components
RUN rustup component add rustfmt clippy llvm-tools-preview

# Install cargo tools
RUN cargo install cargo-hack cargo-audit cargo-llvm-cov

# Set up working directory
WORKDIR /workspace

# Pre-build dependencies
COPY Cargo.toml Cargo.lock ./
COPY crates/librist-sys/Cargo.toml ./crates/librist-sys/
COPY crates/librist/Cargo.toml ./crates/librist/
RUN mkdir -p crates/librist-sys/src crates/librist/src \
    && echo "fn main() {}" > crates/librist-sys/src/lib.rs \
    && echo "fn main() {}" > crates/librist/src/lib.rs \
    && cargo build --release \
    && rm -rf crates/

CMD ["bash"]
```

```json
// .devcontainer/devcontainer.json
{
  "name": "librist-rust",
  "dockerFile": "Dockerfile",
  "features": {
    "ghcr.io/devcontainers/features/rust:1": {
      "version": "1.75"
    }
  },
  "customizations": {
    "vscode": {
      "extensions": [
        "rust-lang.rust-analyzer",
        "vadimcn.vscode-lldb",
        "serayuzgur.crates",
        "tamasfe.even-better-toml"
      ],
      "settings": {
        "rust-analyzer.checkOnSave.command": "clippy"
      }
    }
  },
  "postCreateCommand": "cargo build",
  "remoteUser": "root"
}
```

## Pre-commit Hooks

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt
        entry: cargo fmt --all -- --check
        language: system
        types: [rust]
        pass_filenames: false

      - id: cargo-clippy
        name: cargo clippy
        entry: cargo clippy --workspace --all-targets -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false

      - id: cargo-test
        name: cargo test
        entry: cargo test --workspace --lib
        language: system
        types: [rust]
        pass_filenames: false
```

## Badge Configuration

Add to README.md:

```markdown
[![CI](https://github.com/irlserver/librist-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/irlserver/librist-rust/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/librist.svg)](https://crates.io/crates/librist)
[![docs.rs](https://docs.rs/librist/badge.svg)](https://docs.rs/librist)
[![codecov](https://codecov.io/gh/username/librist-rust/branch/main/graph/badge.svg)](https://codecov.io/gh/username/librist-rust)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
```
