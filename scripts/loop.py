#!/usr/bin/env python3
"""Loop orchestrator (docs/PLAN.md sections 15.2 to 15.12).

Usage:
  scripts/loop.py run [--task T0NN] [--dry-run]   run one task end to end
  scripts/loop.py status                          budgets, HALT, and recent outcomes

One task goes through: pick, test author, builder, reviewer (up to three rounds), gates,
merge into the branch checked out in the main checkout (normally main). Each role is a headless `claude -p` run in the task's own git worktree under
target/loop/. Roles only edit files; this script checks what each role changed, commits,
enforces budgets, and merges. It is the only part of the loop that uses judgment-free
authority over the repository.

Guardrails (PLAN 15.11):
  * loop/HALT stops everything before the next role starts.
  * Per-role, per-task, and per-day budgets in dollars, from loop/budgets.txt.
  * A shelve-rate limit: too many shelved tasks among the recent ones writes loop/HALT.
  * Roles get a fixed tool allowlist (no network tools, no git history changes), and the
    script rejects any change outside the files a role may touch.
  * Every merge records its spec, review, gate log, and cost in loop/metrics.jsonl and
    loop/runs/<task>/.

Picking: with --task, that task; else the spec in tasks/active/ if there is one; else the
first spec in tasks/backlog/ whose `**Depends on:**` tasks are all in tasks/done/.

Protected path: loop agents may not edit this file.
"""

import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOOP = ROOT / "loop"
TASKS = ROOT / "tasks"
WORK = ROOT / "target" / "loop"
HALT = LOOP / "HALT"
METRICS = LOOP / "metrics.jsonl"

PROTECTED = (".github/", "scripts/", "deny.toml", "loop/")
READ_TOOLS = ["Read", "Glob", "Grep", "Bash(git diff:*)", "Bash(git log:*)",
              "Bash(git status:*)", "Bash(ls:*)"]
WRITE_TOOLS = READ_TOOLS + ["Edit", "Write", "Bash(cargo build:*)", "Bash(cargo test:*)",
                            "Bash(cargo check:*)", "Bash(cargo fmt:*)", "Bash(cargo clippy:*)",
                            "Bash(cargo run:*)", "Bash(cargo doc:*)"]


class Shelve(Exception):
    """The task cannot finish. Carries the reason for the shelve report."""


class Stop(Exception):
    """The loop must stop: HALT, a budget, or a broken environment."""


# ---------------------------------------------------------------- small helpers

def sh(cmd, cwd=ROOT, check=True, env=None, timeout=None, stdin=None):
    result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env=env,
                            timeout=timeout, input=stdin)
    if check and result.returncode != 0:
        raise Stop(f"{' '.join(cmd)} failed:\n{result.stdout}\n{result.stderr}")
    return result


def git(*args, cwd=ROOT, check=True):
    return sh(["git", *args], cwd=cwd, check=check).stdout


def log(task, message):
    stamp = dt.datetime.now().strftime("%H:%M:%S")
    print(f"[{stamp}] {task}: {message}", flush=True)


def read_kv(path: Path) -> dict[str, str]:
    """Strict `key = value` text (DECISIONS.md D11): comments with #, no duplicate keys."""
    values: dict[str, str] = {}
    for n, line in enumerate(path.read_text().splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if "=" not in line:
            raise Stop(f"{path}:{n}: expected `key = value`")
        key, value = (part.strip() for part in line.split("=", 1))
        if key in values:
            raise Stop(f"{path}:{n}: duplicate key `{key}`")
        values[key] = value
    return values


def budgets() -> dict[str, float]:
    keys = ["role_usd", "task_usd", "day_usd", "role_timeout_s", "max_rounds",
            "shelve_window", "shelve_limit"]
    raw = read_kv(LOOP / "budgets.txt")
    unknown = set(raw) - set(keys)
    missing = set(keys) - set(raw)
    if unknown or missing:
        raise Stop(f"loop/budgets.txt: unknown {sorted(unknown)}, missing {sorted(missing)}")
    return {k: float(v) for k, v in raw.items()}


def metrics() -> list[dict]:
    if not METRICS.exists():
        return []
    return [json.loads(line) for line in METRICS.read_text().splitlines() if line.strip()]


def spent_today() -> float:
    today = dt.date.today().isoformat()
    return sum(m.get("cost_usd", 0.0) for m in metrics() if m.get("date") == today)


def check_halt():
    if HALT.exists():
        raise Stop(f"loop/HALT exists: {HALT.read_text().strip() or 'no reason given'}")


# ---------------------------------------------------------------- picking

def task_id(path: Path) -> str:
    return path.name.split("-", 1)[0]


def depends_on(spec: str) -> list[str]:
    match = re.search(r"\*\*Depends on:\*\*(.*)", spec)
    return re.findall(r"T\d{3}", match[1]) if match else []


def pick(wanted: str | None) -> Path:
    done = {task_id(p) for p in (TASKS / "done").glob("T*.md")}
    active = sorted((TASKS / "active").glob("T*.md"))
    if wanted:
        for folder in ("active", "backlog"):
            for path in sorted((TASKS / folder).glob(f"{wanted}-*.md")):
                return path
        raise Stop(f"no spec for {wanted} in tasks/active or tasks/backlog")
    if len(active) > 1:
        raise Stop(f"more than one active task: {[p.name for p in active]}")
    if active:
        return active[0]
    for path in sorted((TASKS / "backlog").glob("T*.md")):
        if all(dep in done for dep in depends_on(path.read_text())):
            return path
    raise Stop("no task is ready: tasks/active is empty and nothing in the backlog is unblocked")


# ---------------------------------------------------------------- roles

def role_env() -> dict[str, str]:
    env = dict(os.environ)
    occt = ROOT / "target" / "occt" / "install"
    if "OCCT_ROOT" not in env and occt.is_dir():
        env["OCCT_ROOT"] = str(occt)
    # One shared target directory keeps rebuilds incremental across tasks.
    env["CARGO_TARGET_DIR"] = str(WORK / "target")
    return env


def run_role(task, name, prompt, tree: Path, tools, limits, costs, run_dir: Path) -> str:
    check_halt()
    task_spent = sum(costs.values())
    room = min(limits["role_usd"], limits["task_usd"] - task_spent,
               limits["day_usd"] - spent_today() - task_spent)
    if room <= 0.05:
        raise Stop(f"budget exhausted before {name} (task ${task_spent:.2f}, "
                   f"day ${spent_today() + task_spent:.2f})")
    log(task, f"{name}: starting (budget ${room:.2f})")
    started = time.time()
    # The prompt goes on stdin: a diff can be longer than the command line allows.
    cmd = ["claude", "-p", "--output-format", "json", "--permission-mode", "dontAsk",
           "--allowedTools", *tools, "--max-budget-usd", f"{room:.2f}",
           "--strict-mcp-config", "--no-session-persistence"]
    try:
        result = sh(cmd, cwd=tree, check=False, env=role_env(),
                    timeout=limits["role_timeout_s"], stdin=prompt)
    except subprocess.TimeoutExpired:
        raise Shelve(f"{name} ran past {limits['role_timeout_s']:.0f} s")
    try:
        out = json.loads(result.stdout)
    except json.JSONDecodeError:
        raise Stop(f"{name}: claude did not return JSON:\n{result.stdout[-2000:]}\n{result.stderr[-2000:]}")
    cost = float(out.get("total_cost_usd") or 0.0)
    costs[name] = costs.get(name, 0.0) + cost
    text = out.get("result") or ""
    (run_dir / f"{name}.md").write_text(text)
    log(task, f"{name}: done in {time.time() - started:.0f} s, ${cost:.2f}")
    if out.get("is_error"):
        raise Shelve(f"{name} ended with an error: {out.get('subtype', '')} {text[-500:]}")
    return text


def changed_files(tree: Path, since: str) -> list[str]:
    tracked = git("diff", "--name-only", since, cwd=tree).split()
    untracked = git("ls-files", "--others", "--exclude-standard", cwd=tree).split()
    return sorted(set(tracked) | set(untracked))


def is_test_file(path: str) -> bool:
    parts = path.split("/")
    return len(parts) >= 4 and parts[0] == "crates" and parts[2] in ("tests", "golden")


def commit(tree: Path, message: str):
    git("add", "-A", cwd=tree)
    if git("status", "--porcelain", cwd=tree).strip():
        git("-c", "user.name=rustcad loop", "-c", "user.email=loop@rustcad.invalid",
            "commit", "-q", "-m", message, cwd=tree)


def prompt_for(role: str, spec: str, extra: str = "") -> str:
    instructions = (LOOP / "roles" / f"{role}.md").read_text()
    return f"{instructions}\n\n---\n\n# Task spec\n\n{spec}\n{extra}"


# ---------------------------------------------------------------- one task

def run_task(spec_path: Path, dry_run: bool):
    limits = budgets()
    check_halt()
    task = task_id(spec_path)
    spec = spec_path.read_text()
    title = spec.splitlines()[0].lstrip("# ").strip()
    if git("status", "--porcelain", "--untracked-files=no").strip():
        raise Stop("the main checkout has uncommitted changes; commit or stash them first")
    branch = git("rev-parse", "--abbrev-ref", "HEAD").strip()
    if branch == "HEAD":
        raise Stop("the main checkout has a detached HEAD; check out the branch to merge into")

    log(task, f"picked: {title} (merges into {branch})")
    if dry_run:
        log(task, "dry run: stopping before any role runs")
        return

    run_dir = LOOP / "runs" / task
    run_dir.mkdir(parents=True, exist_ok=True)
    tree = WORK / "worktrees" / task
    loop_branch = f"loop/{task}"
    if tree.exists():
        git("worktree", "remove", "--force", str(tree), check=False)
    git("branch", "-D", loop_branch, check=False)
    base = git("rev-parse", "HEAD").strip()
    git("worktree", "add", "-q", "-b", loop_branch, str(tree), base)

    costs: dict[str, float] = {}
    started = time.time()
    rounds = 0
    outcome, reason = "shelved", ""
    try:
        # Tests first (PLAN 15.4, step 3).
        run_role(task, "test-author", prompt_for("test-author", spec), tree, WRITE_TOOLS,
                 limits, costs, run_dir)
        test_files = changed_files(tree, base)
        bad = [f for f in test_files if not is_test_file(f)]
        if not test_files:
            raise Shelve("the test author wrote no tests")
        if bad:
            raise Shelve(f"the test author changed non-test files: {bad}")
        tests_now = sh(["cargo", "test", "--workspace"], cwd=tree, check=False, env=role_env())
        if tests_now.returncode == 0:
            raise Shelve("the new tests already pass before any implementation")
        commit(tree, f"{task}: acceptance tests")
        after_tests = git("rev-parse", "HEAD", cwd=tree).strip()

        findings = ""
        approved = False
        while rounds < limits["max_rounds"]:
            rounds += 1
            extra = "\n# Test author's files (do not edit)\n\n" + "\n".join(test_files)
            if findings:
                extra += f"\n\n# Reviewer findings to address\n\n{findings}"
            build_text = run_role(task, f"builder-{rounds}", prompt_for("builder", spec, extra),
                                  tree, WRITE_TOOLS, limits, costs, run_dir)
            touched = changed_files(tree, after_tests)
            forbidden = [f for f in touched if f in test_files or f.startswith(PROTECTED)
                         or f.startswith("tasks/")]
            if forbidden:
                raise Shelve(f"the builder changed files it may not touch: {forbidden}")
            commit(tree, f"{task}: implementation, round {rounds}")

            diff = git("diff", base, cwd=tree)
            review = run_role(task, f"reviewer-{rounds}",
                              prompt_for("reviewer", spec, f"\n# Diff\n\n```diff\n{diff}\n```"),
                              tree, READ_TOOLS, limits, costs, run_dir)
            if re.search(r"VERDICT:\s*APPROVE", review):
                approved = True
                break
            findings = review
        if not approved:
            raise Shelve(f"not approved after {rounds} rounds")

        # Verify (PLAN 15.4, step 7): every gate, against the base the task started from.
        log(task, "gates: running")
        # The gates use the tree's own target/ (they run probes from target/debug, and
        # cargo-mutants builds copies of the tree), so not the roles' shared target dir.
        env = {**role_env(), "GATES_BASE": base, "OCCT_REQUIRED": "1"}
        env.pop("CARGO_TARGET_DIR", None)
        active = TASKS / "active" / spec_path.name
        tree_active = tree / "tasks" / "active" / spec_path.name
        tree_active.parent.mkdir(parents=True, exist_ok=True)
        if not tree_active.exists():
            shutil.copy(spec_path, tree_active)  # the API gate reads the active spec
        gates = sh(["bash", "scripts/gates.sh"], cwd=tree, check=False, env=env)
        tree_active.unlink(missing_ok=True)
        (run_dir / "gates.log").write_text(gates.stdout + gates.stderr)
        if gates.returncode != 0:
            raise Shelve("gates failed; see loop/runs/" + task + "/gates.log")

        # Merge (step 8): one squashed commit on main with the spec moved and the roadmap ticked.
        subject, body = commit_message(build_text, title)
        git("merge", "--squash", loop_branch)
        done = TASKS / "done" / spec_path.name
        if spec_path != done:
            shutil.move(str(spec_path), done)
        tick_roadmap(task)
        git("add", "-A", "crates", "tasks", "ROADMAP.md")
        git("commit", "-q", "-m", subject + ("\n\n" + body if body else ""))
        outcome = "merged"
        log(task, f"merged: {subject}")
    except Shelve as why:
        reason = str(why)
        shelve(spec_path, task, reason, run_dir)
        log(task, f"shelved: {reason}")
    except Stop as why:
        # HALT or a budget: the task is not at fault, so its spec stays where it was.
        outcome, reason = "stopped", str(why)
        raise
    finally:
        record(task, title, outcome, reason, rounds, costs, started)
        save_records(task, outcome, reason)
        git("worktree", "remove", "--force", str(tree), check=False)
        if outcome == "merged":
            git("branch", "-D", loop_branch, check=False)
        if outcome != "stopped":
            rate_limit(limits)


def commit_message(build_text: str, title: str) -> tuple[str, str]:
    match = re.search(r"COMMIT:\s*(.+)", build_text)
    subject = match[1].strip() if match else title
    body = build_text[match.end():].strip() if match else ""
    # Commit messages describe the change only (AGENTS.md rule 6).
    # "model" and "agent" are ordinary words here (document model, MCP agents), so only
    # attribution phrases are filtered.
    banned = re.compile(r"co-authored-by|generated (with|by)|signed-off-by|\bclaude\b|anthropic|"
                        r"\bllm\b|language model|ai assistant", re.IGNORECASE)
    body = "\n".join(line for line in body.splitlines() if not banned.search(line)).strip()
    if banned.search(subject):
        subject = title
    return subject[:72], body


def save_records(task: str, outcome: str, reason: str):
    """Commits the run's records so main stays clean: into the merge commit when the task
    merged, otherwise in a commit of its own."""
    paths = ["loop/metrics.jsonl", f"loop/runs/{task}", "tasks"]
    git("add", "-A", *paths)
    if not git("diff", "--cached", "--name-only").strip():
        return
    if outcome == "merged":
        git("commit", "-q", "--amend", "--no-edit")
    else:
        verb = "Shelve" if outcome == "shelved" else "Record stopped run of"
        git("commit", "-q", "-m", f"{verb} {task}\n\n{reason[:500]}")


def tick_roadmap(task: str):
    path = ROOT / "ROADMAP.md"
    text = path.read_text()
    ticked = re.sub(rf"^- \[ \] {task}\b", f"- [x] {task}", text, flags=re.MULTILINE)
    path.write_text(ticked)


def shelve(spec_path: Path, task: str, reason: str, run_dir: Path):
    shelved = TASKS / "shelved" / spec_path.name
    if spec_path.exists() and spec_path != shelved:
        shutil.move(str(spec_path), shelved)
    report = run_dir / "shelved.md"
    report.write_text(f"# {task} shelved\n\n{reason}\n\nRole outputs are beside this file.\n")


def record(task, title, outcome, reason, rounds, costs, started):
    entry = {
        "date": dt.date.today().isoformat(),
        "task": task,
        "title": title,
        "outcome": outcome,
        "reason": reason,
        "rounds": rounds,
        "seconds": round(time.time() - started),
        "cost_usd": round(sum(costs.values()), 4),
        "cost_by_role": {k: round(v, 4) for k, v in costs.items()},
    }
    with METRICS.open("a") as f:
        f.write(json.dumps(entry) + "\n")


def rate_limit(limits):
    recent = metrics()[-int(limits["shelve_window"]):]
    shelved = sum(1 for m in recent if m["outcome"] == "shelved")
    if shelved >= limits["shelve_limit"]:
        HALT.write_text(f"{shelved} of the last {len(recent)} tasks were shelved "
                        f"({dt.datetime.now().isoformat(timespec='seconds')}). "
                        "Read loop/runs/*/shelved.md, then delete this file to resume.\n")


# ---------------------------------------------------------------- commands

def status():
    limits = budgets()
    print(f"HALT: {HALT.read_text().strip() if HALT.exists() else 'no'}")
    print(f"spent today: ${spent_today():.2f} of ${limits['day_usd']:.2f}")
    for m in metrics()[-5:]:
        print(f"  {m['date']} {m['task']} {m['outcome']} rounds={m['rounds']} "
              f"${m['cost_usd']:.2f} {m['reason']}")
    try:
        print(f"next: {pick(None).relative_to(ROOT)}")
    except Stop as why:
        print(f"next: none ({why})")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run")
    run.add_argument("--task")
    run.add_argument("--dry-run", action="store_true")
    sub.add_parser("status")
    args = parser.parse_args()
    try:
        if args.command == "status":
            status()
        else:
            if shutil.which("claude") is None:
                raise Stop("the claude CLI is not on PATH")
            run_task(pick(args.task), args.dry_run)
    except Stop as why:
        print(f"loop stopped: {why}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
