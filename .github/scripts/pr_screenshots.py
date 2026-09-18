#!/usr/bin/env python3
"""Post a PR's GUI screenshots from CI as a before/after comment.

Run by .github/workflows/pr-screenshots.yml as github-actions[bot]. It only
reads data from the PR (its body and CI artifacts) and never runs its code.

    pr_screenshots.py post --run <CI run id>     after CI finishes on a PR
    pr_screenshots.py post --pr <number>         after the PR body is edited
    pr_screenshots.py prune                      drop images older than KEEP_DAYS

The PR body asks for screenshots with a block like

    <!-- screenshots: context-menu-row, fixture-zoomed -->

(or `none`). Names are e2e `screenshot()` names from crates/dirstats-gui/src/e2e.rs.
Images are hosted on the SHOTS_BRANCH branch, under <yyyy-mm>/pr-<n>/<run>/.
"""

import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageChops

REPO = os.environ["GITHUB_REPOSITORY"]
SHOTS_BRANCH = "ci-screenshots"
KEEP_DAYS = 90
MARKER = "<!-- dirstats-bot:pr-screenshots -->"
GUI_PATHS = ("crates/dirstats-gui/",)
# Screenshots of the runner's own disk: they differ on every run.
VOLATILE = ("real-disk",)
# Artifact suffix -> column label, in display order.
PLATFORMS = {
    "ubuntu-latest": "Ubuntu",
    "archlinux": "Arch",
    "macos-latest": "macOS",
    "windows-latest": "Windows",
}
MAX_PNG_BYTES = 5 * 1024 * 1024
NAME_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,63}$")
BLOCK_RE = re.compile(r"<!--\s*screenshots:\s*(.*?)\s*-->", re.S | re.I)


def gh(*args, parse=True):
    out = subprocess.run(["gh", *args], check=True, capture_output=True, text=True).stdout
    return json.loads(out) if parse and out.strip() else out


def git(*args, cwd):
    subprocess.run(["git", *args], cwd=cwd, check=True)


def parse_block(body):
    """(names, problems). names is None when the body has no block."""
    m = BLOCK_RE.search(body or "")
    if not m:
        return None, []
    names, problems = [], []
    for raw in re.split(r"[\s,]+", m.group(1).strip()):
        if not raw or raw.lower() == "none":
            continue
        (names if NAME_RE.match(raw) else problems).append(raw)
    return list(dict.fromkeys(names)), [f"`{p[:40].replace(chr(96), '')}` is not a valid screenshot name." for p in problems]


def download(run_id, dest):
    """{platform: {name: path}} from a run's e2e-screenshots-* artifacts."""
    shots = {}
    try:
        gh("run", "download", str(run_id), "-R", REPO, "-p", "e2e-screenshots-*", "-D", str(dest), parse=False)
    except subprocess.CalledProcessError:
        return shots
    for plat in PLATFORMS:
        d = dest / f"e2e-screenshots-{plat}"
        if not d.is_dir():
            continue
        for f in d.glob("*.png"):
            if f.is_symlink() or f.stat().st_size > MAX_PNG_BYTES:
                continue
            name = re.sub(r"-(linux|macos|windows)$", "", f.stem)
            if NAME_RE.match(name):
                shots.setdefault(plat, {})[name] = f
    return shots


def same_pixels(a, b):
    with Image.open(a) as x, Image.open(b) as y:
        if x.size != y.size:
            return False
        return ImageChops.difference(x.convert("RGBA"), y.convert("RGBA")).getbbox() is None


def base_run(pr):
    """The CI run on main at the PR's merge base, or None."""
    base = gh("api", f"repos/{REPO}/compare/{pr['baseRefOid']}...{pr['headRefOid']}", "--jq", ".merge_base_commit.sha", parse=False).strip()
    runs = gh("api", f"repos/{REPO}/actions/workflows/ci.yml/runs?head_sha={base}&event=push&branch=main&status=completed")
    runs = runs.get("workflow_runs", [])
    return (runs[0]["id"], base) if runs else (None, base)


def head_run(sha):
    runs = gh("api", f"repos/{REPO}/actions/workflows/ci.yml/runs?head_sha={sha}&event=pull_request&status=completed")
    runs = runs.get("workflow_runs", [])
    return runs[0]["id"] if runs else None


def find_pr_for_run(run_id):
    run = gh("api", f"repos/{REPO}/actions/runs/{run_id}")
    if run["event"] != "pull_request":
        return None, None
    prs = gh("pr", "list", "-R", REPO, "--state", "open", "--search", run["head_sha"], "--json", "number,headRefOid")
    for p in prs:
        # A run for an older push must not overwrite the newer comment.
        if p["headRefOid"] == run["head_sha"]:
            return p["number"], run_id
    return None, None


def publish(files, folder):
    """Commit files ({relative path: source}) under folder on SHOTS_BRANCH, return the raw URL base."""
    with tempfile.TemporaryDirectory() as tmp:
        wt = Path(tmp)
        url = f"https://x-access-token:{os.environ['GH_TOKEN']}@github.com/{REPO}.git"
        git("init", "-q", cwd=wt)
        git("config", "user.name", "github-actions[bot]", cwd=wt)
        git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com", cwd=wt)
        git("remote", "add", "origin", url, cwd=wt)
        for attempt in range(5):
            has_branch = subprocess.run(["git", "fetch", "-q", "--depth=1", "origin", SHOTS_BRANCH], cwd=wt).returncode == 0
            if has_branch:
                git("checkout", "-q", "-B", SHOTS_BRANCH, "FETCH_HEAD", cwd=wt)
            else:
                git("checkout", "-q", "--orphan", SHOTS_BRANCH, cwd=wt)
            for rel, src in files.items():
                dst = wt / folder / rel
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src, dst)
            git("add", "-A", cwd=wt)
            git("commit", "-q", "--allow-empty", "-m", f"Add {folder}", cwd=wt)
            if subprocess.run(["git", "push", "-q", "origin", SHOTS_BRANCH], cwd=wt).returncode == 0:
                return f"https://raw.githubusercontent.com/{REPO}/{SHOTS_BRANCH}/{folder}"
            git("reset", "-q", "--hard", cwd=wt)
        sys.exit("could not push to " + SHOTS_BRANCH)


def img(url, rel):
    return f'<img src="{url}/{rel}" width="400">'


def render(head_id, base_id, base_sha, requested, problems, before, after, url):
    run_link = lambda i: f"[{i}](https://github.com/{REPO}/actions/runs/{i})"
    lines = [MARKER, "## Screenshots"]
    warn = lambda text: lines.extend(["", f"> [!WARNING]\n> {text}"])
    if requested is None:
        # Only reached when the PR touches the GUI (or was already commented on).
        warn("This PR changes the GUI but asks for no screenshots. Add "
             "`<!-- screenshots: name, ... -->` (or `none`) to the description; see the "
             "`gui-pr-screenshots` skill.")
    for p in problems:
        warn(p)
    if not after:
        warn("The CI run has no screenshots. Did the `gui e2e` jobs run?")

    all_names = sorted({n for s in after.values() for n in s} | {n for s in before.values() for n in s})
    for name in requested or []:
        if name not in all_names:
            warn(f"No e2e test takes a screenshot named `{name}`.")

    def section(name):
        rows, same = [], []
        for plat, label in PLATFORMS.items():
            a, b = after.get(plat, {}).get(name), before.get(plat, {}).get(name)
            if not a and not b:
                continue
            if a and b and same_pixels(a, b):
                same.append(label)
                continue
            bcell = img(url, f"before/{plat}/{name}.png") if b else ("new in this PR" if base_id else "no base run")
            acell = img(url, f"after/{plat}/{name}.png") if a else "removed in this PR"
            rows.append(f"| {label} | {bcell} | {acell} |")
        return rows, same

    shown = set()
    for name in requested or []:
        if name not in all_names:
            continue
        shown.add(name)
        rows, same = section(name)
        lines += ["", f"### `{name}`", ""]
        if rows:
            lines += ["| | Before (`main`) | After (this PR) |", "|---|---|---|", *rows]
        if same:
            lines.append(f"Unchanged on {', '.join(same)}.")
            if not rows:
                plat = next(p for p, l in PLATFORMS.items() if l == same[0])
                lines += ["", img(url, f"after/{plat}/{name}.png")]

    extra, extra_names = [], []
    for name in all_names:
        if name in shown or name.startswith(VOLATILE):
            continue
        rows, _ = section(name)
        if rows:
            extra_names.append(name)
            extra += ["", f"#### `{name}`", "", "| | Before (`main`) | After (this PR) |", "|---|---|---|", *rows]
    if extra:
        lines += ["", "<details><summary>Also changed (not requested)</summary>", *extra, "", "</details>"]
    elif requested == [] and not problems:
        lines += ["", "No screenshots requested, and no screenshot changed."]

    base_txt = f"{run_link(base_id)} on `main` at {base_sha[:7]}" if base_id else f"none: no CI run on `main` at {base_sha[:7]}"
    lines += ["", f"<sub>After: CI run {run_link(head_id)}. Before: {base_txt}. "
                  f"Posted by `.github/workflows/pr-screenshots.yml`, updated on each push. "
                  f"Images kept {KEEP_DAYS} days.</sub>"]
    return "\n".join(lines), sorted(shown) + extra_names


def upsert_comment(number, body):
    comments = gh("api", "--paginate", "--slurp", f"repos/{REPO}/issues/{number}/comments")
    mine = [c for page in comments for c in page
            if c["user"]["login"] == "github-actions[bot]" and c["body"].startswith(MARKER)]
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump({"body": body}, f)
    if mine:
        gh("api", "-X", "PATCH", f"repos/{REPO}/issues/comments/{mine[0]['id']}", "--input", f.name)
    else:
        gh("api", "-X", "POST", f"repos/{REPO}/issues/{number}/comments", "--input", f.name)


def post(args):
    if args.run:
        number, head_id = find_pr_for_run(args.run)
        if not number:
            print("run is not for an open PR's latest commit; nothing to do")
            return
    else:
        number = args.pr
    pr = gh("pr", "view", str(number), "-R", REPO, "--json", "number,body,baseRefOid,headRefOid,files,state")
    if pr["state"] != "OPEN":
        return
    if not args.run:
        head_id = head_run(pr["headRefOid"])
        if not head_id:
            print("CI has not finished on the head commit; the workflow_run trigger will post")
            return

    requested, problems = parse_block(pr["body"])
    touches_gui = any(f["path"].startswith(GUI_PATHS) for f in pr["files"])
    has_comment = any(MARKER in c["body"] for c in gh("pr", "view", str(number), "-R", REPO, "--json", "comments")["comments"])
    if requested is None and not touches_gui and not has_comment:
        print("no screenshots block and no GUI change; nothing to do")
        return

    base_id, base_sha = base_run(pr)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        after = download(head_id, tmp / "after")
        before = download(base_id, tmp / "before") if base_id else {}
        month = datetime.date.today().strftime("%Y-%m")
        folder = f"{month}/pr-{number}/{head_id}"
        url = f"https://raw.githubusercontent.com/{REPO}/{SHOTS_BRANCH}/{folder}"
        body, used = render(head_id, base_id, base_sha, requested, problems, before, after, url)
        files = {}
        for side, shots in (("before", before), ("after", after)):
            for plat, names in shots.items():
                for name in used:
                    if name in names:
                        files[f"{side}/{plat}/{name}.png"] = names[name]
        if args.dry_run:
            print(body, *sorted(files), sep="\n")
            return
        if files:
            publish(files, folder)
    upsert_comment(number, body)


def prune(_args):
    """Rewrite SHOTS_BRANCH as one commit holding only the last KEEP_DAYS of images."""
    cutoff = (datetime.date.today() - datetime.timedelta(days=KEEP_DAYS)).strftime("%Y-%m")
    with tempfile.TemporaryDirectory() as tmp:
        wt = Path(tmp)
        url = f"https://x-access-token:{os.environ['GH_TOKEN']}@github.com/{REPO}.git"
        if subprocess.run(["git", "clone", "-q", "--depth=1", "-b", SHOTS_BRANCH, url, str(wt)]).returncode:
            return
        old = [d for d in wt.iterdir() if re.match(r"^\d{4}-\d{2}$", d.name) and d.name < cutoff]
        if not old:
            return
        for d in old:
            shutil.rmtree(d)
        git("config", "user.name", "github-actions[bot]", cwd=wt)
        git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com", cwd=wt)
        git("checkout", "-q", "--orphan", "pruned", cwd=wt)
        git("add", "-A", cwd=wt)
        git("commit", "-q", "--allow-empty", "-m", f"Keep screenshots from {cutoff} on", cwd=wt)
        git("push", "-q", "--force", "origin", f"pruned:{SHOTS_BRANCH}", cwd=wt)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(required=True)
    p = sub.add_parser("post")
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--run", type=int)
    g.add_argument("--pr", type=int)
    p.add_argument("--dry-run", action="store_true", help="print the comment; push and post nothing")
    p.set_defaults(func=post)
    sub.add_parser("prune").set_defaults(func=prune)
    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
