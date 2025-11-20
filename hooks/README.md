# Git Hooks

This directory contains Git hooks for the AiDb project to ensure code quality.

## Available Hooks

### pre-commit

The pre-commit hook runs automatically before each commit and performs the following checks:

1. **Code Formatting** (`cargo fmt --all -- --check`)
   - Ensures all code follows consistent formatting rules
   - If formatting issues are found, the commit is blocked
   - Fix by running: `cargo fmt --all`

2. **Linting** (`cargo clippy`)
   - Checks for common mistakes and improvements
   - Enforces coding best practices
   - Fix by addressing the warnings/errors shown

## Installation

To install the hooks, run from the repository root:

```bash
./install-hooks.sh
```

This will copy the hooks from this directory to `.git/hooks/`.

## Bypassing Hooks

In rare cases where you need to bypass the hooks (not recommended), you can use:

```bash
git commit --no-verify
```

## Manual Hook Installation

If you prefer to install hooks manually:

```bash
cp hooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

## Running Checks Manually

You can run the same checks manually at any time:

```bash
# Format code
cargo fmt --all

# Check formatting
cargo fmt --all -- --check

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```
