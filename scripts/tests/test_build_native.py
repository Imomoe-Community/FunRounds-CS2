import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "build_native", Path(__file__).resolve().parents[1] / "build-native.py"
)
build_native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(build_native)
ROOT = Path(__file__).resolve().parents[2]


class BuildNativeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=Path(__file__).parent)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def create_sdk_layout(self, work):
        sdk = work / "s2sdk"
        include = sdk / "external" / "plugify" / "include" / "polyhook"
        include.mkdir(parents=True)
        (include / "polyhook.cpp").write_text("// fixture\n", encoding="utf-8")
        return sdk

    def create_imports(self, generated_include):
        imports = generated_include / "external" / "plugify" / "include" / "s2sdk"
        imports.mkdir(parents=True, exist_ok=True)
        (imports / "s2sdk.cpp").write_text("// fixture\n", encoding="utf-8")
        (imports / "s2sdk.hpp").write_text("// fixture\n", encoding="utf-8")

    def run_mock_build(self, system, binary_name):
        work = self.root / "work"
        output = self.root / "output"
        work.mkdir()
        output.mkdir()
        commands = []

        def fake_checkout(sdk, _url, _commit):
            self.create_sdk_layout(work)

        def fake_generate(_sdk, generated_output):
            self.create_imports(generated_output)
            return generated_output / "external" / "plugify" / "include"

        def fake_run(command):
            commands.append(command)
            if "--build" in command:
                binary_dir = work / "build" / "bin"
                if system == "Windows":
                    binary_dir /= "Release"
                binary_dir.mkdir(parents=True, exist_ok=True)
                (binary_dir / binary_name).write_bytes(b"native fixture")

        with patch.object(build_native.platform, "system", return_value=system), \
                patch.object(build_native, "checkout_commit", side_effect=fake_checkout), \
                patch.object(build_native, "ensure_sdk_sources"), \
                patch.object(build_native, "generate_s2sdk_imports", side_effect=fake_generate), \
                patch.object(build_native, "require_tool", return_value="cmake"), \
                patch.object(build_native, "check_linux_compilers", return_value=("/usr/bin/gcc-14", "/usr/bin/g++-14")), \
                patch.object(build_native, "windows_generator", return_value="Visual Studio 18 2026"), \
                patch.object(build_native, "build_s2sdk") as provider_build, \
                patch.object(build_native, "run", side_effect=fake_run):
            result = build_native.configure_and_build(ROOT, work, output)

        self.assertEqual(result, output)
        self.assertEqual(provider_build.call_args.args[:3], (work / "s2sdk", work, output))
        self.assertEqual((output / "bin" / binary_name).read_bytes(), b"native fixture")
        manifest = ROOT / "native/funrounds-native/funrounds-native.pplugin"
        gamedata = ROOT / "native/funrounds-native/gamedata/funrounds-native.jsonc"
        self.assertEqual((output / manifest.name).read_bytes(), manifest.read_bytes())
        self.assertEqual((output / "gamedata" / gamedata.name).read_bytes(), gamedata.read_bytes())
        return commands

    def test_generates_pinned_s2sdk_import_manifest(self):
        sdk = self.root / "sdk"
        sdk.mkdir()
        (sdk / "version.txt").write_text("3.0.2\n", encoding="utf-8")
        (sdk / "plugify-plugin-s2sdk.pplugin.in").write_text(
            '{"version":"${S2SDK_VERSION}","name":"${S2SDK_PACKAGE}"}', encoding="utf-8"
        )
        generated_include = self.root / "generated" / "include"
        commands = []

        def fake_run(command):
            commands.append(command)
            self.create_imports(generated_include)

        with patch.object(build_native, "require_tool", return_value="plugify-gen"), \
                patch.object(build_native, "run", side_effect=fake_run):
            build_native.generate_s2sdk_imports(sdk, generated_include)

        rendered = json.loads((self.root / "generated/plugify-plugin-s2sdk.pplugin").read_text("utf-8"))
        self.assertEqual(rendered, {"version": "3.0.2", "name": "s2sdk"})
        self.assertEqual(commands[0][1:7], ["-manifest", str(self.root / "generated/plugify-plugin-s2sdk.pplugin"), "-output", str(generated_include), "-lang", "cpp"])
        self.assertIn("-overwrite", commands[0])
        self.assertTrue((generated_include / "external/plugify/include/s2sdk/s2sdk.cpp").is_file())
        self.assertTrue((generated_include / "external/plugify/include/s2sdk/s2sdk.hpp").is_file())

    def test_linux_build_uses_gcc14_abi0_and_static_cxx_runtime(self):
        commands = self.run_mock_build("Linux", "libfunrounds-native.so")
        configure = commands[0]
        self.assertIn("-DCMAKE_BUILD_TYPE=Release", configure)
        self.assertIn("-DCMAKE_C_COMPILER=/usr/bin/gcc-14", configure)
        self.assertIn("-DCMAKE_CXX_COMPILER=/usr/bin/g++-14", configure)
        self.assertIn("-DSOURCESDK_USE_ABI0=ON", configure)
        self.assertIn("-DCMAKE_SHARED_LINKER_FLAGS=-static-libstdc++ -static-libgcc", configure)
        self.assertIn("-DCMAKE_BUILD_WITH_INSTALL_RPATH=ON", configure)
        self.assertIn("-DCMAKE_INSTALL_RPATH=$ORIGIN", configure)
        self.assertIn("-DSOURCESDK_LINK_ENABLE_RPATH=OFF", configure)
        self.assertIn(
            "-DFUNROUNDS_S2SDK_GENERATED_INCLUDE_DIR="
            + str(self.root / "work/generated-s2sdk/include/external/plugify/include"),
            configure,
        )

    def test_linux_compiler_check_rejects_gcc_13(self):
        with patch.dict(build_native.os.environ, {"CC": "gcc", "CXX": "g++"}), \
                patch.object(build_native.shutil, "which", side_effect=["/usr/bin/gcc", "/usr/bin/g++"]), \
                patch.object(build_native, "capture", side_effect=["13.2.0", "14.2.0"]):
            with self.assertRaisesRegex(build_native.BuildError, "GCC 14 or newer"):
                build_native.check_linux_compilers()

    def test_windows_build_selects_explicit_vs_x64_generator_and_stages_dll(self):
        commands = self.run_mock_build("Windows", "funrounds-native.dll")
        configure = commands[0]
        self.assertEqual(configure[configure.index("-G") + 1], "Visual Studio 18 2026")
        self.assertEqual(configure[configure.index("-A") + 1], "x64")
        self.assertFalse(any("CMAKE_SHARED_LINKER_FLAGS" in item for item in configure))

    def test_windows_generator_tracks_installed_visual_studio(self):
        program_files = self.root / "Program Files (x86)"
        vswhere = program_files / "Microsoft Visual Studio/Installer/vswhere.exe"
        vswhere.parent.mkdir(parents=True)
        vswhere.write_text("", encoding="utf-8")
        with patch.object(build_native.shutil, "which", return_value=None), \
                patch.dict(build_native.os.environ, {"ProgramFiles(x86)": str(program_files)}), \
                patch.object(build_native, "capture", return_value="17.14.37806.7") as capture:
            self.assertEqual(build_native.windows_generator(), "Visual Studio 17 2022")
        self.assertEqual(capture.call_args.args[0][1:], ["-latest", "-products", "*", "-property", "installationVersion"])

    def test_builds_and_stages_patched_s2sdk_on_both_platforms(self):
        for system, binary in (("Linux", "libplugify-plugin-s2sdk.so"), ("Windows", "plugify-plugin-s2sdk.dll")):
            with self.subTest(system=system):
                work = self.root / system
                sdk = work / "s2sdk"
                for path in ("LICENSE", "external/parallel-hashmap/LICENSE", "external/zydis/LICENSE",
                             "external/zydis/dependencies/zycore/LICENSE", "external/sourcesdk/thirdparty/protobuf/LICENSE"):
                    target = sdk / path
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text("notice", "utf-8")
                build = work / "build-s2sdk"
                binary_dir = build / "bin" / "Release" if system == "Windows" else build / "bin"
                binary_dir.mkdir(parents=True)
                (binary_dir / binary).write_bytes(b"patched provider")
                for name in ("plugify-plugin-s2sdk.pplugin", "gamedata.jsonc", "settings.jsonc", "translations.jsonc"):
                    (build / name).write_text("{}", "utf-8")
                output = work / "output"
                with patch.object(build_native.platform, "system", return_value=system), patch.object(build_native, "run") as run:
                    build_native.build_s2sdk(sdk, work, output, "cmake", ["-DCMAKE_BUILD_TYPE=Release"])
                commands = [call.args[0] for call in run.call_args_list]
                self.assertIn("external/zydis", commands[0])
                self.assertEqual("-Dprotobuf_WITH_ZLIB=OFF" in commands[1], system == "Windows")
                self.assertIn("plugify-plugin-s2sdk", commands[2])
                self.assertEqual((output / "s2sdk/bin" / binary).read_bytes(), b"patched provider")
                self.assertEqual(len(list((output / "s2sdk/licenses").glob("*.txt"))), 5)
                source = json.loads((output / "s2sdk/source.json").read_text("utf-8"))
                self.assertEqual(source["repository"], "https://github.com/Mai-Ooizumi/plugify-plugin-s2sdk.git")
                self.assertEqual(source["commit"], "021b5d5a90abb6014df0ca128a4af4d3725888d0")


if __name__ == "__main__":
    unittest.main()
