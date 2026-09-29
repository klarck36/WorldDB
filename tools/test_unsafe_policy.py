from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class UnsafePolicyTests(unittest.TestCase):
    def test_forbidden_unsafe_code_fails_rustc(self):
        rustc = shutil.which("rustc")
        if rustc is None:
            rustup = shutil.which("rustup")
            if rustup is not None:
                resolved = subprocess.run(
                    [rustup, "which", "rustc"], capture_output=True, text=True, check=False
                )
                if resolved.returncode == 0:
                    candidate = resolved.stdout.strip()
                    if candidate and Path(candidate).is_file():
                        rustc = candidate
        self.assertIsNotNone(rustc, "the declared Rust toolchain must provide rustc")
        source = "#![forbid(unsafe_code)]\nfn main() { unsafe { let _ = 1; } }\n"
        with tempfile.TemporaryDirectory(prefix="worlddb-unsafe-policy-") as directory:
            source_path = Path(directory) / "forbidden.rs"
            output_path = Path(directory) / "forbidden.rmeta"
            source_path.write_text(source, encoding="utf-8")
            completed = subprocess.run(
                [rustc, "--edition=2024", "--emit=metadata", "-o", str(output_path), str(source_path)],
                capture_output=True,
                text=True,
                check=False,
            )
        self.assertNotEqual(completed.returncode, 0, "forbid(unsafe_code) must reject the fixture")
        self.assertIn("unsafe", completed.stderr.lower())


if __name__ == "__main__":
    unittest.main()
