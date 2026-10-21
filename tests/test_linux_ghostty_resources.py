#!/usr/bin/env python3
"""Packaged Ghostty resources reach the terminal: themes load and bash gets shell integration.

Stages resources with scripts/stage-ghostty-resources.sh into the isolated XDG_DATA_HOME, then
reads the spawned shell's initial environment from /proc. No input is sent to any terminal.
"""
import os
from pathlib import Path
import subprocess
import tempfile

from linux_app import running_app

REPO_ROOT = Path(__file__).resolve().parent.parent


def shell_environment(app):
    """Return the environment of the first child process that Ghostty spawned with its resources."""
    for pid in app.children():
        try:
            raw = Path(f"/proc/{pid}/environ").read_bytes()
        except OSError:
            continue
        pairs = (entry.split(b"=", 1) for entry in raw.split(b"\0") if b"=" in entry)
        environment = {key.decode(): value.decode(errors="replace") for key, value in pairs}
        if "GHOSTTY_SHELL_FEATURES" in environment or "TERM_PROGRAM" in environment:
            return environment
    return None


def launch(root, theme):
    """Write an isolated Ghostty config naming `theme` and return the running application context."""
    config = root / "config/ghostty"
    config.mkdir(parents=True, exist_ok=True)
    (config / "config").write_text(f"theme = {theme}\n")
    extra = {"SHELL": "/bin/bash"}
    # Local runs may use an isolated Broadway display; CI uses the harness default under Xvfb.
    # The harness owns XDG_RUNTIME_DIR, so link the Broadway server socket into it.
    if os.environ.get("GDK_BACKEND") == "broadway" and os.environ.get("BROADWAY_DISPLAY"):
        extra.update(GDK_BACKEND="broadway", BROADWAY_DISPLAY=os.environ["BROADWAY_DISPLAY"])
        port = int(os.environ["BROADWAY_DISPLAY"].lstrip(":")) + 1
        name = f"broadway{port}.socket"
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700, exist_ok=True)
        if not (runtime / name).exists():
            (runtime / name).symlink_to(Path(os.environ["XDG_RUNTIME_DIR"]) / name)
    return running_app(root, extra_environment=extra)


def main():
    with tempfile.TemporaryDirectory(prefix="cmux-ghostty-resources-") as directory:
        root = Path(directory)
        data = root / "data"
        subprocess.check_call([str(REPO_ROOT / "scripts/stage-ghostty-resources.sh"), str(data)])

        with launch(root, "Dracula") as app:
            found = {}

            def shell_started():
                environment = shell_environment(app)
                if environment is not None:
                    found.update(environment)
                return environment is not None

            app.wait_for(shell_started, "terminal shell spawn", timeout=20)
            assert found.get("GHOSTTY_RESOURCES_DIR") == str(data / "cmux/ghostty"), found.get("GHOSTTY_RESOURCES_DIR")
            assert found.get("TERM") == "xterm-ghostty", found.get("TERM")
            assert found.get("TERMINFO") == str(data / "cmux/terminfo"), found.get("TERMINFO")
            assert "GHOSTTY_BASH_INJECT" in found, "bash shell integration was not injected"
            assert "path" in found.get("GHOSTTY_SHELL_FEATURES", ""), found.get("GHOSTTY_SHELL_FEATURES")
        log = (root / "app.log").read_text(errors="replace")
        assert "ghostty config:" not in log, "named theme failed to load:\n" + log[-4000:]

        # Control: an unknown theme is reported, so the clean log above is meaningful.
        with launch(root, "cmux-no-such-theme") as app:
            def reported():
                return "cmux-no-such-theme" in (root / "app.log").read_text(errors="replace")

            app.wait_for(reported, "unknown theme diagnostic", timeout=20)
        log = (root / "app.log").read_text(errors="replace")
        assert "ghostty config:" in log, log[-4000:]
    print("ghostty resources, named theme and bash shell integration passed")


if __name__ == "__main__":
    main()
