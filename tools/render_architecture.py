#!/usr/bin/env python3
"""Render architecture DOT sources as standalone SVGs with titles and descriptions."""
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
DIAGRAMS = ROOT / "docs/diagrams"
SVG = "http://www.w3.org/2000/svg"
ET.register_namespace("", SVG)

DESCRIPTIONS = {
    "system-architecture": ("Fairing system architecture", "Application-supplied host, UI and adapters connect to library-owned shell state, widgets and rendering. Device I/O and persistence remain application integrations."),
    "runtime-flow": ("Fairing host update flow", "Logic-only updates and UI frames share state processing. Matching UI frames reuse prepared state; both paths can produce events without requiring continuous redraw."),
    "security-architecture": ("Fairing protected security architecture", "The application verifier owns session mutation. Read-only tokens and typed requirements feed the common mediation kernel before managed application payload entry. This is an API boundary within one process."),
    "session-lifecycle": ("Fairing session lifecycle", "Live publication, busy reads, terminal issuer revocation and local detachment are distinct. A matching issuer callback handles other token readers; new login creates a new pair."),
}


def main():
    for name, (title, description) in DESCRIPTIONS.items():
        source = DIAGRAMS / (name + ".dot")
        result = subprocess.run(["dot", "-Tsvg", str(source)], check=True, capture_output=True, text=True)
        raw = re.sub(r"<!DOCTYPE[^>]*>\s*", "", result.stdout)
        root = ET.fromstring(raw)
        for text in root.iter("{" + SVG + "}text"):
            text.set("font-family", "DejaVu Sans, Arial, sans-serif")
        root.set("role", "img")
        root.set("aria-labelledby", "diagram-title diagram-description")
        for index, (tag, identity, text) in enumerate([
            ("title", "diagram-title", title), ("desc", "diagram-description", description)
        ]):
            element = ET.Element("{" + SVG + "}" + tag, {"id": identity})
            element.text = text
            root.insert(index, element)
        destination = source.with_suffix(".svg")
        ET.ElementTree(root).write(destination, encoding="utf-8", xml_declaration=True)
        print(destination.relative_to(ROOT))


if __name__ == "__main__":
    main()
