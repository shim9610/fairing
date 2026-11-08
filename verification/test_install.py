#!/usr/bin/env python3
"""Offline controls for verified archive downloads under transport failures."""
import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

import install


class Downloads(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.archive = Path(self.directory.name) / "tool.zip"
        self.payload = b"complete pinned archive"
        self.pin = {"url": "https://example.invalid/tool.zip",
                    "asset_api_url": "https://example.invalid/releases/assets/123",
                    "sha256": hashlib.sha256(self.payload).hexdigest()}

    def error(self, code):
        return HTTPError(self.pin["url"], code, "test response", {}, None)

    def test_gateway_failure_then_verified_success(self):
        with patch.object(install.urllib.request, "urlopen", side_effect=[self.error(504), io.BytesIO(self.payload)]) as request, patch.object(install.time, "sleep") as sleep:
            install.download(self.pin, self.archive)
        self.assertEqual(request.call_count, 2)
        self.assertEqual(request.call_args_list[0].args[0].full_url, self.pin["url"])
        self.assertEqual(request.call_args_list[1].args[0].full_url, self.pin["asset_api_url"])
        self.assertEqual(request.call_args_list[1].args[0].get_header("Accept"), "application/octet-stream")
        sleep.assert_called_once_with(2)
        self.assertEqual(self.archive.read_bytes(), self.payload)
        self.assertFalse(self.archive.with_suffix(".zip.download").exists())

    def test_exhausted_retries_preserve_existing_archive(self):
        self.archive.write_bytes(b"previous archive")
        with patch.object(install.urllib.request, "urlopen", side_effect=self.error(504)) as request, patch.object(install.time, "sleep") as sleep:
            with self.assertRaises(HTTPError):
                install.download(self.pin, self.archive)
        self.assertEqual(request.call_count, 4)
        self.assertEqual([call.args[0] for call in sleep.call_args_list], [2, 4, 8])
        self.assertEqual(self.archive.read_bytes(), b"previous archive")

    def test_permanent_error_is_not_retried(self):
        with patch.object(install.urllib.request, "urlopen", side_effect=self.error(404)) as request, patch.object(install.time, "sleep") as sleep:
            with self.assertRaises(HTTPError):
                install.download(self.pin, self.archive)
        self.assertEqual(request.call_count, 1)
        sleep.assert_not_called()
        self.assertFalse(self.archive.exists())

    def test_checksum_failure_is_not_retried_or_published(self):
        with patch.object(install.urllib.request, "urlopen", return_value=io.BytesIO(b"wrong archive")) as request, patch.object(install.time, "sleep") as sleep:
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                install.download(self.pin, self.archive)
        self.assertEqual(request.call_count, 1)
        sleep.assert_not_called()
        self.assertFalse(self.archive.exists())
        self.assertFalse(self.archive.with_suffix(".zip.download").exists())


if __name__ == "__main__":
    unittest.main()
