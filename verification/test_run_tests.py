"""The test gate must reject missing filters, ignored tests and failed commands."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class RustTestGate(unittest.TestCase):
    def test_actual_process_output_and_status(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'tests.log'
            for text, code, accepted in [
                ('test result: ok. 1 passed; 0 failed; 0 ignored;', 0, True),
                ('test result: ok. 0 passed; 0 failed; 1 ignored;', 0, False),
                ('test result: ok. 0 passed; 0 failed; 17 filtered out;', 0, False),
                ('compilation succeeded', 0, False),
                ('test result: FAILED. 0 passed; 1 failed;', 1, False),
                ('test result: ok. 1 passed; 0 failed;', 1, False),
            ]:
                with self.subTest(text=text, code=code):
                    command = [sys.executable, str(Path(__file__).with_name('run_tests.py')),
                               '--log', str(log), '--', sys.executable, '-c',
                               f'print({text!r}); raise SystemExit({code})']
                    result = subprocess.run(command, capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, accepted)
                    self.assertIn(text, log.read_text())


if __name__ == '__main__':
    unittest.main()
