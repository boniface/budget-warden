# Security Policy

## Supported Versions

`budget-warden` is currently pre-1.0. Security fixes are applied to the latest unreleased or published version.

## Reporting a Vulnerability

Do not open a public issue for a suspected vulnerability.

Report security concerns privately through the repository security advisory process, or contact the maintainer directly if private advisories are not available. Include:

- A description of the issue.
- A minimal reproduction if possible.
- The affected version or commit.
- Any known impact or workaround.

## Secret Handling

This library must not store or load application secrets directly.

Host applications are responsible for reading secrets from sources such as:

- `.env.local` or `.env.prod`
- Kubernetes Secrets
- Docker secrets
- CI/CD secret stores
- Cloud secret managers

The host application should use those values to create database or Redis pools, then pass the pool into `budget-warden`.

Do not put credentials in budget policy TOML files. Policy config should contain budget rules only.

## Logging

Do not log:

- Database URLs
- Redis URLs
- Passwords
- API keys
- Access tokens
- Raw secret values
- Sensitive request metadata

Tracing events should identify budget keys and policy outcomes without exposing credentials.

## Dependency Security

The project uses:

```sh
make security
```

This runs:

- `cargo audit`
- `cargo deny check`

Dependency changes should pass the security gate before merge.
