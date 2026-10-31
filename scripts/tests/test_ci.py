import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import re
from unittest.mock import patch
import zipfile


SPEC = importlib.util.spec_from_file_location("ci", Path(__file__).resolve().parents[1] / "ci.py")
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)


class CITests(unittest.TestCase):
    def test_business_files_do_not_embed_machine_specific_paths(self):
        pattern = re.compile(r"(?<!\w)[A-Za-z]:[/\\]|/(?:home|Users)/[^/\s]+/")
        for directory in ("crates", "native", "config"):
            for path in (ci.ROOT / directory).rglob("*"):
                if path.is_file() and path.suffix in (".rs", ".cpp", ".hpp", ".asm", ".toml", ".txt", ".pplugin", ".jsonc"):
                    self.assertIsNone(pattern.search(path.read_text("utf-8")), str(path.relative_to(ci.ROOT)))

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.make_repo()

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, "utf-8")
        return target

    def make_repo(self, version="1.2.3"):
        self.write("Cargo.toml", f'[workspace.package]\nversion = "{version}"\n')
        self.write("Cargo.lock", "\n".join(
            f'[[package]]\nname = "{name}"\nversion = "{version}"'
            for name in ("funrounds-core", "funrounds-plugin")
        ))
        self.write("crates/funrounds-core/src/lib.rs", f'pub const VERSION: &str = "{version}";\n')
        main = {"version": version, "dependencies": [{"name": "s2sdk", "constraints": "=3.0.6"},
                {"name": "funrounds_native", "constraints": f"={version}"}]}
        native = {"version": version, "dependencies": [{"name": "s2sdk", "constraints": "=3.0.6"}]}
        self.write("packaging/funrounds.pplugin", json.dumps(main))
        self.write("native/funrounds-native/funrounds-native.pplugin", json.dumps(native))
        self.write("bindings/s2sdk.pplugin", '{"version":"3.0.6","dependencies":[{"name":"configs","constraints":">=2.1.0"}]}')
        self.write("native/funrounds-native/gamedata/funrounds-native.jsonc", '{}')
        self.write("config/funrounds.example.toml", '# Generic example\n')
        for language in ("zh-CN", "en"):
            self.write(f"config/i18n/{language}.toml", '[messages]\n"mode.example.name" = "Example"\n')
        self.write("LICENSE", "License")
        self.write("licenses/dependency.txt", "Dependency notice")

    def assets(self, version="1.2.3"):
        output = self.root / "dist"
        output.mkdir(exist_ok=True)
        for platform in ci.PLATFORMS:
            (output / f"funrounds-{version}-{platform}.zip").write_bytes(b"archive")
        return output

    def sdk_fixture(self, platform, version="3.0.6"):
        name = "plugify-plugin-s2sdk.dll" if platform == "windows-x64" else "libplugify-plugin-s2sdk.so"
        self.write(f"native-output/s2sdk/bin/{name}", "patched SDK")
        self.write("native-output/s2sdk/plugify-plugin-s2sdk.pplugin", json.dumps({"name": "s2sdk", "version": version}))
        for name in ("gamedata.jsonc", "settings.jsonc", "translations.jsonc", "source.json"):
            self.write(f"native-output/s2sdk/{name}", "{}")
        self.write("native-output/s2sdk/licenses/S2SDK-GPL-3.0.txt", "SDK notice")

    def env(self):
        event = {"repository": {"default_branch": "main"}, "before": "a" * 40}
        path = self.write("event.json", json.dumps(event))
        return {"GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/heads/main", "GITHUB_SHA": "b" * 40,
                "GITHUB_REPOSITORY": "example/project", "GITHUB_EVENT_PATH": str(path)}

    def test_version_and_prerelease_metadata(self):
        self.assertFalse(ci.metadata(self.root)["prerelease"])
        self.make_repo("1.2.3-alpha.test")
        self.assertTrue(ci.metadata(self.root)["prerelease"])
        self.write("crates/funrounds-core/src/lib.rs", 'pub const VERSION: &str = "1.2.3-Alpha Test";')
        self.assertEqual(ci.metadata(self.root)["tag"], "v1.2.3-alpha.test")

    def test_metadata_rejects_stale_versions(self):
        for path in ("Cargo.lock", "packaging/funrounds.pplugin", "native/funrounds-native/funrounds-native.pplugin",
                     "crates/funrounds-core/src/lib.rs"):
            with self.subTest(path=path):
                self.make_repo()
                target = self.root / path
                target.write_text(target.read_text("utf-8").replace("1.2.3", "1.2.2"), "utf-8")
                with self.assertRaises(ValueError):
                    ci.metadata(self.root)

    def test_invalid_semver_is_rejected(self):
        for version in ("0.2-Alpha Test", "01.2.3", "1.2.3-alpha.01", "1.2.3\nother=value"):
            with self.subTest(version=version):
                self.write("Cargo.toml", '[workspace.package]\nversion = ' + json.dumps(version))
                with self.assertRaises(ValueError):
                    ci.metadata(self.root)

    def test_release_only_on_default_branch_push(self):
        env = self.env()
        event = json.loads(Path(env["GITHUB_EVENT_PATH"]).read_text("utf-8"))
        self.assertTrue(ci.release_push(event, env))
        for changed in ({"GITHUB_REF": "refs/heads/feature"}, {"GITHUB_EVENT_NAME": "pull_request"}):
            self.assertFalse(ci.release_push(event, dict(env, **changed)))
        self.assertFalse(ci.release_push(dict(event, deleted=True), env))

    @patch.object(ci.subprocess, "run")
    @patch.object(ci.subprocess, "check_output")
    def test_version_change_uses_push_base_not_head_parent(self, output, run):
        before = "a" * 40
        output.side_effect = ["Cargo.toml\n", '[workspace.package]\nversion="1.2.2"']
        self.assertTrue(ci.version_changed({"before": before}, "1.2.3", self.root))
        self.assertIn(f"{before}:Cargo.toml", output.call_args.args[0])
        output.side_effect = ["Cargo.toml\n", '[workspace.package]\nversion="1.2.3"']
        self.assertFalse(ci.version_changed({"before": before}, "1.2.3", self.root))
        self.assertTrue(ci.version_changed({"before": "0" * 40}, "1.2.3", self.root))
        output.side_effect = [""]
        self.assertTrue(ci.version_changed({"before": before}, "1.2.3", self.root))

    def test_package_contains_both_plugins_and_no_private_or_live_config(self):
        for platform, (rust_name, rust_entry, native_name) in ci.PLATFORMS.items():
            with self.subTest(platform=platform):
                self.write(f"target/release/{rust_name}", "binary")
                native = self.root / "native-output"
                self.sdk_fixture(platform)
                self.write(f"native-output/bin/{native_name}", "native binary")
                for path in ("funrounds-native.pplugin", "gamedata/funrounds-native.jsonc"):
                    self.write(f"native-output/{path}", (self.root / "native/funrounds-native" / path).read_text("utf-8"))
                self.write("config/fr_players.toml", "private preferences")
                self.write("docs/private.txt", "not an artifact")
                archive = ci.package(platform, native, self.root / "dist", self.root)
                with zipfile.ZipFile(archive) as bundle:
                    names = bundle.namelist()
                    self.assertIn(f"addons/plugify/envs/funrounds/bin/{rust_entry}", names)
                    self.assertIn(f"addons/plugify/envs/funrounds_native/bin/{native_name}", names)
                    self.assertIn("licenses/dependency.txt", names)
                    self.assertIn("licenses/s2sdk/S2SDK-GPL-3.0.txt", names)
                    self.assertIn("addons/plugify/envs/s2sdk/plugify-plugin-s2sdk.pplugin", names)
                    self.assertIn("addons/plugify/envs/s2sdk/source.json", names)
                    self.assertIn("addons/plugify/envs/s2sdk/settings.jsonc", names)
                    self.assertIn("addons/plugify/envs/s2sdk/gamedata.jsonc", names)
                    self.assertIn("addons/plugify/envs/s2sdk/translations.jsonc", names)
                    self.assertTrue(any(name.startswith("addons/plugify/envs/s2sdk/bin/") for name in names))
                    self.assertIn("addons/plugify/configs/funrounds.example.toml", names)
                    for language in ("zh-CN", "en"):
                        self.assertIn(f"addons/plugify/configs/funrounds/i18n/{language}.toml", names)
                    self.assertFalse(any("fr_players" in name or "private" in name or name.endswith("/funrounds.toml") for name in names))

    def test_missing_binary_fails_before_creating_archive(self):
        with self.assertRaisesRegex(ValueError, "Missing or empty"):
            ci.package("windows-x64", self.root / "native-output", self.root / "dist", self.root)
        self.assertFalse((self.root / "dist").exists())

    def test_package_rejects_wrong_sdk_version_and_missing_resources(self):
        self.write("target/release/funrounds_plugin.dll", "binary")
        self.write("native-output/bin/funrounds-native.dll", "native binary")
        for path in ("funrounds-native.pplugin", "gamedata/funrounds-native.jsonc"):
            self.write(f"native-output/{path}", (self.root / "native/funrounds-native" / path).read_text("utf-8"))
        self.sdk_fixture("windows-x64", version="3.0.0")
        with self.assertRaisesRegex(ValueError, "Bundled S2SDK"):
            ci.package("windows-x64", self.root / "native-output", self.root / "dist", self.root)
        self.sdk_fixture("windows-x64")
        (self.root / "native-output/s2sdk/gamedata.jsonc").unlink()
        with self.assertRaisesRegex(ValueError, "Missing or empty"):
            ci.package("windows-x64", self.root / "native-output", self.root / "dist", self.root)
        self.assertFalse((self.root / "dist").exists())

    @patch.object(ci.subprocess, "check_output")
    def test_linux_abi_comparison_is_not_decimal(self, output):
        output.side_effect = ["GLIBC_2.2.5 GLIBC_2.7 GLIBC_2.31", "NEEDED libc.so.6"]
        ci.check_linux_abi([Path("sample.so")])
        output.side_effect = ["GLIBC_2.32"]
        with self.assertRaises(ValueError):
            ci.check_linux_abi([Path("sample.so")])

    @patch.object(ci.subprocess, "check_output")
    def test_linux_binaries_do_not_depend_on_build_machine_paths(self, output):
        for dynamic in ("(RUNPATH) Library runpath: [$ORIGIN:/workspace/sdk/lib]",
                        "(NEEDED) Shared library: [/workspace/sdk/libv8.so]"):
            output.side_effect = ["GLIBC_2.29", dynamic]
            with self.assertRaisesRegex(ValueError, "Build-machine"):
                ci.check_linux_abi([Path("sample.so")])
        output.side_effect = ["GLIBC_2.29", "(RUNPATH) Library runpath: [$ORIGIN]"]
        ci.check_linux_abi([Path("sample.so")])
        output.side_effect = ["GLIBC_2.31", "Shared library: [libstdc++.so.6]"]
        with self.assertRaises(ValueError):
            ci.check_linux_abi([Path("sample.so")])

    @patch.object(ci, "version_changed", return_value=True)
    @patch.object(ci, "gh_json")
    @patch.object(ci.subprocess, "run")
    def test_release_stages_draft_then_publishes_exact_sha(self, run, api, changed):
        api.side_effect = [None, None]
        ci.publish(self.assets(), self.env(), self.root)
        create, publish = [call.args[0] for call in run.call_args_list]
        self.assertIn("--draft", create)
        self.assertIn("b" * 40, create)
        self.assertIn("Includes patched S2SDK", create[create.index("--notes") + 1])
        self.assertIn("configs >=2.1.0", create[create.index("--notes") + 1])
        self.assertIn("--draft=false", publish)
        self.assertIn("--prerelease=false", publish)

    @patch.object(ci, "version_changed", return_value=True)
    @patch.object(ci, "gh_json")
    @patch.object(ci.subprocess, "run")
    def test_prerelease_and_draft_retry(self, run, api, changed):
        self.make_repo("1.2.3-alpha.test")
        api.side_effect = [None, {"draft": True, "target_commitish": "b" * 40}]
        ci.publish(self.assets("1.2.3-alpha.test"), self.env(), self.root)
        self.assertIn("--clobber", run.call_args_list[0].args[0])
        self.assertIn("--prerelease=true", run.call_args_list[-1].args[0])
        self.assertIn("--latest=false", run.call_args_list[-1].args[0])

    @patch.object(ci, "version_changed", return_value=True)
    @patch.object(ci, "gh_json")
    @patch.object(ci.subprocess, "run")
    def test_existing_tag_cannot_be_moved(self, run, api, changed):
        api.return_value = {"object": {"type": "commit", "sha": "c" * 40}}
        with self.assertRaisesRegex(ValueError, "different commit"):
            ci.publish(self.assets(), self.env(), self.root)
        run.assert_not_called()

    @patch.object(ci, "version_changed", return_value=False)
    @patch.object(ci, "gh_json")
    def test_unchanged_version_never_accesses_release_api(self, api, changed):
        with self.assertRaisesRegex(ValueError, "version-changing"):
            ci.publish(self.assets(), self.env(), self.root)
        api.assert_not_called()

    @patch.object(ci, "version_changed", return_value=True)
    @patch.object(ci, "gh_json")
    def test_missing_platform_archive_never_accesses_release_api(self, api, changed):
        assets = self.assets()
        next(assets.iterdir()).unlink()
        with self.assertRaisesRegex(ValueError, "Both platform"):
            ci.publish(assets, self.env(), self.root)
        api.assert_not_called()

    @patch.object(ci, "version_changed", return_value=True)
    @patch.object(ci, "gh_json")
    @patch.object(ci.subprocess, "run")
    def test_published_release_rerun_is_read_only(self, run, api, changed):
        assets = self.assets()
        api.side_effect = [{"object": {"type": "commit", "sha": "b" * 40}},
                           {"draft": False, "prerelease": False, "assets": [{"name": path.name, "size": 10} for path in assets.iterdir()]}]
        ci.publish(assets, self.env(), self.root)
        run.assert_not_called()

    @patch.object(ci.subprocess, "run")
    def test_api_error_is_not_treated_as_missing_release(self, run):
        run.return_value = subprocess.CompletedProcess([], 1, "", "authentication failed (HTTP 401)")
        with self.assertRaises(RuntimeError):
            ci.gh_json("example", missing=True)
        run.return_value = subprocess.CompletedProcess([], 1, "", "Not Found (HTTP 404)")
        self.assertIsNone(ci.gh_json("example", missing=True))


if __name__ == "__main__":
    unittest.main()
