# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Measure a machine's noise floor: the C reference of one workload, run many
times, and the coefficient of variation of its wall time.

    python3 bench/machine/noise.py [--workload nbody] [--size s] [--runs 50]

A pinned bare-metal core should come in under 1 %; the result goes into the
machine's profile as `measured_noise_cv`, and a publication box must measure
under the charter's `statistics.noisy_cv`.
"""

import argparse
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import run  # noqa: E402


def noise(workload_name, size, runs):
    """Wall times of `runs` cold runs of the workload's C port, and their CV."""
    args = run.parse_args(["--allow-dirty", "--langs", "c"])
    work = Path(tempfile.mkdtemp(prefix="miri-noise-"))
    ctx = run.Context(args, run.load_charter(), run.charter_hash(), "noise", work, Path("miri"))
    workload = next(w for w in run.load_workloads([workload_name]))
    built = run.build(ctx, workload.name, "c", workload.port("c"), "default")
    if not isinstance(built, run.Built):
        raise SystemExit(f"the C port of {workload_name} did not build: {built}")
    input_path = run.input_file(ctx, workload, size)
    env = run.child_environment(work)
    walls = [run.measure([*built.run_words, str(input_path)], env, work, 600).process_wall_ns for _ in range(runs)]
    return walls, run.coefficient_of_variation(walls)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--workload", default="nbody")
    parser.add_argument("--size", default="s")
    parser.add_argument("--runs", type=int, default=50)
    args = parser.parse_args(argv)
    walls, cv = noise(args.workload, args.size, args.runs)
    threshold = run.load_charter()["statistics"]["noisy_cv"]
    print(f"{args.workload} size {args.size}, C, {len(walls)} runs: median {sorted(walls)[len(walls) // 2] / 1e6:.1f} ms, CV {cv:.2%}")
    print(f"charter noisy_cv {threshold:.0%}: {'under' if cv < threshold else 'OVER'} the threshold")


if __name__ == "__main__":
    main()
