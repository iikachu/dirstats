---
name: gui-pr-screenshots
description: Ask for before/after screenshots on a pull request that changes the dirstats GUI. The PR body names the screenshots; the "PR screenshots" workflow posts them from CI as github-actions[bot]. Use when a PR touches crates/dirstats-gui, or when asked for screenshots, before/after images or visual evidence on a PR.
---

# Screenshots on a GUI pull request

You don't take, upload or host screenshots. You say which ones matter, and
CI posts them.

The `gui e2e` CI jobs run `crates/dirstats-gui/src/e2e.rs` on Ubuntu, Arch,
macOS and Windows and upload each `screenshot()` as an artifact. When CI
finishes on a PR, `.github/workflows/pr-screenshots.yml` compares those
images with the ones from `main` at the PR's base and posts one comment on
the PR, with before/after tables. It updates that comment on every push and
whenever the PR body changes.

**Never paste local screenshots into a PR.** The repository is public, and a
local run shows your paths and machine. Look at local PNGs (`target/e2e/`) only
to check your work.

## 1. Make sure the change is on screen

An e2e test must reach the changed state and call
`screenshot(&mut harness, "<name>")` there. Add or extend one with the
`add-gui-e2e-test` skill. The e2e fixtures have fixed names, so an unchanged
screen gives the same pixels on every run. Keep it that way: no timestamps,
random names or durations in a screenshot.

## 2. Name the screenshots in the PR body

Put this anywhere in the PR description:

```markdown
<!-- screenshots: context-menu-row, context-menu-row-zoomed -->
```

- Names are the `screenshot()` names without the OS suffix (lowercase, digits and `-`).
- Pick the few that show the change, not every screenshot the test takes.
- When a GUI change has nothing to show (a refactor, accessibility), write
  `<!-- screenshots: none -->` and say why in the PR body.

It's an HTML comment, so readers don't see it. You can edit it any time; the
bot reruns when the body changes.

## 3. Check the bot's comment

Once CI is green, open the PR and read the **Screenshots** comment:

- Each requested screenshot is shown for each OS where it changed, with
  "new in this PR" when `main` has no such image. Where it's pixel-identical
  to `main`, it says so.
- **Also changed (not requested)** lists screenshots you didn't ask for that
  changed anyway. Either add them to the block or find out why they moved.
- A warning means the block is missing, a name doesn't exist, or CI uploaded
  no images (the `gui e2e` jobs failed or didn't run).

Look at the images and tell the user what they show. If the comment is
missing, check the "PR screenshots" workflow run in the Actions tab.

## How the bot works (for changing it)

- Code: `.github/scripts/pr_screenshots.py`, run by `pr-screenshots.yml` as
  `github-actions[bot]` with `GITHUB_TOKEN`.
- It runs `main`'s copy of the workflow and never checks out PR code, so it
  is safe for forks. Keep it that way.
- Images go on the `ci-screenshots` branch, under `<yyyy-mm>/pr-<n>/<run id>/`.
  Only the bot writes there. A weekly job rewrites the branch as a single
  commit, dropping months older than 90 days.
- `real-disk*` screenshots scan the runner's own disk and differ on every
  run. They come from the `disk scan` jobs, whose `disk-scan-screenshots-*`
  artifacts the bot doesn't collect.
