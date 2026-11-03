#!/usr/bin/env python3
"""Audit retained egui backend sites; this is a source inventory, not a refinement proof."""
import collections
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "crates/fairing-widgets/src"


def sites(source):
    # Only calls, not identifiers in prose. Include all temporary/persisted access variants.
    return dict(sorted(collections.Counter(
        ["backend." + m for m in re.findall(
            r"\.\s*((?:get|insert|remove)_(?:temp|persisted)\w*)\s*(?=\(|::<)", source)]
        + ["lifecycle." + m for m in re.findall(
            r"crate::interaction::(?:rules::)?(\w+)\s*(?=\(|::<)", source)]
    ).items()))


def audit():
    spec = json.loads((ROOT / "verification/retained-state.json").read_text())
    actual = {str(path.relative_to(BASE)): sites(path.read_text())
              for path in BASE.rglob("*.rs") if path.name != "tests.rs"}
    actual = {path: calls for path, calls in actual.items() if calls}
    expected = {path: item["sites"] for path, item in spec.items()}
    if actual != expected:
        differences = {path: {"reviewed": expected.get(path), "actual": actual.get(path)}
                       for path in actual.keys() | expected.keys()
                       if actual.get(path) != expected.get(path)}
        raise SystemExit("RETAINED_STATE_INVENTORY_CHANGED: review lifetime/operation coverage\n"
                         + json.dumps(differences, indent=2, sort_keys=True))
    return {"status": "passed", "scope": "Retained egui data and lifecycle call sites in fairing-widgets; "
            "does not cover arbitrary Rust fields, callbacks or upstream internals", "sites": spec}


if __name__ == "__main__":
    print(json.dumps(audit(), indent=2))
