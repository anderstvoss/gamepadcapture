# GamepadCapture

`gamepadcapture` is an early Rust foundation for safely capturing and
interpreting game-controller input. It is deliberately small while its public
API and supported platforms are being defined.

## Status

Pre-1.0. Do not rely on API stability or use it to make security decisions.

## Development

Install the Rust toolchain specified in `rust-toolchain.toml`, then run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

For contribution, security-reporting, and project-governance expectations,
see [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

This project is intended to be licensed under GNU AGPL-3.0-only. Before the
first public release, add the complete canonical license text and replace the
copyright holder placeholder in [NOTICE](NOTICE).
