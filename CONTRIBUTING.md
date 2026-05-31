# Contributing

Thanks for helping improve `updt`.

## Development Setup

Install Rust with rustup, then clone the repository and build it:

```bash
cargo build
```

Run the test suite:

```bash
cargo test
```

Before opening a pull request, run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Pull Requests

- Keep changes focused on one behavior or platform area.
- Update `README.md` when changing user-facing commands, targets, or platform policy.
- Include tests when changing parsing, selection, profile policy, or command orchestration.
- Mention the operating system and package manager used to verify the change.

## Reporting Bugs

When filing an issue, include:

- Operating system and shell.
- `updt --version` output.
- The target package manager that failed or behaved unexpectedly.
- Relevant command output, with secrets or tokens removed.
