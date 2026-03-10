---
name: wrapup
description: Wrap up session — update beads issues, documentation, and memory. Use at end of session, after completing work, or when user says "wrap up", "wrapup", or "land the plane".
user_invocable: true
---

# Wrap Up

Update all project state to reflect current reality. Context-dependent — assess what actually changed and update accordingly.

## 1. Beads Issues

```bash
bd list
```

- Close completed issues with `bd close <id> --reason "..."`
- Update in-progress issues with `bd update <id> --status open` if paused
- Create new issues for discovered work: `bd create "..." -p N --json`
- Fix stale dependencies with `bd dep add/remove`

## 2. Documentation

Assess which docs are affected by recent work. Not all apply every time:

| Doc | When to Update |
|---|---|
| `CLAUDE.md` | Architecture, commands, key details, or conventions changed |
| `.claude/rules/*.md` | Workflow or style conventions changed |
| `.claude/agents/*.md` | Agent capabilities or skills changed |
| `.claude/skills/*/SKILL.md` | Skill behavior changed |
| `PLAN.md` | Progress on plan items or plan changes |
| `README.md` | User-facing features or setup changed |

**Read before editing** — only update what actually changed. Don't touch docs that are already accurate.

## 3. Memory

Update the project memory file (MEMORY.md in the Claude projects memory directory):

- Architecture changes (new/removed modules, changed interfaces)
- Updated test counts
- New tools, dependencies, or conventions
- Current issue summary and remaining work
- Remove outdated information

Keep it under 200 lines. Link to topic files in the memory directory for detailed notes.

## 4. Git

Check for uncommitted changes and offer to commit:

```bash
git status
git diff --stat
```

Don't commit or push without user confirmation.

## Checklist

```
[ ] Beads issues reflect current state
[ ] Affected documentation updated
[ ] Memory file current and concise
[ ] No uncommitted work left behind
[ ] User informed of what changed
```
