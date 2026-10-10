#!/usr/bin/env python3
"""Assemble and validate fresh C64 artifacts before replacing a working build."""

import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory


ROOT = Path(__file__).resolve().parents[1]
KICK_JAR = ROOT / "toolchain/kickassembler/KickAss.jar"


class AssemblyError(Exception):
    """A compilation or generated artifact failed validation."""


def validate_prg(path: Path) -> tuple[int, int]:
    """Check the two-byte load address and the complete 64 KiB address range."""
    if not path.is_file():
        raise AssemblyError(f"KickAssembler did not generate a fresh PRG: {path}")
    data = path.read_bytes()
    if len(data) < 3:
        raise AssemblyError("Generated PRG must contain a load address and payload")
    start = int.from_bytes(data[:2], "little")
    end = start + len(data) - 2
    if end > 0x10000:
        raise AssemblyError(
            f"Generated PRG exceeds C64 RAM: ${start:04x}..${end - 1:05x}"
        )
    return start, end


def publish_artifacts(artifacts: list[tuple[Path, Path]], stage: Path) -> None:
    """Replace each file atomically, restoring the old set on a publish error.

    All validation precedes this function. The staging directory shares the
    destination filesystem, and the PRG is the last file published. Existing
    artifacts are copied before any replacement, so a failed os.replace does
    not leave an earlier replacement installed alongside the old PRG.
    """
    backup_directory = stage / "previous-artifacts"
    backup_directory.mkdir()
    backups = []
    for index, (_, destination) in enumerate(artifacts):
        backup = backup_directory / str(index)
        exists = destination.exists() or destination.is_symlink()
        if exists:
            if not destination.is_file() and not destination.is_symlink():
                raise AssemblyError(f"Artifact destination is not a file: {destination}")
            shutil.copy2(destination, backup, follow_symlinks=False)
        backups.append(backup if exists else None)

    published = []
    try:
        for index, (fresh, destination) in enumerate(artifacts):
            os.replace(fresh, destination)
            published.append(index)
    except OSError as error:
        rollback_errors = []
        for index in reversed(published):
            destination = artifacts[index][1]
            try:
                if backups[index] is None:
                    destination.unlink()
                else:
                    os.replace(backups[index], destination)
            except OSError as rollback_error:
                rollback_errors.append(f"{destination}: {rollback_error}")
        if rollback_errors:
            raise AssemblyError(
                "Artifact publication failed and rollback was incomplete: "
                + "; ".join(rollback_errors)
            ) from error
        raise AssemblyError(f"Artifact publication failed; old build restored: {error}") from error


def assemble(source: Path, output: Path) -> None:
    source = source.absolute()
    output = output.absolute()
    if not source.is_file():
        raise AssemblyError(f"Source file does not exist: {source}")
    if not KICK_JAR.is_file():
        raise AssemblyError(f"KickAssembler jar does not exist: {KICK_JAR}")
    destinations = [output.with_suffix(".vs"), output.with_suffix(".sym"), output]
    if len(set(destinations)) != 3 or source in destinations:
        raise AssemblyError("PRG and symbol outputs must have distinct paths from the source")
    output.parent.mkdir(parents=True, exist_ok=True)

    with TemporaryDirectory(prefix=f".{output.stem}-", dir=output.parent) as temporary:
        stage = Path(temporary)
        fresh_prg = stage / output.name
        command = [
            "java", "-jar", str(KICK_JAR), str(source),
            "-o", str(fresh_prg), "-odir", str(stage), "-vicesymbols",
        ]
        completed = subprocess.run(
            command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="replace", check=False,
        )
        sys.stdout.write(completed.stdout)
        sys.stdout.flush()
        if completed.returncode != 0:
            raise AssemblyError(f"KickAssembler exited with status {completed.returncode}")
        assertions = re.search(r"Made\s+\d+\s+asserts\s*,\s*(\d+)\s+failed", completed.stdout, re.I)
        if assertions and int(assertions.group(1)):
            raise AssemblyError(f"KickAssembler reported {assertions.group(1)} failed assertions")

        start, end = validate_prg(fresh_prg)
        # KickAssembler names symbol files after the input, even when -o uses
        # a different stem. Both files must be read only from this new stage.
        fresh_vs = stage / f"{source.stem}.vs"
        if not fresh_vs.is_file() or not fresh_vs.read_bytes().strip():
            raise AssemblyError("KickAssembler did not generate fresh non-empty VICE symbols")
        fresh_sym = stage / f"{source.stem}.sym"
        symbol_export_failed = (
            "Internal error while writing symbol file" in completed.stdout
            and "kickass.common.output.SymbolFile" in completed.stdout
        )
        if symbol_export_failed or not fresh_sym.is_file():
            # KickAss 5.25 can assemble a correct PRG and .vs while its
            # optional scope-rich exporter fails. Replace any prior .sym
            # with an empty file; symbol readers then use the fresh .vs.
            fresh_sym.write_bytes(b"")
        publish_artifacts(
            [(fresh_vs, destinations[0]), (fresh_sym, destinations[1]),
             (fresh_prg, destinations[2])],
            stage,
        )
        print(f"Published {output}: {end - start} bytes at ${start:04x}..${end - 1:04x}")
