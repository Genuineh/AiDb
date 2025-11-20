#!/bin/sh
#
# Install Git hooks for the AiDb project
#
# This script installs the pre-commit hook that runs cargo fmt and cargo clippy
# before each commit to ensure code quality.

echo "Installing Git hooks..."

# Check if we're in a Git repository
if [ ! -d ".git" ]; then
    echo "❌ Error: Not in a Git repository root directory"
    echo "Please run this script from the repository root"
    exit 1
fi

# Create hooks directory if it doesn't exist
mkdir -p .git/hooks

# Install pre-commit hook
if [ -f "hooks/pre-commit" ]; then
    cp hooks/pre-commit .git/hooks/pre-commit
    chmod +x .git/hooks/pre-commit
    echo "✓ Installed pre-commit hook"
else
    echo "❌ Error: hooks/pre-commit file not found"
    exit 1
fi

echo ""
echo "✅ Git hooks installed successfully!"
echo ""
echo "The following checks will run before each commit:"
echo "  - cargo fmt --all (code formatting)"
echo "  - cargo clippy (linting)"
echo ""
echo "To bypass these checks (not recommended), use: git commit --no-verify"
echo ""
