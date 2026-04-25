# Release Checklist

## Fast Path

Run these commands in order from repo root.

```bash
git status --short
git rev-parse --abbrev-ref HEAD
rg -n '^version\s*=\s*"' Cargo.toml
```

Update version fields.

```bash
# edit Cargo.toml and Cargo.lock
```

Run quality checks.

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo check
```

Commit and push.

```bash
git add Cargo.toml Cargo.lock README.md
# adjust staged files if README was not touched
git commit -m "chore(release): bump version to X.Y.Z"
git push origin <branch>
```

Publish.

```bash
cargo publish
```

Create and push release tag.

```bash
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

## Common Failures

- `crate ... already exists`.
  - Bump patch version and rerun publish flow.

- `working directory contain changes not yet committed`.
  - Commit release files first, then publish without `--allow-dirty`.

- `clippy` warnings.
  - Fix warnings before publish. Do not bypass with allow flags.

- `tag ... already exists`.
  - Verify whether remote tag points to expected release commit.
  - If tag points elsewhere, stop and ask before any force update.

## Targeted Checks For updt

- Verify `updt --version` reflects new version after build.
- Verify fish completion can suggest `updt --` options.
- Verify `updt update ` completion still includes all expected targets.
