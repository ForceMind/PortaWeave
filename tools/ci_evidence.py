"""Audit saved GitHub Actions MAIN push run/job JSON; never starts or alters CI.

Input: {"source_commit": SHA, "workflows": [{"run": REST_RUN,
"jobs": REST_JOB_LIST}, ...]}. Snapshot completeness/freshness remains the
collector's responsibility. This is not a mount/product certification.
"""
import argparse
import json
from pathlib import Path
import re


def audit(bundle, expected_sha, expected_workflows=5, expected_jobs=14):
    errors = []
    if not re.fullmatch(r"[0-9a-f]{40}", expected_sha):
        raise ValueError("expected SHA must be a full lowercase commit SHA")
    if expected_workflows < 1 or expected_jobs < 1:
        raise ValueError("expected counts must be positive")
    if not isinstance(bundle, dict) or bundle.get("source_commit") != expected_sha:
        return {"status": "FAIL", "errors": ["bundle source_commit mismatch"], "product_certified": False}
    workflows = bundle.get("workflows")
    if not isinstance(workflows, list):
        return {"status": "FAIL", "errors": ["missing workflow list"], "product_certified": False}
    if len(workflows) != expected_workflows:
        errors.append("workflow count mismatch")
    run_ids, job_ids, workflow_ids = set(), set(), set()
    for entry in workflows:
        if not isinstance(entry, dict) or not isinstance(entry.get("run"), dict):
            errors.append("invalid workflow entry")
            continue
        run = entry["run"]
        rid = run.get("id")
        wid = run.get("workflow_id")
        if type(rid) is not int or rid <= 0 or rid in run_ids:
            errors.append("invalid/duplicate run id")
        else:
            run_ids.add(rid)
        if type(wid) is not int or wid <= 0 or wid in workflow_ids:
            errors.append("invalid/duplicate workflow id")
        else:
            workflow_ids.add(wid)
        if run.get("head_sha") != expected_sha or run.get("event") != "push" or run.get("head_branch") != "main":
            errors.append(f"run {rid}: wrong source/event/branch")
        if run.get("status") != "completed" or run.get("conclusion") != "success":
            errors.append(f"run {rid}: {run.get('status')}/{run.get('conclusion')}")
        jobs = entry.get("jobs")
        if not isinstance(jobs, list) or not jobs:
            errors.append(f"run {rid}: missing jobs")
            continue
        for job in jobs:
            if not isinstance(job, dict):
                errors.append(f"run {rid}: invalid job")
                continue
            jid = job.get("id")
            if type(jid) is not int or jid <= 0 or jid in job_ids:
                errors.append("invalid/duplicate job id")
            else:
                job_ids.add(jid)
            if job.get("run_id") != rid or job.get("head_sha") != expected_sha:
                errors.append(f"job {jid}: run/source mismatch")
            if job.get("status") != "completed" or job.get("conclusion") != "success":
                errors.append(f"job {jid}: {job.get('status')}/{job.get('conclusion')}")
            steps = job.get("steps")
            if not isinstance(steps, list) or not steps:
                errors.append(f"job {jid}: missing steps")
                continue
            if not any(isinstance(s, dict) and s.get("conclusion") == "success" for s in steps):
                errors.append(f"job {jid}: no successful step")
            for step in steps:
                if not isinstance(step, dict) or step.get("status") != "completed" or step.get("conclusion") not in ("success", "skipped"):
                    errors.append(f"job {jid}: unfinished/failed step")
    if len(job_ids) != expected_jobs:
        errors.append("job count mismatch")
    return {"status": "FAIL" if errors else "PASS", "source_commit": expected_sha,
            "workflows": len(run_ids), "jobs": len(job_ids), "errors": errors,
            "product_certified": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("snapshot", type=Path)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--workflows", type=int, default=5)
    parser.add_argument("--jobs", type=int, default=14)
    args = parser.parse_args()
    try:
        result = audit(json.loads(args.snapshot.read_text()), args.sha, args.workflows, args.jobs)
    except (ValueError, OSError) as exc:
        result = {"status": "FAIL", "errors": [str(exc)], "product_certified": False}
    print(json.dumps(result, ensure_ascii=False))
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
