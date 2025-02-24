export JUST_ROOT        := justfile_directory()

# Default to listing recipes
_default:
  @just --list --list-prefix '  > '

# Open project documentation in your local browser
docs: (_build-docs "open" "nodeps")
  @echo '==> Opening documentation in system browser'

# Build project documentation
build-docs: (_build-docs "" "nodeps")

# Fast check project for errors
check:
  @echo '==> Checking project for compile errors'
  cargo check --workspace --all-features

# Run project test suite, skipping storage tests
test:
  @echo '==> Testing project (default)'
  cargo nextest run --workspace

# Run project test suite, including storage tests (requires storage docker services to be running)
test-all:
  @echo '==> Testing project (all features)'
  cargo nextest run --workspace --all-features

# Run test from project documentation
test-doc:
  @echo '==> Testing project docs'
  cargo test --workspace --doc --all-features

# Bumps the binary version to the given version
bump-version to: (_bump-cargo-version to JUST_ROOT + "/Cargo.toml")

# Lint the project for any quality issues
lint: check fmt clippy commit-check

# Run project linter
clippy:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-clippy >/dev/null; then
    echo '==> Running clippy'
    cargo +nightly clippy --workspace --all-targets --all-features --tests -- -D warnings
  else
    echo '==> clippy not found in PATH, skipping'
    echo '    ^^^^^^ To install `rustup component add clippy`, see https://github.com/rust-lang/rust-clippy for details'
  fi

# Run code formatting check
fmt:
  #!/bin/bash
  set -euo pipefail

  if command -v cargo-fmt >/dev/null; then
    echo '==> Running rustfmt'
    cargo +nightly fmt --all
  else
    echo '==> rustfmt not found in PATH, skipping'
    echo '    ^^^^^^ To install `rustup component add rustfmt`, see https://github.com/rust-lang/rustfmt for details'
  fi

# Run commit checker
commit-check:
  #!/bin/bash
  set -euo pipefail

  if command -v cog >/dev/null; then
    echo '==> Running cog check'
    cog check --from-latest-tag
  else
    echo '==> cog not found in PATH, skipping'
    echo '    ^^^ To install `cargo install --locked cocogitto`, see https://github.com/cocogitto/cocogitto for details'
  fi

# Build project documentation
_build-docs $open="" $nodeps="":
  @echo "==> Building project documentation @$JUST_ROOT/target/doc"
  @cargo doc --all-features --document-private-items ${nodeps:+--no-deps} ${open:+--open}

# Bump the version field of a given Cargo.toml file
_bump-cargo-version version file temp=`mktemp`:
  @echo '==> Bumping {{file}} version to {{version}}'
  @perl -spe 'if (/^version/) { s/("[\w.]+")/"$version"/ }' -- -version={{version}} < {{file}} > {{temp}}
  @mv -f {{temp}} {{file}}
