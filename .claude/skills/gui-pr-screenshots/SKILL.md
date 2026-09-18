---
name: gui-pr-screenshots
description: Put before/after screenshots of a GUI change into its pull request, taken from the "gui e2e" GitHub Actions job rather than a local run. Use when a PR touches crates/dirstats-gui, or when asked for screenshots, before/after images or visual evidence on a PR.
---

# Before/after screenshots on a GUI pull request

The `gui e2e` CI job runs `crates/dirstats-gui/src/e2e.rs` on Linux, macOS
and Windows and uploads each test's PNGs as `e2e-screenshots-<os>`. Those
artifacts are the screenshots for a PR: they come from clean runners, match
what reviewers can rerun, and show no local paths.

**Never publish local screenshots.** The repository is public, and a local
run shows your temp dir, scanned paths and machine in the toolbar and
footer. Look at local PNGs to check your work. Only CI's go in a PR.

## 1. Make sure the change is on screen

If the change is visible, an e2e test must reach that state and call
`screenshot(&mut harness, "<name>")` there (see the `add-gui-e2e-test`
skill). Fixtures come from `fixture_dir()`, which on Unix puts them under
`/tmp`, so the toolbar path is neutral. Push the branch and open the PR
first, because CI only runs on a pushed PR.

## 2. Download the artifacts

After: the latest `CI` run on the PR's head commit.

```bash
gh run list --workflow CI --branch "$BRANCH" --limit 1 --json databaseId,headSha,status,conclusion
```

```bash
gh run watch "$AFTER_RUN" --exit-status
```

```bash
gh run download "$AFTER_RUN" --pattern 'e2e-screenshots-*' --dir "$SCRATCH/after"
```

Before: the latest `CI` run on `main` at the PR's base commit (check with
`git merge-base origin/main HEAD`). It runs on every push to `main`.

```bash
gh run list --workflow CI --branch main --event push --limit 5 --json databaseId,headSha,conclusion
```

```bash
gh run download "$BEFORE_RUN" --pattern 'e2e-screenshots-*' --dir "$SCRATCH/before"
```

Each artifact unpacks to `<dir>/e2e-screenshots-<os>/<name>-<os>.png`.
Artifacts expire after 90 days. If the base run's artifacts are gone, rerun
the job with `gh run rerun "$BEFORE_RUN" --job <e2e job id>`.

Open the images and look at them before publishing. Check the change is
visible, and that nothing but `/tmp/dirstats-e2e-…`, runner paths
(`/home/runner`, `C:\Users\RUNNER~1\AppData\Local\Temp`) and fixture names shows up.

## 3. Host them

GitHub has no CLI upload for PR images, so the images go on the orphan
branch `pr-screenshots`, one folder per PR. It never merges, and keeps
PNGs out of `main` (where `*.png` is git-ignored).

```bash
git fetch origin pr-screenshots || true
```

```bash
git worktree add --detach "$SCRATCH/shots" && git -C "$SCRATCH/shots" switch --orphan pr-screenshots
```

Use `git -C "$SCRATCH/shots" switch pr-screenshots` instead when the fetch
found the branch. Copy the images to `pr-<N>/before/` and `pr-<N>/after/`,
then commit with `-f` (the ignore rule still applies) and push:

```bash
git -C "$SCRATCH/shots" add -f "pr-$N" && git -C "$SCRATCH/shots" commit -m "pr-$N: e2e screenshots from CI runs $BEFORE_RUN and $AFTER_RUN"
```

```bash
git -C "$SCRATCH/shots" push origin pr-screenshots
```

Link each image by commit, so later pushes to the branch never change what
the PR shows:
`https://raw.githubusercontent.com/<owner>/<repo>/<commit>/pr-<N>/after/<name>-<os>.png`.
Then run `git worktree remove "$SCRATCH/shots"`.

## 4. Edit the PR

Add a `## Screenshots` section to the body with `gh pr edit <N> --body-file`.
Keep the rest of the body. Use one table per screenshot name, with one row
per OS if they differ or one OS (Linux) if not:

```markdown
### context-menu-row
| Before (main) | After (this PR) |
|---|---|
| ![](…/before/context-menu-row-linux.png) | ![](…/after/context-menu-row-linux.png) |
```

- A screenshot the PR adds has no before. Write "new in this PR" in that
  cell rather than showing an unrelated image.
- If the change is not visible (accessibility, refactors), say so. Show one
  existing screenshot before and after as evidence that nothing moved.
- Name the CI runs the images came from, with links.

Rerun steps 2–4 when a later push changes what the screenshots show. Old
folders stay on `pr-screenshots`, since earlier comments may link to them.
