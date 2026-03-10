#!/usr/bin/env python3
"""Hook: PreToolUse (Bash) — Run cargo fmt check and clippy before allowing git commit.

A Claude Code hook is deterministic: it intercepts the tool call before execution
and blocks it if checks fail. This guarantees enforcement without consuming context
window tokens on every interaction.
"""

import json
import re
import subprocess
import sys


def split_segments(cmd: str) -> list[str]:
    """Split a shell command on unquoted &&, ;, and newline operators."""
    segments: list[str] = []
    current: list[str] = []
    in_single = False
    in_double = False
    i = 0
    while i < len(cmd):
        c = cmd[i]
        if c == "'" and not in_double:
            in_single = not in_single
        elif c == '"' and not in_single:
            in_double = not in_double
        elif not in_single and not in_double:
            if c in (";", "\n"):
                segments.append("".join(current))
                current = []
                i += 1
                continue
            if c == "&" and i + 1 < len(cmd) and cmd[i + 1] == "&":
                segments.append("".join(current))
                current = []
                i += 2
                continue
        current.append(c)
        i += 1
    segments.append("".join(current))
    return segments


def has_git_commit(cmd: str) -> bool:
    """Return True if any command segment contains 'git [options...] commit'."""
    return any(
        re.match(r"\s*git\s+(?:(?!commit)\S+\s+)*commit(?:\s|$)", seg)
        for seg in split_segments(cmd)
    )


def main() -> None:
    try:
        hook_input = json.loads(sys.stdin.read())
    except (json.JSONDecodeError, Exception):
        sys.exit(0)  # Don't block on parse failure

    command = hook_input.get("tool_input", {}).get("command", "")

    if not has_git_commit(command):
        sys.exit(0)

    cwd = hook_input.get("cwd")

    # Run cargo fmt check
    try:
        result = subprocess.run(
            ["cargo", "fmt", "--check"],
            capture_output=True,
            text=True,
            cwd=cwd,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        print(f"cargo fmt not found or failed to execute: {exc}", file=sys.stderr)
        sys.exit(2)

    if result.returncode != 0:
        print("cargo fmt --check failed. Fix formatting before committing:", file=sys.stderr)
        print("", file=sys.stderr)
        print(result.stdout + result.stderr, file=sys.stderr)
        sys.exit(2)

    # Run cargo clippy
    try:
        result = subprocess.run(
            ["cargo", "clippy", "--", "-D", "warnings"],
            capture_output=True,
            text=True,
            cwd=cwd,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        print(f"cargo clippy not found or failed to execute: {exc}", file=sys.stderr)
        sys.exit(2)

    if result.returncode != 0:
        print("cargo clippy failed. Fix warnings before committing:", file=sys.stderr)
        print("", file=sys.stderr)
        print(result.stdout + result.stderr, file=sys.stderr)
        sys.exit(2)

    sys.exit(0)


if __name__ == "__main__":
    main()
