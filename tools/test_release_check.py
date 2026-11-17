#!/usr/bin/env python3
"""Exercise fail-closed CI selection before a publishing credential is requested."""

import unittest
import os
from pathlib import Path
import subprocess
import tempfile

from release_check import REQUIRED, resolve, validate_checks


def checks():
    return [dict(id=index, name=name, status="completed", conclusion="success",
                 app={"slug": "github-actions"})
            for index, name in enumerate(sorted(REQUIRED), 1)]


class ReleaseChecks(unittest.TestCase):
    def test_all_required_checks(self):
        validate_checks(checks())

    def test_missing_check(self):
        with self.assertRaises(ValueError):
            validate_checks(checks()[1:])

    def test_unsuccessful_or_pending(self):
        for conclusion in ("failure", "cancelled", "skipped", None):
            with self.subTest(conclusion=conclusion):
                runs = checks()
                runs[0]["conclusion"] = conclusion
                with self.assertRaises(ValueError):
                    validate_checks(runs)
        runs = checks()
        runs[0]["status"] = "in_progress"
        with self.assertRaises(ValueError):
            validate_checks(runs)

    def test_failed_rerun_overrides_pass(self):
        runs = checks()
        runs.append(dict(runs[0], id=1000, conclusion="failure"))
        with self.assertRaises(ValueError):
            validate_checks(runs)

    def test_other_app_cannot_supply_required_check(self):
        runs = checks()
        runs[0]["app"] = {"slug": "another-app"}
        with self.assertRaises(ValueError):
            validate_checks(runs)

    def test_reject_ref_and_option_injection(self):
        for tag in ("main", "--help", "v1.0.0; echo bad", "v1.0.0/other", "v01.0.0", "v1.0.0-beta.0"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                resolve(tag)

    def test_tag_source_contract(self):
        with tempfile.TemporaryDirectory() as directory:
            previous = Path.cwd()
            os.chdir(directory)
            try:
                def git(*args):
                    return subprocess.check_output(["git", *args], stderr=subprocess.DEVNULL, text=True).strip()

                git("init", "-q")
                git("config", "user.name", "Release test")
                git("config", "user.email", "release@example.invalid")
                Path("Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0-beta.1"\n')
                for crate in ("fairing-widgets", "fairing"):
                    path = Path("crates") / crate / "Cargo.toml"
                    path.parent.mkdir(parents=True)
                    path.write_text(f'[package]\nname = "{crate}"\nversion.workspace = true\n')
                shell = Path("crates/fairing/Cargo.toml")
                with shell.open("a") as stream:
                    stream.write('[dependencies]\nfairing-widgets = { version = "0.1.0-beta.1" }\n')
                git("add", ".")
                git("commit", "-qm", "Release fixture")
                commit = git("rev-parse", "HEAD")
                git("update-ref", "refs/remotes/origin/main", commit)
                git("tag", "v0.1.0-beta.1")
                self.assertEqual(resolve("v0.1.0-beta.1"), commit)
                git("tag", "v0.1.0-beta.2")
                with self.assertRaises(ValueError):
                    resolve("v0.1.0-beta.2")
                shell.write_text(shell.read_text().replace('version = "0.1.0-beta.1"', 'version = "0.1.0-beta.2"'))
                git("add", ".")
                git("commit", "-qm", "Unmerged mismatching sibling")
                git("tag", "-f", "v0.1.0-beta.1")
                with self.assertRaises(subprocess.CalledProcessError):
                    resolve("v0.1.0-beta.1")
                git("update-ref", "refs/remotes/origin/main", "HEAD")
                with self.assertRaises(ValueError):
                    resolve("v0.1.0-beta.1")
            finally:
                os.chdir(previous)


if __name__ == "__main__":
    unittest.main()
