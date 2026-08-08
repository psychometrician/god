"""Put the engine inside the wheel.

**This is what makes an installed copy self-contained.** ``run.py`` asks for
``god/bin/god-cli`` first, and nothing else in the package ever creates that
file, so without this hook the lookup falls through to a development checkout:
fine on the machine that built it, useless anywhere else.

Two things this has to get right, and both are recorded because the obvious
version of each is wrong.

**The build happens in an isolated copy of the source.** ``python -m build`` and
``uv build`` both copy the project to a temporary tree first, and the engine is
not in that copy, because ``bin/`` is gitignored and not part of the sdist. So
the engine is found by walking up from *this file's* real location rather than
from the working directory, and ``GOD_CLI`` overrides that for a build running
somewhere else entirely.

**A wheel carrying a binary is not pure Python.** Left alone, setuptools tags it
``py3-none-any``, which tells every other platform on earth that the wheel is
theirs. It is not: it holds one Mach-O or ELF executable. ``root_is_pure = False``
is what makes the tag name the platform it was built for.
"""

import os
import shutil
from pathlib import Path

from setuptools import setup
from setuptools.command.build_py import build_py

try:  # setuptools >= 70 moved it; older installs still have the wheel package
    from setuptools.command.bdist_wheel import bdist_wheel
except ImportError:  # pragma: no cover - depends on the build environment
    from wheel.bdist_wheel import bdist_wheel

HERE = Path(__file__).resolve().parent


def engine() -> Path:
    """The built engine, or a message naming the command that makes one."""
    named = os.environ.get("GOD_CLI", "")
    if named and Path(named).is_file():
        return Path(named)

    # Climbed rather than counted, which is what both bindings do at run time.
    for directory in (HERE, *HERE.parents):
        for profile in ("release", "debug"):
            candidate = directory / "target" / profile / "god-cli"
            if candidate.is_file():
                return candidate

    found = shutil.which("god-cli")
    if found:
        return Path(found)

    raise SystemExit(
        "god: the wheel carries the engine, and there is no engine to carry.\n"
        "  Build it first:   cargo build --release\n"
        "  Or point at one:  GOD_CLI=/path/to/god-cli python -m build"
    )


class BuildWithEngine(build_py):
    def run(self) -> None:
        source = engine()
        destination = HERE / "god" / "bin"
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination / "god-cli")
        (destination / "god-cli").chmod(0o755)
        print(f"god: carrying the engine from {source}")
        super().run()


class PlatformWheel(bdist_wheel):
    def finalize_options(self) -> None:
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self):
        """One wheel per platform, not one per platform *and* Python version.

        `root_is_pure = False` alone produces `cp314-cp314-macosx_...`, because
        setuptools assumes an impure wheel holds a compiled extension bound to
        one interpreter. This one holds a standalone executable that any Python
        can run, so the interpreter tags go back to `py3-none` and a single file
        serves every 3.x on that platform.
        """
        _python, _abi, platform = super().get_tag()
        return "py3", "none", platform


setup(cmdclass={"build_py": BuildWithEngine, "bdist_wheel": PlatformWheel})
