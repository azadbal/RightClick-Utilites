"""Real FFmpeg integration checks. Run after building release/UtilitiesCli.exe."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "tests" / "output"
CLI = ROOT / "release" / "UtilitiesCli.exe"
FFMPEG = ROOT / "release" / "ffmpeg.exe"
FFPROBE = ROOT / "release" / "ffprobe.exe"


def process(args, **kwargs):
    return subprocess.run([str(a) for a in args], capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=180, **kwargs)


def probe(path):
    result = process([FFPROBE, "-v", "error", "-show_streams", "-show_format", "-of", "json", path])
    if result.returncode:
        raise AssertionError(result.stderr)
    return json.loads(result.stdout)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class VideoFunctions(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        OUTPUT.mkdir(parents=True, exist_ok=True)
        cls.work = OUTPUT / ("test " + str(os.getpid()) + " ünicode")
        cls.work.mkdir()
        cls.video = cls.work / "source ü noise.mkv"
        result = process([FFMPEG, "-v", "error", "-f", "lavfi", "-i",
                          "nullsrc=s=640x360:r=24,geq=lum='random(1)*255':cb=128:cr=128",
                          "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000",
                          "-t", "6", "-c:v", "ffv1", "-c:a", "pcm_s16le", cls.video])
        if result.returncode:
            raise AssertionError(result.stderr)
        cls.original_hash = digest(cls.video)
        cls.source_info = probe(cls.video)
        cls.audio = cls.work / "audio only.wav"
        result = process([FFMPEG, "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440", "-t", "1", cls.audio])
        if result.returncode:
            raise AssertionError(result.stderr)
        cls.image = cls.work / "still.png"
        result = process([FFMPEG, "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=64x64", "-frames:v", "1", cls.image])
        if result.returncode:
            raise AssertionError(result.stderr)
        cls.config = cls.work / "functions.json"
        result = process([CLI, "functions", "--config", cls.config, "--json"])
        if result.returncode:
            raise AssertionError(result.stderr + result.stdout)

    def cli(self, *args, success=True):
        result = process([CLI, *args, "--json"])
        payload = json.loads(result.stdout)
        self.assertEqual(result.returncode == 0, success, result.stderr + result.stdout)
        self.assertEqual(payload["success"], success, result.stdout)
        return payload

    def test_compression_caps_and_full_duration(self):
        for mb in (15, 20):
            with self.subTest(mb=mb):
                result = self.cli("compress", self.video, "--max-mb", str(mb))
                output = Path(result["output"])
                self.assertEqual(output.parent, self.work)
                self.assertLessEqual(output.stat().st_size, mb * 1_000_000)
                info = probe(output)
                self.assertAlmostEqual(float(info["format"]["duration"]), float(self.source_info["format"]["duration"]), delta=0.15)
                self.assertEqual(next(s for s in info["streams"] if s["codec_type"] == "video")["codec_name"], "h264")
                self.assertTrue(any(s["codec_type"] == "audio" for s in info["streams"]))
        self.assertEqual(digest(self.video), self.original_hash)

    def test_strip_audio_stream_copy_and_unique_output(self):
        outputs = [Path(self.cli("strip-audio", self.video)["output"]) for _ in range(2)]
        self.assertNotEqual(*outputs)
        source_video = next(s for s in self.source_info["streams"] if s["codec_type"] == "video")
        for output in outputs:
            info = probe(output)
            self.assertFalse(any(s["codec_type"] == "audio" for s in info["streams"]))
            video = next(s for s in info["streams"] if s["codec_type"] == "video")
            self.assertEqual(video["codec_name"], source_video["codec_name"])
            self.assertAlmostEqual(float(info["format"]["duration"]), float(self.source_info["format"]["duration"]), delta=0.15)
        self.assertEqual(digest(self.video), self.original_hash)

    def test_reject_non_video_and_misleading_extensions(self):
        text = self.work / "fake.mp4"
        text.write_text("not a video", encoding="utf-8")
        audio_mp4 = self.work / "renamed audio.mp4"
        shutil.copyfile(self.audio, audio_mp4)
        image_mp4 = self.work / "renamed image.mp4"
        shutil.copyfile(self.image, image_mp4)
        for source in (self.audio, self.image, text, audio_mp4, image_mp4, self.work):
            for command in ("strip-audio", "compress"):
                with self.subTest(source=source.name, command=command):
                    args = [command, source]
                    if command == "compress":
                        args += ["--max-mb", "15"]
                    destination = self.work / (source.name + ".rejected.mp4")
                    args += ["--output", destination]
                    result = self.cli(*args, success=False)
                    self.assertTrue(result["error"])
                    self.assertFalse(destination.exists())

    def test_existing_output_input_protection_and_failure(self):
        destination = self.work / "existing.mkv"
        destination.write_bytes(b"existing output")
        self.cli("strip-audio", self.video, "--output", destination, success=False)
        self.assertEqual(destination.read_bytes(), b"existing output")
        self.cli("strip-audio", self.video, "--output", self.video, "--overwrite", success=False)
        self.assertEqual(digest(self.video), self.original_hash)
        self.cli("strip-audio", self.work / "missing.mp4", success=False)

    def test_config_runner_and_applicability(self):
        result = self.cli("run", "strip-audio", self.video, "--config", self.config)
        self.assertFalse(any(s["codec_type"] == "audio" for s in probe(result["output"])["streams"]))
        self.cli("run", "strip-audio", self.audio, "--config", self.config, success=False)
        self.cli("run", "unknown-function", self.video, "--config", self.config, success=False)

    def test_already_silent_video(self):
        silent = Path(self.cli("strip-audio", self.video)["output"])
        result = self.cli("strip-audio", silent)
        self.assertFalse(any(s["codec_type"] == "audio" for s in probe(result["output"])["streams"]))
        result = self.cli("compress", silent, "--max-mb", "15")
        self.assertFalse(any(s["codec_type"] == "audio" for s in probe(result["output"])["streams"]))

    def test_custom_folder_command_arguments_and_error(self):
        folder = self.work / "folder & ü space"
        folder.mkdir()
        script = self.work / "check args.py"
        script.write_text("import json, pathlib, sys\npathlib.Path(sys.argv[1], 'received.json').write_text(json.dumps(sys.argv[1:]), encoding='utf-8')\n", encoding="utf-8")
        config = self.work / "custom.json"
        item = {"id": "custom-folder", "label": "Custom folder", "enabled": True,
                "appliesTo": "folder", "extensions": [], "executable": sys.executable,
                "args": [str(script), "{input}", "literal & text", "{stem}", "{parent}"]}
        config.write_text(json.dumps({"version": 1, "functions": [item]}), encoding="utf-8")
        self.cli("run", "custom-folder", folder, "--config", config)
        received = json.loads((folder / "received.json").read_text(encoding="utf-8"))
        self.assertEqual(Path(received[0]).resolve(), folder.resolve())
        self.assertEqual(received[1], "literal & text")
        self.cli("run", "custom-folder", self.video, "--config", config, success=False)
        item["args"] = ["-c", "import sys; sys.exit(7)"]
        config.write_text(json.dumps({"version": 1, "functions": [item]}), encoding="utf-8")
        result = self.cli("run", "custom-folder", folder, "--config", config, success=False)
        self.assertIn("7", result["error"])

    def test_cli_unattended_contract(self):
        self.assertEqual(process([CLI, "--help"]).returncode, 0)
        self.assertEqual(process([CLI, "--version"]).returncode, 0)
        self.cli("compress", self.video, "--max-mb", "0", success=False)


if __name__ == "__main__":
    unittest.main(verbosity=2)
