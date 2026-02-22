"""idio-loom: Python wrapper for the loom multi-agent turn engine."""

import json
import os
import signal
import subprocess
from pathlib import Path


class LoomError(Exception):
    """Base error for loom operations."""


class LoomTimeout(LoomError):
    """Turn timed out."""


class Loom:
    """Entry point. Points at a workspace directory containing loom.yaml."""

    def __init__(self, workspace=None, loom_bin=None):
        self.workspace = Path(workspace) if workspace else Path.cwd()
        self.loom_bin = loom_bin or "loom"

    def thread(self, name, model=None, timeout=900):
        """Return a Thread handle."""
        return Thread(self, name, model=model, timeout=timeout)

    def list_threads(self):
        """List all threads. Returns raw output string."""
        return self._run(["list"])

    def _run(self, args, timeout=30):
        """Run a loom CLI command and return stdout."""
        cmd = [self.loom_bin, "--dir", str(self.workspace)] + args
        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
                preexec_fn=os.setsid,
            )
        except subprocess.TimeoutExpired as e:
            raise LoomTimeout(f"loom command timed out after {timeout}s: {' '.join(args)}") from e

        if result.returncode != 0:
            raise LoomError(f"loom failed (exit {result.returncode}): {result.stderr.strip()}")

        return result.stdout


class Thread:
    """Handle to a named thread within a loom workspace."""

    def __init__(self, loom, name, model=None, timeout=900):
        self._loom = loom
        self.name = name
        self.model = model
        self.timeout = timeout

    def do(self, agent, nudge=None, *, hears=None, system=None, model=None, timeout=None):
        """Run a turn. Returns output string."""
        args = ["t", self.name, "do", agent]

        if hears:
            args.extend(["hears", hears])

        if nudge:
            args.append(nudge)

        effective_model = model or self.model
        if effective_model:
            args = ["--model", effective_model] + args

        if system:
            args = ["--use-system", system] + args

        effective_timeout = timeout or self.timeout
        return self._loom._run(args, timeout=effective_timeout + 30)  # +30s grace for process overhead

    @property
    def step(self):
        """Number of completed turns."""
        meta = self._read_meta()
        return len(meta.get("thread", []))

    @property
    def last_output(self):
        """Content of .last_output file."""
        run_dir = self._run_dir()
        path = run_dir / ".last_output"
        if not path.exists():
            raise LoomError(f"no .last_output for thread '{self.name}'")
        return path.read_text()

    def read(self, turn=None, input=False):
        """Read a turn's output (or input)."""
        args = ["t", self.name, "read"]
        if turn is not None:
            args.append(str(turn))
        if input:
            args.append("--input")
        return self._loom._run(args)

    def show(self):
        """Thread summary."""
        return self._loom._run(["t", self.name, "show"])

    def rewind(self, n):
        """Rewind to snapshot N."""
        self._loom._run(["t", self.name, "rewind", str(n)])

    def delete(self):
        """Delete thread entirely."""
        self._loom._run(["t", self.name, "delete"])

    def reset(self):
        """Reset thread (clear turns, keep name)."""
        self._loom._run(["t", self.name, "reset"])

    def _run_dir(self):
        """Resolve the thread's run directory by reading loom.yaml."""
        yaml_path = self._loom.workspace / "loom.yaml"
        if not yaml_path.exists():
            raise LoomError(f"no loom.yaml in {self._loom.workspace}")

        # Simple YAML parsing for state field (avoid pyyaml dependency)
        state = ".loom"
        for line in yaml_path.read_text().splitlines():
            line = line.strip()
            if line.startswith("state:"):
                state = line.split(":", 1)[1].strip()
                break

        state_path = Path(state)
        if not state_path.is_absolute():
            state_path = self._loom.workspace / state_path

        return state_path / "runs" / self.name

    def _read_meta(self):
        """Read meta.json for this thread."""
        meta_path = self._run_dir() / "meta.json"
        if not meta_path.exists():
            raise LoomError(f"thread '{self.name}' not found")
        return json.loads(meta_path.read_text())
