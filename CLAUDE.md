# worklog — AI Assistant Instructions

## Quick Reference

- Do not create backup files (.bak, .backup, etc.) — this project uses Git.
- Always add a newline at the end of files.
- All new files: lowercase, no spaces, use hyphens
- The archive file (`Work Log Archive.md`) keeps its original name

## Purpose

This repo tracks Griswald's daily worklog at PickNik and hosts a Rust web server for managing the log via a local web UI.

## Rules

See @.claude/rules/git-workflow.md for commit conventions and PR requirements.

See @.claude/rules/rust-style.md for Rust style and conventions.

See @.claude/rules/testing.md for Rust testing patterns.

See @.claude/rules/agent-delegation.md for the coder/reviewer/test-runner pipeline.

## Agents

Subagent definitions live in `.claude/agents/` (`coder`, `code-reviewer`, `test-runner`). When agents are requested, prefer running the full pipeline: **coder → code-reviewer → test-runner**. This ensures code is reviewed and builds before being presented.

## Worklog Format

Entries are organized by ISO week number, then by date. Each day is a bullet list of activities.

```markdown
## Week N

Mon DD, YYYY

* Activity one
* Activity two
```

## Weekly Update Categories

When preparing updates for Rich (manager), include:
- **Mentoring**: Who, what, total weekly hour estimate
- **Hiring**: Weekly hours split by interviews, process improvement, and training others
- **AI adoption**: Who on team used AI tools and for how many PRs
- **Sprint goals**: Epic/story list (bi-weekly, before sprint planning on Thursday)

## Voice Workflow

Claude can accept worklog updates via voice (VoiceMode). When receiving voice updates:
- Add entries via the web UI or API
- Keep entries concise — bullet points, not paragraphs
- Preserve the person's voice/style but clean up filler words
