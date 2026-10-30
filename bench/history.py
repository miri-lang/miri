# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Measure released compilers on this tree's workloads, for the version line.

    python3 bench/history.py --tags 0.6.0-beta.4 0.7.0-beta.5 --profile preliminary-vps [--sizes m]

For every tag it exports that commit's source into a scratch directory with
`git archive` (the working tree is never touched), builds its release
compiler and runtime libraries with that tag's own `make release`, and runs
this tree's `bench/run.py` workloads for Miri alone with that compiler and
that tag's standard library. C and Rust are measured once, beside this
tree's compiler, in the same session, and every tag's ratios use those
references, so each point of the line is read against the same day's C.

A workload a tag cannot compile is recorded `unsupported` with the tag and
the first diagnostic, never dropped. Each tag's records go to
`bench/results/history/<tag>-<profile>/`, the current tree's to
`bench/results/history/current-<profile>/`, each with `env.json` naming the
tag, its commit and whether that compiler's optimisation level reached its
code generator. Only a publishable machine profile may record a point.
Standard library only.
"""

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

import run
import summarize

HISTORY = run.BENCH / "results" / "history"
REFERENCE_LANGUAGES = ("c", "rust")
# The commit that made --release and --opt-level reach the Cranelift code
# generator; a compiler built from a tree without it compiles at opt_level none.
CODEGEN_OPTIMIZATION_COMMIT = "9f4f190a83d862ccc0178c53b54b21b54e53e0c1"
# What a built compiler tree keeps once its binary and runtime libraries exist:
# everything else under a cargo target directory is intermediate.
INTERMEDIATE = ("deps", "build", "incremental", ".fingerprint", "examples")
MARKER = ".history-commit"


def publishable_profile(name):
    """The machine profile, refused unless its numbers may be published."""
    if not (run.BENCH / "machine" / f"{name}.toml").is_file():
        raise SystemExit(f"no machine profile named '{name}'")
    profile = run.load_profile(name)
    if not profile.get("publishable"):
        raise SystemExit(f"refusing to record a history point on '{name}': only a publishable profile may")
    return profile


def git(*words, check=True):
    done = subprocess.run(["git", *words], cwd=run.REPO, capture_output=True, text=True, check=False)
    if check and done.returncode != 0:
        raise SystemExit(f"git {' '.join(words)} failed: {done.stderr.strip()}")
    return done


def commit_facts(revision):
    """The commit `revision` names, when it was made, and whether its tree
    holds the change that made the optimisation level reach the code
    generator."""
    sha = git("rev-parse", f"{revision}^{{commit}}").stdout.strip()
    optimizing = git("rev-parse", f"{CODEGEN_OPTIMIZATION_COMMIT}^{{commit}}").stdout.strip()
    return {
        "commit": sha,
        "commit_time": git("show", "-s", "--format=%cI", sha).stdout.strip(),
        "codegen_optimized": git("merge-base", "--is-ancestor", optimizing, sha, check=False).returncode == 0,
        "runtime_profile": run.runtime_profile_of(git("show", f"{sha}:src/runtime/core/Cargo.toml").stdout),
    }


def tag_tree(tag, sha, work):
    """The tag's source, exported into `work/<tag>` and built in release the
    way that tag's Makefile builds it; reused when already built from `sha`."""
    tree = work / tag
    marker = tree / MARKER
    if marker.is_file() and marker.read_text().strip() == sha and (tree / "target" / "release" / "miri").is_file():
        return tree
    shutil.rmtree(tree, ignore_errors=True)
    tree.mkdir(parents=True)
    archive = subprocess.Popen(["git", "archive", sha], cwd=run.REPO, stdout=subprocess.PIPE)
    subprocess.run(["tar", "-x", "-C", str(tree)], stdin=archive.stdout, check=True)
    archive.stdout.close()
    if archive.wait() != 0:
        raise SystemExit(f"git archive {tag} failed")
    with open(work / f"build-{tag}.log", "w") as log:
        if subprocess.run(["make", "release"], cwd=tree, stdout=log, stderr=subprocess.STDOUT, check=False).returncode != 0:
            raise SystemExit(f"`make release` failed at {tag}; see {log.name}")
    prune(tree)
    marker.write_text(sha + "\n")
    return tree


def prune(tree):
    """Drop the intermediate build output, keeping the compiler and the
    runtime libraries it finds beside itself."""
    for target in [tree / "target" / "release", *(tree / "src" / "runtime").glob("*/target/release")]:
        for name in INTERMEDIATE:
            shutil.rmtree(target / name, ignore_errors=True)


def run_words(args, out, languages, miri=None, stdlib=None):
    """The bench/run.py command for one session."""
    words = [sys.executable, str(run.BENCH / "run.py"), "--langs", ",".join(languages), "--profile", args.profile]
    words += ["--results-dir", str(out)]
    for flag, value in (("--sizes", args.sizes), ("--only", args.only), ("--runs", args.runs)):
        if value:
            words += [flag, str(value)]
    if miri is not None:
        words += ["--miri", str(miri), "--miri-stdlib", str(stdlib), "--allow-unoptimized"]
    if args.allow_dirty:
        words.append("--allow-dirty")
    return words


def measure(words):
    if subprocess.run(words, cwd=run.REPO, check=False).returncode != 0:
        raise SystemExit(f"the session failed: {' '.join(words[1:3])}")


def mark_unsupported(records, tag):
    """A port the tag's compiler could not build is unsupported at that tag,
    with its first diagnostic."""
    for record in records:
        if record["language"] == "miri" and record["status"] == "build-failed" and record.get("run_index") is None:
            record["status"] = "unsupported"
            record["reason"] = f"unsupported ({tag}): {record.get('reason', 'no diagnostic was printed')}"
    return records


def with_references(records, reference_records, commit):
    """The tag's Miri records with this session's C and Rust records adopted
    under the tag's run id, each keeping the run it came from; every Miri
    record names the tag's commit as the compiler measured."""
    run_id = records[0]["run_id"]
    own = [r | {"miri_sha": commit} for r in records if r["language"] == "miri"]
    adopted = [r | {"run_id": run_id, "reference_run_id": r["run_id"]} for r in reference_records if r["language"] in REFERENCE_LANGUAGES]
    return own + adopted


def point_environment(env, facts, label, profile, sizes, tag=None, reference_run_id=None):
    """env.json of one point: the run's environment, the tag and commit it
    measured, and whether that compiler's optimisation reached the code
    generator."""
    point = {
        "tag": tag,
        "tag_sha": facts["commit"] if tag else None,
        "miri_sha": facts["commit"],
        "commit_time": facts["commit_time"],
        "codegen_optimized": facts["codegen_optimized"],
        "miri_runtime_profile": facts["runtime_profile"],
        "point_label": label,
        "label": profile.get("label"),
        "sizes": sizes or ",".join(run.SIZES),
    }
    if reference_run_id:
        point["reference_run_id"] = reference_run_id
    return env | point


def rewrite_point(out, records, env):
    (out / "env.json").write_text(json.dumps(env, indent=2, sort_keys=True) + "\n")
    with open(out / "results.jsonl", "w") as f:
        for record in records:
            f.write(json.dumps(record, sort_keys=True) + "\n")
    summarize.write_summary(out)


def read_point(out):
    return summarize.load_records(out), json.loads((out / "env.json").read_text())


def measure_current(args, profile, root):
    """This tree's compiler with the C and Rust references, measured once."""
    out = root / f"current-{args.profile}"
    measure(run_words(args, out, ["miri", *REFERENCE_LANGUAGES]))
    facts = commit_facts("HEAD")
    records, env = read_point(out)
    env = point_environment(env, facts, f"{facts['commit'][:8]} (current)", profile, args.sizes)
    rewrite_point(out, records, env)
    return records, env


def measure_tag(args, profile, root, tag, references):
    """One release's compiler, with the session's references adopted."""
    facts = commit_facts(tag)
    tree = tag_tree(tag, facts["commit"], Path(args.work).resolve())
    out = root / f"{tag}-{args.profile}"
    measure(run_words(args, out, ["miri"], tree / "target" / "release" / "miri", tree / "src" / "stdlib"))
    records, env = read_point(out)
    records = with_references(mark_unsupported(records, tag), references, facts["commit"])
    env = point_environment(env, facts, tag, profile, args.sizes, tag, references[0]["run_id"])
    rewrite_point(out, records, env)
    if not args.keep_trees:
        shutil.rmtree(tree / "target", ignore_errors=True)


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--tags", nargs="+", required=True, help="release tags, oldest first")
    parser.add_argument("--profile", required=True, help="a publishable machine profile")
    parser.add_argument("--sizes", help="comma-separated sizes: s, m, l (default: all)")
    parser.add_argument("--only", help="comma-separated workloads")
    parser.add_argument("--runs", type=int, help="measured runs per cell, in place of the charter's")
    parser.add_argument("--work", default=str(run.REPO / "target" / "history"), help="where each tag's tree is built")
    parser.add_argument("--results", default=str(HISTORY), help="where the points go")
    parser.add_argument("--keep-trees", action="store_true", help="keep each tag's built compiler")
    parser.add_argument("--allow-dirty", action="store_true", help="run from a tree with uncommitted changes")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    profile = publishable_profile(args.profile)
    root = Path(args.results)
    references, _ = measure_current(args, profile, root)
    for tag in args.tags:
        measure_tag(args, profile, root, tag, references)
    print(f"history points written under {root}")


if __name__ == "__main__":
    main()
