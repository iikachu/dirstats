---
name: commit-style
description: How commits, branches and pull requests are written in the dirstats repository. Use whenever about to commit, push or open a PR here.
---

# Commits in this repository

## Subject line

`area: a sentence saying what is now true`

```
gui, tui: the scan status says "skipped", not "errors"
gui: the toolbar shows the app's name before a scan, not "No scan"
ntfs: whole NTFS drives are scanned from the master file table on Windows
readme: written for end users, showing what the app does and how to use it
```

- The area is the crate without its `dirstats-` prefix (`scan`, `treemap`,
  `ntfs`, `app`, `tui`, `gui`, `cli` for the binary) or a repository area
  (`ci`, `readme`, `credits`, `skills`, `gitignore`). Several areas are joined
  with a comma.
- After the colon, describe the resulting behaviour from the user's side, in
  lower case, present tense, no trailing full stop. Not "fix", "update",
  "refactor X": say what the app does now, and where it helps, what it did
  before ("..., not "No scan"").
- One behaviour per commit. If the subject needs "and", consider two commits.

## Body

Optional. Use it for why, for what was ported and from where (see the
`port-upstream-code` skill), or for a constraint a later reader must know.
Wrap at about 72 columns. End with the co-author line the session was given.

## Before committing

- Never commit on `main` directly; branch first. The working tree may be
  shared with other sessions, so prefer a separate `git worktree` for your
  branch over switching the shared tree's branch.
- Stage by path. `git status` may show other sessions' uncommitted work
  (files you did not touch); leave it alone and never `git add -A` in the
  shared tree.
- Never use bare `git stash`; the stash stack is shared across worktrees.
- Check the staged diff for personal data before every commit, since the
  repository is public:

```bash
git diff --cached | grep -n -iE '/Users/|/home/|C:\\\\Users|@[a-z0-9.-]+\.[a-z]{2,}'
```

  The only address that belongs in a commit is a `noreply` one. Screenshots
  can leak a home directory through the path shown in the toolbar; `*.png` is
  git-ignored for that reason, so do not force-add one.
- `git config user.name` and `user.email` should already be the project's
  GitHub identity with its `noreply` address. If they are not, stop and ask
  rather than committing under another name.

## Pull requests

Title in the same `area: sentence` form. Body: what changed, how it is gated
or limited, what is not covered, and what was actually run and where ("passes
on macOS locally; Linux and Windows first run in this PR's CI"). Do not merge
or enable auto-merge unless asked.
