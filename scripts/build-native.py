#!/usr/bin/env python3
"""Build and stage the native plugin and its pinned, patched S2SDK provider."""

from __future__ import annotations

import argparse
import json
import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Optional, Sequence, Tuple


S2SDK_URL = "https://github.com/Mai-Ooizumi/plugify-plugin-s2sdk.git"
# funrounds branch; pin the patched provider used by both plugins and the bundle.
S2SDK_COMMIT = "dfa3a3e8e88a70872b66d379490d3228a48f6d81"
S2SDK_VERSION = "3.0.6"
SOURCESDK_URL = "https://github.com/Wend4r/sourcesdk.git"
SOURCESDK_COMMIT = "f3de33c58595c628c12b4ffaba76dfcd49b8763f"


class BuildError(RuntimeError):
    pass


def run(command: Sequence[str], *, cwd: Optional[Path] = None) -> None:
    print("+ " + subprocess.list2cmdline([str(part) for part in command]), flush=True)
    try:
        subprocess.run([str(part) for part in command], cwd=cwd, check=True)
    except FileNotFoundError as error:
        raise BuildError(f"Required executable was not found: {command[0]}") from error
    except subprocess.CalledProcessError as error:
        raise BuildError(f"Command failed with exit code {error.returncode}: {command[0]}") from error


def capture(command: Sequence[str], *, cwd: Optional[Path] = None) -> str:
    try:
        result = subprocess.run(
            [str(part) for part in command],
            cwd=cwd,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except FileNotFoundError as error:
        raise BuildError(f"Required executable was not found: {command[0]}") from error
    except subprocess.CalledProcessError as error:
        detail = error.stderr.strip() if error.stderr else ""
        suffix = f" ({detail})" if detail else ""
        raise BuildError(f"Command failed with exit code {error.returncode}: {command[0]}{suffix}") from error
    return result.stdout.strip()


def require_tool(name: str) -> str:
    executable = shutil.which(name)
    if executable is None:
        raise BuildError(f"Required tool is not on PATH: {name}")
    return executable


def git_output(repository: Path, *arguments: str) -> str:
    return capture(["git", "-C", str(repository), *arguments])


def checkout_commit(repository: Path, url: str, commit: str) -> None:
    if not repository.exists():
        repository.mkdir(parents=True)
    if not repository.is_dir():
        raise BuildError(f"SDK checkout path is not a directory: {repository}")

    if not (repository / ".git").exists():
        if any(repository.iterdir()):
            raise BuildError(f"Refusing to use a non-empty, non-Git SDK path: {repository}")
        run(["git", "init", "--quiet", str(repository)])
        run(["git", "-C", str(repository), "remote", "add", "origin", url])
    else:
        try:
            root = Path(git_output(repository, "rev-parse", "--show-toplevel")).resolve()
        except BuildError as error:
            raise BuildError(f"SDK path is not a usable Git checkout: {repository}") from error
        if root != repository.resolve():
            raise BuildError(f"SDK path resolves inside another checkout: {repository}")
        try:
            origin = git_output(repository, "remote", "get-url", "origin")
        except BuildError as error:
            raise BuildError(f"SDK checkout has no origin remote: {repository}") from error
        if origin.rstrip("/") != url.rstrip("/"):
            raise BuildError(f"SDK checkout origin does not match the pinned public source: {repository}")
        dirty = git_output(repository, "status", "--porcelain", "--ignore-submodules=all")
        if dirty:
            raise BuildError(f"SDK checkout contains local changes; choose a clean --work-dir: {repository}")

    try:
        current = git_output(repository, "rev-parse", "HEAD")
    except BuildError:
        current = ""
    if current != commit:
        run(["git", "-C", str(repository), "fetch", "--depth", "1", "origin", commit])
        run(["git", "-C", str(repository), "checkout", "--quiet", "--detach", "FETCH_HEAD"])

    actual = git_output(repository, "rev-parse", "HEAD")
    if actual != commit:
        raise BuildError(f"SDK checkout resolved to {actual}, expected pinned commit {commit}")


def ensure_sdk_sources(sdk: Path) -> Path:
    sourcesdk = sdk / "external" / "sourcesdk"
    if sourcesdk.exists() and (sourcesdk / ".git").exists():
        try:
            current = git_output(sourcesdk, "rev-parse", "HEAD")
        except BuildError:
            current = ""
    else:
        current = ""

    if current != SOURCESDK_COMMIT:
        run(["git", "-C", str(sdk), "submodule", "update", "--init", "--depth", "1", "--", "external/sourcesdk"])
    checkout_commit(sourcesdk, SOURCESDK_URL, SOURCESDK_COMMIT)

    run(
        [
            "git",
            "-C",
            str(sourcesdk),
            "submodule",
            "update",
            "--init",
            "--recursive",
            "--depth",
            "1",
            "--",
            "thirdparty/protobuf",
            "thirdparty/game_protobufs",
        ]
    )
    actual = git_output(sourcesdk, "rev-parse", "HEAD")
    if actual != SOURCESDK_COMMIT:
        raise BuildError(f"SourceSDK checkout resolved to {actual}, expected pinned commit {SOURCESDK_COMMIT}")
    return sourcesdk


def generate_s2sdk_imports(sdk: Path, generated_output: Path) -> Path:
    version_file = sdk / "version.txt"
    template_file = sdk / "plugify-plugin-s2sdk.pplugin.in"
    if not version_file.is_file() or not template_file.is_file():
        raise BuildError("Pinned S2SDK checkout is missing its version or plugin manifest template")

    version = version_file.read_text(encoding="utf-8").strip().removeprefix("v")
    if version != S2SDK_VERSION:
        raise BuildError(f"Pinned S2SDK source reports version {version}, expected {S2SDK_VERSION}")

    template = template_file.read_text(encoding="utf-8")
    replacements = {"${S2SDK_VERSION}": version, "${S2SDK_PACKAGE}": "s2sdk"}
    for placeholder, value in replacements.items():
        if placeholder not in template:
            raise BuildError(f"S2SDK manifest template is missing expected placeholder {placeholder}")
        template = template.replace(placeholder, value)
    if re.search(r"\$\{[^}]+\}", template):
        raise BuildError("S2SDK manifest template contains an unhandled CMake placeholder")

    manifest = generated_output.parent / "plugify-plugin-s2sdk.pplugin"
    manifest.parent.mkdir(parents=True, exist_ok=True)
    try:
        json.loads(template)
    except json.JSONDecodeError as error:
        raise BuildError(f"Rendered S2SDK plugin manifest is invalid JSON: {error}") from error
    manifest.write_text(template, encoding="utf-8")

    generator = require_tool("plugify-gen")
    generated_output.mkdir(parents=True, exist_ok=True)
    run([generator, "-manifest", str(manifest), "-output", str(generated_output), "-lang", "cpp", "-overwrite"])

    # plugify-gen nests C++ bindings beneath its standard provider include root.
    generated_include = generated_output / "external" / "plugify" / "include"
    for relative in (Path("s2sdk/s2sdk.cpp"), Path("s2sdk/s2sdk.hpp")):
        if not (generated_include / relative).is_file():
            raise BuildError(f"plugify-gen did not create required S2SDK import file: {relative}")
    return generated_include


def check_linux_compilers() -> Tuple[str, str]:
    compilers: list[str] = []
    for variable, fallback in (("CC", "gcc"), ("CXX", "g++")):
        requested = os.environ.get(variable, fallback)
        executable = shutil.which(requested)
        if executable is None:
            raise BuildError(f"{variable} compiler is not on PATH: {requested}")
        version = capture([executable, "-dumpfullversion"])
        match = re.match(r"(\d+)", version)
        if match is None or int(match.group(1)) < 14:
            raise BuildError(f"{variable} must be GCC 14 or newer for the SteamRT build; found {version or 'unknown'}")
        compilers.append(executable)
    return compilers[0], compilers[1]


def windows_generator() -> str:
    vswhere = shutil.which("vswhere")
    if vswhere is None:
        for variable in ("ProgramFiles(x86)", "ProgramFiles"):
            program_files = os.environ.get(variable)
            if program_files:
                candidate = Path(program_files) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
                if candidate.is_file():
                    vswhere = str(candidate)
                    break
    if vswhere is None:
        raise BuildError("Visual Studio was not located: vswhere is not available")

    version = capture([vswhere, "-latest", "-products", "*", "-property", "installationVersion"])
    match = re.match(r"^(\d+)\.", version)
    generators = {17: "Visual Studio 17 2022", 18: "Visual Studio 18 2026"}
    if match is None or int(match.group(1)) not in generators:
        raise BuildError(f"Unsupported Visual Studio installation version: {version or 'unknown'}")
    return generators[int(match.group(1))]


def build_s2sdk(sdk: Path, work_dir: Path, output_dir: Path, cmake: str, options: Sequence[str]) -> Path:
    run(["git", "-C", str(sdk), "submodule", "update", "--init", "--recursive", "--depth", "1", "--",
         "external/parallel-hashmap", "external/zydis"])
    build = work_dir / "build-s2sdk"
    run([cmake, "-S", str(sdk), "-B", str(build), *options, "-Dprotobuf_WITH_ZLIB=OFF",
         "-DCMAKE_SUPPRESS_REGENERATION=ON", "-DSOURCESDK_GAME_TARGET=cs2", "-DSOURCESDK_USE_ABI0=ON",
         f"-DCMAKE_LIBRARY_OUTPUT_DIRECTORY={build / 'bin'}", f"-DCMAKE_RUNTIME_OUTPUT_DIRECTORY={build / 'bin'}"])
    run([cmake, "--build", str(build), "--config", "Release", "--target", "plugify-plugin-s2sdk", "--parallel", "4"])
    name = "plugify-plugin-s2sdk.dll" if platform.system() == "Windows" else "libplugify-plugin-s2sdk.so"
    binary_dir = build / "bin" / "Release" if platform.system() == "Windows" else build / "bin"
    staged = output_dir / "s2sdk"
    (staged / "bin").mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary_dir / name, staged / "bin" / name)
    for name in ("plugify-plugin-s2sdk.pplugin", "gamedata.jsonc", "settings.jsonc", "translations.jsonc"):
        shutil.copy2(build / name, staged / name)
    licenses = {
        "S2SDK-GPL-3.0.txt": sdk / "LICENSE",
        "parallel-hashmap.txt": sdk / "external/parallel-hashmap/LICENSE",
        "Zydis.txt": sdk / "external/zydis/LICENSE",
        "Zycore.txt": sdk / "external/zydis/dependencies/zycore/LICENSE",
        "protobuf.txt": sdk / "external/sourcesdk/thirdparty/protobuf/LICENSE",
    }
    (staged / "licenses").mkdir(exist_ok=True)
    for name, path in licenses.items():
        shutil.copy2(path, staged / "licenses" / name)
    (staged / "source.json").write_text(json.dumps({
        "repository": S2SDK_URL, "commit": S2SDK_COMMIT, "version": S2SDK_VERSION,
        "sourcesdk_commit": SOURCESDK_COMMIT,
    }, indent=2) + "\n", encoding="utf-8")
    protobuf = build / "external/sourcesdk/thirdparty/protobuf"
    return protobuf / "Release/libprotobuf.lib" if platform.system() == "Windows" else protobuf / "libprotobuf.a"


def configure_and_build(repo_root: Path, work_dir: Path, output_dir: Path) -> Path:
    system = platform.system()
    if system not in {"Windows", "Linux"}:
        raise BuildError(f"Native builds are supported on Windows and Linux, not {system}")

    sdk = work_dir / "s2sdk"
    checkout_commit(sdk, S2SDK_URL, S2SDK_COMMIT)

    plugify_include = sdk / "external" / "plugify" / "include"
    polyhook_source = plugify_include / "polyhook" / "polyhook.cpp"
    if not polyhook_source.is_file():
        raise BuildError(f"Pinned S2SDK checkout is missing the required PolyHook provider: {polyhook_source}")

    ensure_sdk_sources(sdk)
    generated_output = work_dir / "generated-s2sdk" / "include"
    generated_include = generate_s2sdk_imports(sdk, generated_output)

    native_source = repo_root / "native" / "funrounds-native"
    build_dir = work_dir / "build"
    cmake = require_tool("cmake")
    configure = [
        cmake,
        "-S",
        str(native_source),
        "-B",
        str(build_dir),
        "-DCMAKE_SUPPRESS_REGENERATION=ON",
        f"-DFUNROUNDS_PLUGIFY_INCLUDE_DIR={plugify_include}",
        f"-DFUNROUNDS_S2SDK_SOURCE_DIR={sdk}",
        f"-DFUNROUNDS_S2SDK_GENERATED_INCLUDE_DIR={generated_include}",
        "-DSOURCESDK_GAME_TARGET=cs2",
        "-DSOURCESDK_USE_ABI0=ON",
    ]

    if system == "Windows":
        options = ["-G", windows_generator(), "-A", "x64"]
    else:
        gcc, gxx = check_linux_compilers()
        options = [
            "-DCMAKE_BUILD_TYPE=Release",
            f"-DCMAKE_C_COMPILER={gcc}",
            f"-DCMAKE_CXX_COMPILER={gxx}",
            "-DCMAKE_SHARED_LINKER_FLAGS=-static-libstdc++ -static-libgcc",
            "-DCMAKE_BUILD_WITH_INSTALL_RPATH=ON",
            "-DCMAKE_INSTALL_RPATH=$ORIGIN",
            "-DSOURCESDK_LINK_ENABLE_RPATH=OFF",
        ]
    protobuf = build_s2sdk(sdk, work_dir, output_dir, cmake, options)
    configure.append(f"-DFUNROUNDS_PROTOBUF_LIBRARY={protobuf}")
    configure.extend(options)

    run(configure)
    run([cmake, "--build", str(build_dir), "--config", "Release", "--target", "funrounds-native", "--parallel"])

    binary_name = "funrounds-native.dll" if system == "Windows" else "libfunrounds-native.so"
    candidates = [build_dir / "bin" / "Release" / binary_name, build_dir / "bin" / binary_name]
    binary = next((candidate for candidate in candidates if candidate.is_file()), None)
    if binary is None:
        searched = ", ".join(str(candidate) for candidate in candidates)
        raise BuildError(f"Native build succeeded but did not produce {binary_name}; searched {searched}")

    manifest = native_source / "funrounds-native.pplugin"
    gamedata = native_source / "gamedata" / "funrounds-native.jsonc"
    if not manifest.is_file() or not gamedata.is_file():
        raise BuildError("Native plugin manifest or gamedata is missing from the source tree")
    try:
        json.loads(manifest.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise BuildError(f"Native plugin manifest is invalid JSON: {error}") from error

    (output_dir / "bin").mkdir(parents=True, exist_ok=True)
    (output_dir / "gamedata").mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, output_dir / "bin" / binary_name)
    shutil.copy2(manifest, output_dir / manifest.name)
    shutil.copy2(gamedata, output_dir / "gamedata" / gamedata.name)
    return output_dir


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build and stage the native plugin and patched S2SDK for the current platform.")
    parser.add_argument("--work-dir", type=Path, default=Path(".work/native"), help="isolated SDK and build workspace")
    parser.add_argument("--output-dir", type=Path, default=Path(".work/native-output"), help="native plugin package root")
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    repo_root = Path(__file__).resolve().parents[1]
    work_dir = args.work_dir.resolve()
    output_dir = args.output_dir.resolve()
    work_dir.mkdir(parents=True, exist_ok=True)
    output_dir.mkdir(parents=True, exist_ok=True)

    try:
        result = configure_and_build(repo_root, work_dir, output_dir)
    except BuildError as error:
        print(f"build-native: error: {error}", file=sys.stderr)
        return 1
    print(f"Native plugin package staged at {result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
