# Contributing to GamepadCapture

Thanks for contributing. This project is pre-1.0; changes should be small,
reviewable, and accompanied by tests when behavior changes.

## Setup and checks

Use the toolchain in `rust-toolchain.toml`. Before opening a pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Install [pre-commit](https://pre-commit.com/) and run `pre-commit install` to
enable local secret and hygiene checks. Run `pre-commit run --all-files` before
pushing.

## Pull requests

1. Branch from `main` and keep the change task-scoped.
2. Explain the motivation and any new dependency.
3. Add or update tests and documentation as applicable.
4. Do not commit secrets, recordings with personal data, credentials, private
   keys, environment files, or hard-coded local paths.
5. Ensure all CI checks pass before requesting review.

## Review policy

This is currently a single-maintainer project. Every change reaches `main`
through a pull request with required CI checks and resolved conversations; the
author performs the pull-request checklist as a recorded self-review. The
repository does not claim independent-review coverage while it has no second
maintainer. When an independent maintainer joins, branch protection will be
changed to require an approving review (and CODEOWNERS where appropriate).

Do not bypass required checks, force-push, or delete `main`. Security-sensitive
changes should receive an additional review whenever a qualified reviewer is
available.

Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md),
not through public issues. By contributing, you agree that your contributions
are licensed under [AGPL-3.0-only](LICENSE).
