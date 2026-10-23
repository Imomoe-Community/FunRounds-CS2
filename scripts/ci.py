"""Validate release metadata, package plugins, and publish successful CI builds."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(
    r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
)
PLATFORMS = {
    "windows-x64": ("funrounds_plugin.dll", "funrounds_plugin.dll", "funrounds-native.dll"),
    "linux-x64": ("libfunrounds_plugin.so", "funrounds_plugin.so", "libfunrounds-native.so"),
}


def metadata(root=ROOT):
    version = tomllib.loads((root / "Cargo.toml").read_text("utf-8"))["workspace"]["package"]["version"]
    match = SEMVER.fullmatch(version)
    if not match or any(part.isdigit() and len(part) > 1 and part[0] == "0" for part in (match[4] or "").split(".")):
        raise ValueError("workspace.package.version must be a valid SemVer")
    manifests = [root / "packaging/funrounds.pplugin", root / "native/funrounds-native/funrounds-native.pplugin"]
    main, native = [json.loads(path.read_text("utf-8")) for path in manifests]
    if any(manifest["version"] != version for manifest in (main, native)):
        raise ValueError("Cargo.toml and both plugin manifests must have the same version")
    dependency = next(item for item in main["dependencies"] if item["name"] == "funrounds_native")
    if dependency["constraints"] != f"={version}":
        raise ValueError("funrounds_native dependency must match the workspace version")
    sdk_versions = [next(item["constraints"] for item in manifest["dependencies"] if item["name"] == "s2sdk") for manifest in (main, native)]
    provider = json.loads((root / "bindings/s2sdk.pplugin").read_text("utf-8"))
    if sdk_versions != [f"={provider['version']}"] * 2:
        raise ValueError("S2SDK dependency versions must match the binding provider")
    packages = tomllib.loads((root / "Cargo.lock").read_text("utf-8"))["package"]
    for name in ("funrounds-core", "funrounds-plugin"):
        if next(package["version"] for package in packages if package["name"] == name) != version:
            raise ValueError("Cargo.lock workspace versions are stale; run cargo check")
    source = (root / "crates/funrounds-core/src/lib.rs").read_text("utf-8")
    label = re.search(r'pub const VERSION: &str = "([^"\n]+)";', source)
    display = label[1] if label else version
    if label:
        normalized = re.fullmatch(r"(\d+\.\d+(?:\.\d+)?)(?:-([A-Za-z0-9. -]+))?", display)
        if not normalized:
            raise ValueError("VERSION display label must identify the workspace version")
        number, suffix = normalized.groups()
        number += ".0" if number.count(".") == 1 else ""
        normalized_version = number + ("-" + suffix.lower().replace(" ", ".") if suffix else "")
        if normalized_version != version.split("+")[0]:
            raise ValueError("Update the core VERSION display label with the workspace version")
    return {"version": version, "tag": f"v{version}", "prerelease": bool(match[4]), "display": display}


def version_changed(event, current, root=ROOT):
    before = event["before"]
    if set(before) == {"0"}:
        return True
    if not re.fullmatch(r"[0-9a-f]{40,64}", before):
        raise ValueError("Invalid push base commit")
    subprocess.run(["git", "cat-file", "-e", f"{before}^{{commit}}"], cwd=root, check=True)
    exists = subprocess.check_output(["git", "ls-tree", "--name-only", before, "--", "Cargo.toml"], cwd=root, text=True)
    if not exists.strip():
        return True
    source = subprocess.check_output(["git", "show", f"{before}:Cargo.toml"], cwd=root, text=True)
    previous = tomllib.loads(source)["workspace"]["package"]["version"]
    return previous != current


def release_push(event, env):
    return (
        env.get("GITHUB_EVENT_NAME") == "push"
        and not event.get("deleted", False)
        and env.get("GITHUB_REF") == f"refs/heads/{event['repository']['default_branch']}"
    )


def package(platform, native_output, output_dir, root=ROOT):
    info = metadata(root)
    rust_name, rust_entry, native_name = PLATFORMS[platform]
    envs = "addons/plugify/envs"
    entries = {
        f"{envs}/funrounds/bin/{rust_entry}": root / "target/release" / rust_name,
        f"{envs}/funrounds/funrounds.pplugin": root / "packaging/funrounds.pplugin",
        f"{envs}/funrounds_native/bin/{native_name}": native_output / "bin" / native_name,
        f"{envs}/funrounds_native/funrounds-native.pplugin": native_output / "funrounds-native.pplugin",
        f"{envs}/funrounds_native/gamedata/funrounds-native.jsonc": native_output / "gamedata/funrounds-native.jsonc",
        "addons/plugify/configs/funrounds.example.toml": root / "config/funrounds.example.toml",
        "LICENSE": root / "LICENSE",
    }
    for path in (root / "licenses").rglob("*"):
        if path.is_file():
            entries[path.relative_to(root).as_posix()] = path
    for path in entries.values():
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Missing or empty package input: {path}")
    built_manifest = json.loads((native_output / "funrounds-native.pplugin").read_text("utf-8"))
    if built_manifest != json.loads((root / "native/funrounds-native/funrounds-native.pplugin").read_text("utf-8")):
        raise ValueError("Built native manifest differs from the source manifest")
    if (native_output / "gamedata/funrounds-native.jsonc").read_bytes() != (root / "native/funrounds-native/gamedata/funrounds-native.jsonc").read_bytes():
        raise ValueError("Built native gamedata differs from source")
    output_dir.mkdir(parents=True, exist_ok=True)
    archive = output_dir / f"funrounds-{info['version']}-{platform}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
        for name, path in sorted(entries.items()):
            bundle.write(path, name)
    return archive


def gh_json(endpoint, missing=False):
    result = subprocess.run(["gh", "api", endpoint], text=True, capture_output=True)
    if result.returncode:
        if missing and "(HTTP 404)" in result.stderr:
            return None
        raise RuntimeError(result.stderr.strip())
    return json.loads(result.stdout)


def check_linux_abi(paths):
    for path in paths:
        versions = subprocess.check_output(["readelf", "--version-info", str(path)], text=True)
        required = [tuple(map(int, value.split("."))) for value in re.findall(r"GLIBC_([0-9.]+)", versions)]
        if not required or max(required) > (2, 31):
            raise ValueError(f"Unsupported glibc requirement in {path}")
        dynamic = subprocess.check_output(["readelf", "-d", str(path)], text=True)
        if "Shared library: [libstdc++" in dynamic:
            raise ValueError(f"Unexpected shared C++ runtime in {path}")


def publish(artifact_dir, env=os.environ, root=ROOT):
    info = metadata(root)
    event = json.loads(Path(env["GITHUB_EVENT_PATH"]).read_text("utf-8"))
    if not release_push(event, env) or not version_changed(event, info["version"], root):
        raise ValueError("Publishing requires a version-changing push to the default branch")
    sha, repo, tag = env["GITHUB_SHA"], env["GITHUB_REPOSITORY"], info["tag"]
    if not re.fullmatch(r"[0-9a-f]{40,64}", sha) or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise ValueError("Invalid release repository or commit")
    assets = [artifact_dir / f"funrounds-{info['version']}-{platform}.zip" for platform in PLATFORMS]
    if any(not path.is_file() or path.stat().st_size == 0 for path in assets):
        raise ValueError("Both platform archives are required before publishing")
    reference = gh_json(f"repos/{repo}/git/ref/tags/{tag}", missing=True)
    if reference:
        obj = reference["object"]
        while obj["type"] == "tag":
            obj = gh_json(f"repos/{repo}/git/tags/{obj['sha']}")["object"]
        if obj["type"] != "commit" or obj["sha"] != sha:
            raise ValueError("Existing release tag points to a different commit; refusing to move it")
    release = gh_json(f"repos/{repo}/releases/tags/{tag}", missing=True)
    if release and not release["draft"]:
        available = {asset["name"] for asset in release["assets"] if asset["size"] > 0}
        if not reference or release["prerelease"] != info["prerelease"] or not {path.name for path in assets} <= available:
            raise ValueError("Published release is incomplete; refusing to overwrite it")
        print(f"{tag} already published for this commit")
        return
    if release:
        if release["target_commitish"] != sha:
            raise ValueError("Existing draft targets a different commit")
        command = ["gh", "release", "upload", tag, *map(str, assets), "--clobber", "--repo", repo]
    else:
        native = json.loads((root / "native/funrounds-native/funrounds-native.pplugin").read_text("utf-8"))
        dependencies = ", ".join(f"{item['name']} {item['constraints']}" for item in native["dependencies"])
        notes = (f"Requires Plugify with the Rust and C++ language modules, {dependencies}. "
                 "Dependencies are not bundled. Includes a generic funrounds.example.toml, not an active server configuration.")
        command = ["gh", "release", "create", tag, *map(str, assets), "--draft", "--target", sha,
                   "--title", info["display"], "--notes", notes, "--generate-notes", "--repo", repo]
        if info["prerelease"]:
            command += ["--prerelease"]
    subprocess.run(command, cwd=root, check=True)
    command = ["gh", "release", "edit", tag, "--draft=false", f"--prerelease={str(info['prerelease']).lower()}", "--repo", repo]
    if info["prerelease"]:
        command += ["--latest=false"]
    subprocess.run(command, cwd=root, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("version")
    pack = commands.add_parser("package")
    pack.add_argument("--platform", choices=PLATFORMS, required=True)
    pack.add_argument("--native-output", type=Path, required=True)
    pack.add_argument("--output-dir", type=Path, default=Path("dist"))
    release = commands.add_parser("release")
    release.add_argument("--artifacts", type=Path, default=Path("dist"))
    abi = commands.add_parser("abi")
    abi.add_argument("paths", type=Path, nargs="+")
    args = parser.parse_args()
    if args.command == "version":
        info = metadata()
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text("utf-8")) if os.environ.get("GITHUB_EVENT_PATH") else {}
        info["release"] = release_push(event, os.environ) and version_changed(event, info["version"])
        lines = [f"{key}={str(value).lower() if isinstance(value, bool) else value}" for key, value in info.items()]
        print("\n".join(lines))
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                output.write("\n".join(lines) + "\n")
    elif args.command == "package":
        print(package(args.platform, args.native_output, args.output_dir))
    elif args.command == "release":
        publish(args.artifacts)
    else:
        check_linux_abi(args.paths)


if __name__ == "__main__":
    main()
