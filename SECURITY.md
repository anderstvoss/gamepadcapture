# Security Policy

## Supported versions

Security fixes are provided for the latest commit on `main` until a release
support policy is published.

## Reporting a vulnerability

Do not open public issues for suspected vulnerabilities.

Use this repository's **Private Vulnerability Reporting** form:
`https://github.com/anderstvoss/gamepadcapture/security/advisories/new`.
Enable it in GitHub repository settings before accepting external
contributions.

Include the affected version or commit, reproduction steps, impact, and any
suggested remediation. The maintainers aim to acknowledge reports within seven
days and will coordinate a disclosure timeline before publishing an advisory.

## Security controls

- GitHub Actions use commit-SHA-pinned third-party actions and minimal token
  permissions.
- CI runs formatting, linting, tests, dependency review, CodeQL, secret
  scanning, SBOM generation, and OpenSSF Scorecard checks.
- Dependabot monitors GitHub Actions and Cargo dependencies weekly.
- Do not commit credentials, private keys, `.env` files, recordings containing
  personal data, or local machine paths.

Repository settings remain part of the security boundary: enable private
vulnerability reporting, Dependabot alerts/security updates, secret scanning,
and protected `main` before accepting external contributions.
