"""Read-only structural precheck for the WorldDB Luna task plan.

Run: python WorldDB_1.0_Plancheck.py
The source/working-copy semantic comparison is the separate M0-02 docs verify.
Evidence references are checked syntactically only; real test/run artifacts must
be inspected and recorded in each milestone's gate review.
"""

from __future__ import annotations

import argparse
import csv
import re
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path


ROOT = Path(__file__).resolve().parent
PLAN = ROOT / "WorldDB_1.0_Luna_Arbeitsplan.md"
TASKS = ROOT / "WorldDB_1.0_Taskregister.tsv"
INVARIANTS = ROOT / "WorldDB_1.0_Invariantenabdeckung.tsv"
FOLLOWUPS = ROOT / "WorldDB_1.0_Folgebelege.tsv"
TASK_PATTERN = re.compile(r"^- \[([ x])\] \*\*(M\d+-\d+[a-z]?) – (.+?)\.\*\*", re.M)
COUNT_PATTERN = re.compile(r"beschreibt derzeit (\d+) Tasks")
VALID_STATES = {"PLANNED", "READY", "RUNNING", "WAITING_EXTERNAL", "BLOCKED", "DONE"}


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def concrete_refs(value: str) -> bool:
    refs = [part.strip() for part in value.split(";")]
    return bool(value) and all(
        re.fullmatch(r"(?:test|artifact|run):.+", ref)
        and bool(ref.split(":", 1)[1].strip())
        and ref.split(":", 1)[1].strip().lower() not in {"todo", "tbd", "placeholder", "?", "-"}
        for ref in refs
    )


def checked_timestamp(value: str) -> datetime | None:
    """Accept only comparable ISO-8601 instants with an explicit UTC offset."""
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError:
        return None
    return parsed if parsed.tzinfo is not None and parsed.utcoffset() is not None else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gate-precheck", choices=[f"M{i}" for i in range(11)], help="check structural evidence due at this milestone; never a full gate approval")
    args = parser.parse_args()
    errors: list[str] = []
    plan = PLAN.read_text(encoding="utf-8")
    described = [(m.group(2), m.group(3), m.group(1)) for m in TASK_PATTERN.finditer(plan)]
    count_match = COUNT_PATTERN.search(plan)
    if not count_match or int(count_match.group(1)) != len(described):
        errors.append("Declared task count differs from plan task blocks")
    plan_by_id = {task_id: (title, checkbox) for task_id, title, checkbox in described}
    if len(plan_by_id) != len(described):
        errors.append("Duplicate task ID in plan")

    tasks = read_tsv(TASKS)
    task_by_id = {row["task_id"]: row for row in tasks}
    task_position = {row["task_id"]: index for index, row in enumerate(tasks)}
    if len(task_by_id) != len(tasks):
        errors.append("Duplicate task ID in task register")
    if set(plan_by_id) != set(task_by_id):
        errors.append(
            f"Plan/register ID difference: plan only={sorted(set(plan_by_id) - set(task_by_id))}, "
            f"register only={sorted(set(task_by_id) - set(plan_by_id))}"
        )
    if [row["task_id"] for row in tasks] != [task_id for task_id, _, _ in described]:
        errors.append("Task register order differs from plan order")

    by_milestone: dict[str, list[str]] = defaultdict(list)
    for row in tasks:
        task_id = row["task_id"]
        milestone = task_id.split("-")[0]
        by_milestone[milestone].append(task_id)
        if row["milestone"] != milestone:
            errors.append(f"{task_id}: wrong milestone")
        if task_id in plan_by_id and row["title"] != plan_by_id[task_id][0]:
            errors.append(f"{task_id}: title differs from plan")
        if row["status"] not in VALID_STATES:
            errors.append(f"{task_id}: invalid status {row['status']!r}")
        if task_id in plan_by_id and (plan_by_id[task_id][1] == "x") != (row["status"] == "DONE"):
            errors.append(f"{task_id}: checkbox/status mismatch")
        if row["status"] == "WAITING_EXTERNAL":
            last_check = checked_timestamp(row["last_check"])
            next_check = checked_timestamp(row["next_check"])
            if not row["external_run"] or not last_check or not next_check:
                errors.append(f"{task_id}: external run lacks ID or offset-aware last/next check")
            elif next_check <= last_check:
                errors.append(f"{task_id}: next check must be after last check")
        if row["status"] == "DONE" and not concrete_refs(row["evidence"]):
            errors.append(f"{task_id}: DONE lacks a typed evidence reference")

    running_ids = [row["task_id"] for row in tasks if row["status"] == "RUNNING"]
    if len(running_ids) > 1:
        errors.append(f"More than one RUNNING task: {running_ids}")

    dep_graph: dict[str, list[str]] = {}
    for row in tasks:
        task_id = row["task_id"]
        deps = [dep for dep in row["depends_on"].split(",") if dep]
        dep_graph[task_id] = deps
        if len(set(deps)) != len(deps):
            errors.append(f"{task_id}: duplicate dependency")
        for dep in deps:
            if dep not in task_by_id:
                errors.append(f"{task_id}: missing dependency {dep}")
            elif int(dep.split("-")[0][1:]) > int(row["milestone"][1:]):
                errors.append(f"{task_id}: dependency from a future milestone {dep}")
            elif task_position[dep] >= task_position[task_id]:
                errors.append(f"{task_id}: dependency is not earlier in the plan {dep}")
        if row["status"] == "READY" and any(
            dep not in task_by_id or task_by_id[dep]["status"] != "DONE" for dep in deps
        ):
            errors.append(f"{task_id}: READY despite open dependency")
        if row["status"] in {"RUNNING", "WAITING_EXTERNAL", "DONE"} and any(
            dep not in task_by_id or task_by_id[dep]["status"] != "DONE" for dep in deps
        ):
            errors.append(f"{task_id}: active or DONE despite open dependency")
        if row["status"] == "PLANNED" and all(
            dep in task_by_id and task_by_id[dep]["status"] == "DONE" for dep in deps
        ):
            errors.append(f"{task_id}: dependencies are DONE; status should be READY")

    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(task_id: str) -> None:
        if task_id in visiting:
            errors.append(f"Dependency cycle at {task_id}")
            return
        if task_id in visited:
            return
        visiting.add(task_id)
        for dep in dep_graph.get(task_id, []):
            if dep in task_by_id:
                visit(dep)
        visiting.remove(task_id)
        visited.add(task_id)

    for task_id in task_by_id:
        visit(task_id)

    for milestone, ids in by_milestone.items():
        expected_gate_dependencies = set(ids[:-1])
        if milestone == "M0":
            # M0-15 is an explicit local development pre-gate; external CI remains
            # mandatory before the RC architecture audit and final publication.
            expected_gate_dependencies.discard("M0-14")
        if milestone != "M10" and set(dep_graph[ids[-1]]) != expected_gate_dependencies:
            errors.append(f"{milestone}: gate dependencies differ from the declared milestone gate scope")
        if milestone != "M0":
            prior = f"M{int(milestone[1:]) - 1}"
            if by_milestone[prior][-1] not in dep_graph[ids[0]]:
                errors.append(f"{milestone}: first task must depend on previous gate")

    if "M0-14" not in dep_graph.get("M9-13b", []):
        errors.append("M9-13b must wait for the deferred M0-14 CI matrix before the RC architecture audit")
    if "M0-14" not in dep_graph.get("M10-10", []):
        errors.append("M10-10 must wait for the deferred M0-14 CI matrix before publication")

    gate_ids = {ids[-1] for milestone, ids in by_milestone.items() if milestone != "M10"}

    invariant_rows = read_tsv(INVARIANTS)
    followup_rows = read_tsv(FOLLOWUPS)
    followup_by_pair = {(row["invariant_id"], row["task_id"]): row for row in followup_rows}
    if len(followup_rows) != len(followup_by_pair):
        errors.append("Duplicate invariant/task pair in follow-up evidence register")
    expected_pairs: set[tuple[str, str]] = set()
    invariant_ids = [row["invariant_id"] for row in invariant_rows]
    if len(invariant_rows) != 253 or len(set(invariant_ids)) != 253:
        errors.append("Invariant matrix must contain exactly 253 unique IDs")
    for row in invariant_rows:
        invariant_id = row["invariant_id"]
        primary = row["primary_task"]
        if not re.fullmatch(r"WDB-[A-Z]+-\d{3}", invariant_id):
            errors.append(f"Malformed invariant ID {invariant_id}")
        if row["class"] not in {"HARD", "GUARDED"}:
            errors.append(f"{invariant_id}: invalid class")
        if row["status"] not in VALID_STATES:
            errors.append(f"{invariant_id}: invalid invariant status {row['status']!r}")
        if primary not in task_by_id:
            errors.append(f"{invariant_id}: missing primary task {primary}")
        elif primary in gate_ids:
            errors.append(f"{invariant_id}: primary evidence points to gate {primary}")
        followups = [item for item in row["followup_evidence_tasks"].split(",") if item]
        if len(set(followups)) != len(followups):
            errors.append(f"{invariant_id}: duplicate follow-up")
        for followup in followups:
            expected_pairs.add((invariant_id, followup))
            if followup not in task_by_id:
                errors.append(f"{invariant_id}: missing follow-up task {followup}")
            elif followup in gate_ids:
                errors.append(f"{invariant_id}: follow-up evidence points to gate {followup}")
            elif primary in task_position and task_position[followup] <= task_position[primary]:
                errors.append(f"{invariant_id}: follow-up {followup} is not after primary {primary}")
        if row["status"] == "DONE" and not (
            concrete_refs(row["positive_evidence"]) and concrete_refs(row["negative_evidence"])
        ):
            errors.append(f"{invariant_id}: DONE lacks concrete positive/negative evidence refs")
        if row["status"] == "DONE" and primary in task_by_id and task_by_id[primary]["status"] != "DONE":
            errors.append(f"{invariant_id}: DONE despite open primary task {primary}")

    if expected_pairs != set(followup_by_pair):
        errors.append(
            f"Follow-up register differs from matrix: missing={sorted(expected_pairs - set(followup_by_pair))}, "
            f"extra={sorted(set(followup_by_pair) - expected_pairs)}"
        )
    for pair, row in followup_by_pair.items():
        if row["status"] not in VALID_STATES:
            errors.append(f"{pair}: invalid follow-up status")
        if row["status"] == "DONE":
            if not (
                concrete_refs(row["evidence_ref"]) and row["result"] == "PASS"
                and checked_timestamp(row["checked_at"])
            ):
                errors.append(f"{pair}: DONE lacks concrete passed evidence and offset-aware check time")
            if row["task_id"] in task_by_id and task_by_id[row["task_id"]]["status"] != "DONE":
                errors.append(f"{pair}: DONE despite open follow-up task")

    if args.gate_precheck:
        milestone_ids = by_milestone[args.gate_precheck]
        gate_id = milestone_ids[-1]
        gate_position = task_position[gate_id]
        m0_14_status = task_by_id.get("M0-14", {}).get("status")
        deferred_ci = (
            int(args.gate_precheck[1:]) < 9
            and m0_14_status in {"BLOCKED", "WAITING_EXTERNAL"}
        )
        for task_id in milestone_ids[:-1]:
            if task_by_id[task_id]["status"] != "DONE":
                if args.gate_precheck == "M0" and task_id == "M0-14" and deferred_ci:
                    continue
                errors.append(f"{args.gate_precheck}: prerequisite task {task_id} is not DONE")
        if args.gate_precheck != "M0":
            prior_gate = by_milestone[f"M{int(args.gate_precheck[1:]) - 1}"][-1]
            if task_by_id[prior_gate]["status"] != "DONE":
                errors.append(f"{args.gate_precheck}: prior gate {prior_gate} is not DONE")
        for row in invariant_rows:
            primary = row["primary_task"]
            if primary in task_position and task_position[primary] < gate_position:
                if deferred_ci and row["invariant_id"] == "WDB-ENG-005":
                    continue
                if row["status"] != "DONE" or not (
                    concrete_refs(row["positive_evidence"]) and concrete_refs(row["negative_evidence"])
                ):
                    errors.append(f"{args.gate_precheck}: {row['invariant_id']} lacks primary positive/negative evidence")
            for followup in (item for item in row["followup_evidence_tasks"].split(",") if item):
                if followup in task_position and task_position[followup] < gate_position:
                    task = task_by_id[followup]
                    proof = followup_by_pair.get((row["invariant_id"], followup))
                    if task["status"] != "DONE" or not proof or not (
                        proof["status"] == "DONE"
                        and concrete_refs(proof["evidence_ref"])
                        and proof["result"] == "PASS"
                        and proof["checked_at"]
                    ):
                        errors.append(f"{args.gate_precheck}: {row['invariant_id']} follow-up {followup} lacks pair-specific evidence")

    if errors:
        for error in errors:
            print("ERROR:", error, file=sys.stderr)
        return 1
    print(
        f"STRUCTURE OK: {len(tasks)} tasks, {len(by_milestone)} milestones, "
        f"{len(invariant_rows)} invariants, {len(followup_rows)} follow-up pairs; DAG and references valid"
    )
    if args.gate_precheck:
        print("GATE PRECHECK ONLY: test/run/artifact existence, outcomes and semantic effectiveness are not verified here")
        if deferred_ci:
            print("DEFERRED RELEASE PREREQUISITE: M0-14 and WDB-ENG-005 remain open; M9-13b and M10-10 are blocked until CI passes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
