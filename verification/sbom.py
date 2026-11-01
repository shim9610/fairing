#!/usr/bin/env python3
"""Generate feature/target-specific library BOMs and validate the official schema."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import urllib.request

from jsonschema import Draft7Validator, FormatChecker
from referencing import Registry, Resource
from referencing.jsonschema import DRAFT7

import install

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target/assurance/sbom"
OUT.mkdir(parents=True, exist_ok=True)
cache = install.BASE / "schemas"
cache.mkdir(parents=True, exist_ok=True)
resources = []
schemas = {}
for name, pin in install.PINS["sbom_schemas"].items():
    path = cache / name
    if not path.exists() or install.digest(path) != pin["sha256"]:
        with urllib.request.urlopen(pin["url"], timeout=60) as response:
            data = response.read()
        if hashlib.sha256(data).hexdigest() != pin["sha256"]:
            raise SystemExit("CycloneDX schema checksum mismatch")
        path.write_bytes(data)
    schema = json.loads(path.read_text())
    schemas[name] = schema
    resources.append((schema["$id"], Resource.from_contents(schema, default_specification=DRAFT7)))
validator = Draft7Validator(schemas["bom-1.5.schema.json"], registry=Registry().with_resources(resources), format_checker=FormatChecker())
binary = install.tool("cargo_cyclonedx")
lock_before = install.digest(ROOT / "Cargo.lock")
environment = dict(os.environ, SOURCE_DATE_EPOCH=subprocess.check_output(["git", "show", "-s", "--format=%ct", "HEAD"], cwd=ROOT, text=True).strip())
summaries = []


def cargo_graph(package, profile, target):
    command = ["cargo", "tree", "--locked", "-p", package, "--target", target,
               "--edges", "normal,build", "--prefix", "depth", "--format", "{p}"]
    if profile == "all-features":
        command.append("--all-features")
    text = subprocess.check_output(command, cwd=ROOT, text=True)
    parents = []
    edges = {}
    for line in text.splitlines():
        match = re.match(r"^(\d+)([\w-]+) v([^ ]+)", line)
        if not match:
            continue  # Cargo's build-dependency section headings.
        depth, name, version = match.groups()
        depth = int(depth)
        node = name, version
        edges.setdefault(node, set())
        parents = parents[:depth]
        if len(parents) != depth:
            raise SystemExit("Unexpected Cargo tree depth")
        if parents:
            edges[parents[-1]].add(node)
        parents.append(node)
    if not edges:
        raise SystemExit("Empty Cargo dependency graph")
    return edges, command


for profile in ["default", "all-features"]:
    for target in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]:
        filename = "fairing-assurance-generated"
        command = [str(binary), "cyclonedx", "--manifest-path", str(ROOT / "Cargo.toml"),
                   "--all", "--format", "json", "--spec-version", "1.5", "--target", target,
                   "--override-filename", filename]
        if profile == "all-features":
            command.append("--all-features")
        subprocess.run(command, cwd=ROOT, env=environment, check=True)
        destination = OUT / profile / target
        destination.mkdir(parents=True, exist_ok=True)
        for package, directory in [("fairing", ROOT / "crates/fairing"), ("fairing-widgets", ROOT / "crates/fairing-widgets"), ("xtask", ROOT / "xtask")]:
            generated = directory / (filename + ".json")
            data = json.loads(generated.read_text())
            validator.validate(data)
            # cargo-cyclonedx can include inactive optional packages from Cargo metadata.
            # Retain its original export, then project package components and edges onto
            # Cargo's normal/build graph for this package, target and feature selection.
            shutil.copy2(generated, destination / (package + ".raw.cdx.json"))
            graph, graph_command = cargo_graph(package, profile, target)
            root_component = data["metadata"]["component"]
            root_key = root_component["name"], root_component["version"]
            components = {(c["name"], c["version"]): c for c in data.get("components", [])}
            if len(components) != len(data.get("components", [])):
                raise SystemExit("Ambiguous package identities in the raw SBOM")
            missing = set(graph) - {root_key} - set(components)
            if missing:
                raise SystemExit(f"SBOM omitted resolved Cargo packages: {missing}")
            raw_count = len(components)
            data["components"] = [components[key] for key in sorted(set(graph) - {root_key})]
            # This is a package dependency BOM, rather than a BOM of example binaries.
            root_component.pop("components", None)
            references = {key: component["bom-ref"] for key, component in components.items() if key in graph}
            references[root_key] = root_component["bom-ref"]
            data["dependencies"] = [{"ref": references[key], "dependsOn": sorted(references[dep] for dep in graph[key])} for key in sorted(graph)]
            root_component.setdefault("properties", []).extend([
                {"name": "fairing:sbom:target", "value": target},
                {"name": "fairing:sbom:features", "value": profile},
                {"name": "fairing:sbom:edges", "value": "normal,build"},
            ])
            validator.validate(data)
            generated.write_text(json.dumps(data, indent=2) + "\n")
            names = {item["name"] for item in data.get("components", [])}
            if package != "xtask" and not names:
                raise SystemExit("Empty product dependency BOM")
            if names & {"loom", "proptest", "libfuzzer-sys", "fairing-assurance"}:
                raise SystemExit("Development/tool dependencies leaked into the library BOM")
            shutil.move(generated, destination / (package + ".cdx.json"))
            summaries.append({"package": package, "profile": profile, "target": target,
                              "raw_components": raw_count, "components": len(data.get("components", [])),
                              "graph_command": graph_command, "schema_valid": True})
        if install.digest(ROOT / "Cargo.lock") != lock_before:
            raise SystemExit("SBOM generation changed the approved product lockfile")
(OUT / "summary.json").write_text(json.dumps({"cargo_lock_sha256": lock_before, "tool": install.PINS["cargo_cyclonedx"], "boms": summaries}, indent=2) + "\n")
print(json.dumps(summaries, indent=2))
