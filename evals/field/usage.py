# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Where a run's effort went, read from its transcript.

A record says what a run cost and whether it ended green; it does not say what
the subject did on the way. This module reads the transcript a record keeps
beside it and answers that for a Miri arm:

- which diagnostic codes the compiler reported to the subject, and how many of
  them came with a repair;
- whether the subject applied repairs with `miri fix --apply` or edited by hand;
- which lookups it made (`miri view`, `miri explain`, `miri test`, `miri skill`);
- how many compile cycles failed before green, split by cause: a syntax guess,
  a type error, a missing import, a target restriction, a runtime trap.

Only what the compiler printed back counts. A code the subject read in the
skill pack or a document is not a code it hit.
"""

import json
import re
from collections import Counter

CODE = re.compile(r"\bMER_([A-Z]{2,4})_(\d{3})\b")
MIRI_COMMAND = re.compile(r"(?:^|[\s;&|(/])miri\s+([a-z][a-z-]*)")

# Commands that compile the subject's program and report what is wrong with it.
COMPILING = ("check", "build", "run", "test")
LOOKUPS = ("view", "explain", "skill", "test")

# The cause of a failed cycle, by the area of the first error it reported.
CAUSE_BY_AREA = {
    "LEX": "syntax",
    "PAR": "syntax",
    "TYP": "type",
    "OWN": "type",
    "IMP": "import",
    "NAM": "import",
    "TAR": "target",
    "RT": "runtime trap",
    "MIR": "compiler",
    "CG": "compiler",
    "BLD": "build",
}


def tool_calls(lines):
    """Every Bash tool call in a transcript, as (command, result text), in order."""
    pending = {}
    calls = []
    for line in lines:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        message = event.get("message") or {}
        for part in message.get("content") or []:
            if not isinstance(part, dict):
                continue
            if part.get("type") == "tool_use" and part.get("name") == "Bash":
                pending[part.get("id")] = (part.get("input") or {}).get("command", "")
            elif part.get("type") == "tool_result" and part.get("tool_use_id") in pending:
                calls.append((pending.pop(part["tool_use_id"]), result_text(part)))
    return calls


def result_text(part):
    content = part.get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(item.get("text", "") for item in content if isinstance(item, dict))
    return ""


def miri_subcommands(command):
    return MIRI_COMMAND.findall(command)


def reported_codes(text):
    """The error and warning codes a compiler's output reported, in order."""
    return ["MER_{}_{}".format(area, number) for area, number in CODE.findall(text)]


def error_codes(text):
    """The codes a compiler's output reported as errors, pretty or JSON."""
    pretty = re.findall(r"error\[(MER_[A-Z]{2,4}_\d{3})\]", text)
    json_errors = re.findall(
        r'"severity"\s*:\s*"error"[^{}]*?"code"\s*:\s*"(MER_[A-Z]{2,4}_\d{3})"', text
    )
    runtime = re.findall(r'"code"\s*:\s*"(MER_RT_\d{3})"', text)
    return pretty or json_errors or runtime


def repaired_codes(text):
    """The codes a JSON envelope reported with a machine-applicable repair."""
    found = []
    for match in re.finditer(r'"code"\s*:\s*"(MER_[A-Z]{2,4}_\d{3})"', text):
        tail = text[match.end() : match.end() + 1500]
        nearest = re.search(r'"code"\s*:', tail)
        scope = tail[: nearest.start()] if nearest else tail
        if '"repair"' in scope:
            found.append(match.group(1))
    return found


def run_usage(lines):
    """What a run did, read from its transcript lines."""
    diagnostics = Counter()
    repaired = Counter()
    lookups = Counter()
    explained = Counter()
    failed_cycles = Counter()
    applied = 0
    compiles = 0
    for command, text in tool_calls(lines):
        subcommands = miri_subcommands(command)
        if not subcommands:
            continue
        for sub in subcommands:
            if sub in LOOKUPS:
                lookups[sub] += 1
            if sub == "explain":
                explained.update(re.findall(r"MER_[A-Z]{2,4}_\d{3}", command))
            if sub == "fix" and "--apply" in command:
                applied += 1
        if not any(sub in COMPILING for sub in subcommands):
            continue
        compiles += 1
        errors = error_codes(text)
        diagnostics.update(reported_codes(text))
        repaired.update(repaired_codes(text))
        if errors:
            area = errors[0].split("_")[1]
            failed_cycles[CAUSE_BY_AREA.get(area, "other")] += 1
    return {
        "compiles": compiles,
        "diagnostics": dict(diagnostics),
        "repairedDiagnostics": dict(repaired),
        "fixApplied": applied,
        "lookups": dict(lookups),
        "explained": dict(explained),
        "failedCycles": dict(failed_cycles),
    }


def arm_usage(runs):
    """The usage of several runs of one arm, summed."""
    total = {
        "runs": len(runs),
        "compiles": 0,
        "fixApplied": 0,
        "diagnostics": Counter(),
        "repairedDiagnostics": Counter(),
        "lookups": Counter(),
        "explained": Counter(),
        "failedCycles": Counter(),
    }
    for usage in runs:
        total["compiles"] += usage["compiles"]
        total["fixApplied"] += usage["fixApplied"]
        for key in ("diagnostics", "repairedDiagnostics", "lookups", "explained", "failedCycles"):
            total[key].update(usage[key])
    reported = sum(total["diagnostics"].values())
    repaired = sum(total["repairedDiagnostics"].values())
    total["repairRatio"] = round(repaired / reported, 3) if reported else None
    for key in ("diagnostics", "repairedDiagnostics", "lookups", "explained", "failedCycles"):
        total[key] = dict(total[key].most_common())
    return total
