# RightClick Utilities CLI

Windows CLI and processing core for RightClick Utilities. Rust 2021; build and test with `cargo build --release` and `cargo test` in this directory. The executable is `target/release/UtilitiesCli.exe`.

```powershell
.\UtilitiesCli.exe compress 'C:\Videos\clip.mp4' --max-mb 15 --json
.\UtilitiesCli.exe compress 'C:\Videos\clip.mp4' --max-mb 20 --output 'C:\Videos\shared.mp4' --json
.\UtilitiesCli.exe strip-audio 'C:\Videos\clip.mp4' --json
.\UtilitiesCli.exe functions --json
.\UtilitiesCli.exe run video-20mb 'C:\Videos\clip.mp4' --json
.\UtilitiesCli.exe run convert-iphone 'C:\Videos\clip.mp4' --json
```

FFmpeg and ffprobe are resolved beside the executable first, then on PATH. Use an FFmpeg build with libx264 and AAC. Video content is inspected independently of its extension; images, audio-only media, attached cover art, and single-frame files are rejected. Compression produces full-duration H.264/AAC MP4 using two-pass encoding. Size limits are decimal MB, and output must be strictly smaller than the requested limit. A budget that cannot preserve the full duration fails without publishing a result. Compression keeps the first non-attached video stream and first audio stream; subtitles, extra tracks, metadata and chapters are omitted. Audio removal copies all non-audio streams into the chosen output container, and fails if that container cannot represent them.

Default outputs are `clip-15MB.mp4`, `clip-20MB.mp4`, and `clip-no-audio.mp4`. Collisions add ` (1)`, ` (2)`, etc. Explicit outputs fail if they exist unless `--overwrite` is provided. The original input is always protected. Relative paths resolve from the caller's current working directory. Encoding happens in a unique temporary directory beside the output and is published only after validation. Ctrl+C stops children and cleans the temporary directory. Forced termination kills child processes through a Windows job object; it can leave an `.rightclick-utilities-*` temporary directory, but never publishes an incomplete output.

`--json` produces exactly one final stdout object. Successful video operations return `success`, `operation`, `output`, `bytes`, and `duration`; compression also returns `maxMb`. Errors return `{"success":false,"error":"..."}` with a nonzero exit code. Custom functions return `success`, `function`, and `input` without assuming they produced an output file. Stderr contains readable stage logs and `progress=<integer>` lines from 0 through 100; only successful publication emits 100. Custom process stdout and stderr are relayed to CLI stderr. All child invocations use argument arrays with no shell and no stdin. Video children do not open console windows.

`functions` and `run` use `%USERPROFILE%\.rightclick-utilities\functions.json`, or an explicit `--config <path>`. This user-profile location is shared with Explorer and avoids AppData write virtualization in packaged desktop hosts. A missing file is initialized atomically with `video-15mb`, `video-20mb`, `convert-iphone`, and `strip-audio`. The iPhone function uses the same 20 MB H.264/AAC MP4 compression pipeline. Configuration format:

```json
{
  "version": 1,
  "functions": [
    {
      "id": "video-20mb",
      "label": "Compress below 20 MB",
      "enabled": true,
      "appliesTo": "file",
      "extensions": [".mp4", ".mov", ".mkv"],
      "executable": "{cli}",
      "args": ["compress", "{input}", "--max-mb", "20", "--json"]
    }
  ]
}
```

IDs contain only ASCII letters, digits, hyphens, and underscores and must be unique. `appliesTo` is `file`, `folder`, or `any`; an empty extensions array means all extensions. Extensions must be lowercase with a leading dot. Each configured argument is one exact process argument; supported placeholders are `{input}`, `{parent}`, `{stem}`, `{cli}`, and `{tools}`. `{tools}` is the CLI executable's directory. `{cli}` functions may invoke only `compress` or `strip-audio`; external executable functions run one process and propagate its exit status. Custom commands execute with the current user's permissions.
