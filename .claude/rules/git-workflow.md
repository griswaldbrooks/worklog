# Git Workflow

## Commit Messages

- Use present tense ("Add feature" not "Added feature").
- Be concise but descriptive — one line summarizing the change.

## Before Committing

1. `cargo fmt --check` — must pass
2. `cargo clippy` — must pass with no warnings
3. Force push: always `--force-with-lease` with explicit branch name

## PR Titles

- Always capitalize the first word.
- Bug fixes: start with "Fix: " (e.g., "Fix: Crash when parsing empty worklog").
- New features: start with "Add " (e.g., "Add web form for new entries").
- Claude workflow changes: start with "Claude: " (e.g., "Claude: Updated CLAUDE.md with Rust conventions").

## Pull Requests

- Always create PRs as **draft** (`gh pr create --draft`). A human must transition it to "Ready for review."
- Commit messages must contain meaningful content.
- Never create custom milestones, labels, types, or other metadata fields on PRs or issues. Only select from existing ones.
