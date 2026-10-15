use crate::process;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Video {
    duration: f64,
    index: u64,
    audio: bool,
    frame_interval: f64,
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .filter(|n| n.is_finite())
}

fn video_info(data: &Value) -> Result<Video> {
    let format = data["format"]["format_name"].as_str().unwrap_or("");
    if format.is_empty()
        || format.split(',').any(|f| {
            matches!(
                f,
                "image2" | "image2pipe" | "gif" | "apng" | "ico" | "jpegxl_anim"
            ) || f.ends_with("_pipe")
        })
    {
        bail!("Input is an image, not a video");
    }
    let streams = data["streams"]
        .as_array()
        .context("Input has no media streams")?;
    let stream = streams
        .iter()
        .find(|s| {
            s["codec_type"] == "video"
                && s["disposition"]["attached_pic"].as_u64().unwrap_or(0) == 0
        })
        .context("Input has no video stream (audio-only files and cover art are unsupported)")?;
    let duration = number(&stream["duration"])
        .or_else(|| number(&data["format"]["duration"]))
        .filter(|v| *v > 0.0)
        .context("Video duration is unavailable or zero")?;
    let rate = stream["avg_frame_rate"]
        .as_str()
        .and_then(|s| s.split_once('/'))
        .and_then(|(n, d)| Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok()?))
        .filter(|r| r.is_finite() && *r > 0.0);
    let frames = number(&stream["nb_frames"]);
    if frames.is_some_and(|n| n <= 1.0)
        || (frames.is_none() && rate.is_some_and(|r| duration * r <= 1.05))
    {
        bail!("Input contains a single still frame, not a video");
    }
    if frames.is_none() && rate.is_none() {
        bail!("Cannot establish that the input contains moving video");
    }
    Ok(Video {
        duration,
        index: stream["index"]
            .as_u64()
            .context("Video stream index is missing")?,
        audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        frame_interval: rate.map(|r| 1.0 / r).unwrap_or(0.04),
    })
}

fn probe(ffprobe: &Path, input: &Path) -> Result<Video> {
    let args = vec![
        "-v".into(),
        "error".into(),
        "-show_streams".into(),
        "-show_format".into(),
        "-of".into(),
        "json".into(),
        input.to_string_lossy().into_owned(),
    ];
    let data = process::run(ffprobe, &args, None, (0, 0), false)
        .context("Cannot inspect video; input may be unsupported or damaged")?;
    video_info(&serde_json::from_slice(&data).context("Invalid ffprobe response")?)
}

fn input_path(input: &Path) -> Result<PathBuf> {
    if !input.is_file() {
        bail!("Input must be an existing video file");
    }
    input.canonicalize().context("Cannot access input file")
}

fn output_path(
    input: &Path,
    output: Option<&Path>,
    suffix: &str,
    extension: &str,
    overwrite: bool,
) -> Result<(PathBuf, bool)> {
    let explicit = output.is_some();
    let path = output.map(Path::to_path_buf).unwrap_or_else(|| {
        input.with_file_name(format!(
            "{}-{suffix}.{extension}",
            input.file_stem().unwrap_or_default().to_string_lossy()
        ))
    });
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .context("Output directory must already exist")?;
    let path = parent.join(path.file_name().context("Output must name a file")?);
    if path == input || (path.is_file() && same_file(&path, input)?) {
        bail!("Output cannot replace the original input");
    }
    if explicit && path.exists() && !overwrite {
        bail!("Output exists; select a different path or pass --overwrite");
    }
    if path.is_dir() {
        bail!("Output path is a directory");
    }
    Ok((path, explicit))
}

fn numbered(path: &Path, number: u32) -> PathBuf {
    if number == 0 {
        return path.to_path_buf();
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    path.with_file_name(format!("{stem} ({number}){extension}"))
}

fn same_file(left: &Path, right: &Path) -> Result<bool> {
    #[cfg(windows)]
    {
        use std::{mem::zeroed, os::windows::io::AsRawHandle};
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let identity = |path: &Path| -> Result<(u32, u32, u32)> {
            let file = std::fs::File::open(path)?;
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok((
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            ))
        };
        Ok(identity(left)? == identity(right)?)
    }
    #[cfg(not(windows))]
    {
        Ok(left.canonicalize()? == right.canonicalize()?)
    }
}

pub(crate) fn publish(
    temp: &Path,
    destination: &Path,
    explicit: bool,
    overwrite: bool,
) -> Result<PathBuf> {
    process::check_cancelled()?;
    if explicit && overwrite {
        #[cfg(windows)]
        move_file(temp, destination, true)?;
        #[cfg(not(windows))]
        std::fs::rename(temp, destination)?;
        return Ok(destination.to_path_buf());
    }
    for n in 0..10_000 {
        let path = numbered(destination, n);
        #[cfg(windows)]
        let result = move_file(temp, &path, false);
        #[cfg(not(windows))]
        let result = std::fs::hard_link(temp, &path);
        match result {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && !explicit => {
                continue
            }
            Err(error) => {
                return Err(error)
                    .context("Cannot publish output without overwriting an existing file")
            }
        }
    }
    bail!("Too many output filename collisions")
}

#[cfg(windows)]
fn move_file(source: &Path, target: &Path, overwrite: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let flags = MOVEFILE_WRITE_THROUGH
        | if overwrite {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), flags) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn check_duration(input: &Video, output: &Video) -> Result<()> {
    let tolerance = (input.frame_interval * 2.0).max(0.5);
    if (input.duration - output.duration).abs() > tolerance {
        bail!(
            "Output duration differs from input ({:.3}s versus {:.3}s); output was not published",
            input.duration,
            output.duration
        );
    }
    Ok(())
}

fn base_args(input: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ]
}

fn budget(max_mb: f64, duration: f64, audio: bool) -> Result<(u64, u64, u64)> {
    if !max_mb.is_finite() || max_mb <= 0.0 || max_mb > 1_000_000.0 {
        bail!("--max-mb must be a finite positive number no greater than 1000000");
    }
    let cap = (max_mb * 1_000_000.0).floor() as u64;
    let total = ((cap as f64 * 0.96 - 16_384.0) * 8.0 / duration).floor();
    let audio_bitrate = if audio {
        if total < 200_000.0 {
            32_000
        } else {
            96_000
        }
    } else {
        0
    };
    let video = total - audio_bitrate as f64;
    if video < 10_000.0 {
        bail!(
            "Size budget is too small for the full {:.2}s video{}; choose a larger --max-mb limit",
            duration,
            if audio { " and audio" } else { "" }
        );
    }
    Ok((cap, video as u64, audio_bitrate))
}

pub fn compress(
    input: &Path,
    max_mb: f64,
    output: Option<&Path>,
    overwrite: bool,
) -> Result<Value> {
    let input = input_path(input)?;
    let ffmpeg = process::tool("ffmpeg")?;
    let ffprobe = process::tool("ffprobe")?;
    let video = probe(&ffprobe, &input)?;
    let (cap, mut video_bitrate, audio_bitrate) = budget(max_mb, video.duration, video.audio)?;
    let (destination, explicit) =
        output_path(&input, output, &format!("{max_mb}MB"), "mp4", overwrite)?;
    if !destination
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
    {
        bail!("Compression output must have a .mp4 extension (H.264/AAC)");
    }
    let temporary = tempfile::Builder::new()
        .prefix(".rightclick-utilities-")
        .tempdir_in(destination.parent().unwrap())?;
    let encoded = temporary.path().join("compressed.mp4");
    eprintln!("Encoding full video with a limit of {cap} bytes");
    eprintln!("progress=0");
    for attempt in 0..3 {
        process::check_cancelled()?;
        let passlog = temporary.path().join(format!("pass-{attempt}"));
        let start = if attempt == 0 {
            0
        } else if attempt == 1 {
            85
        } else {
            93
        };
        let end = if attempt == 0 {
            85
        } else if attempt == 1 {
            93
        } else {
            98
        };
        for pass in 1..=2 {
            eprintln!("Encoding attempt {}, pass {pass}/2", attempt + 1);
            let mut args = base_args(&input);
            args.extend([
                "-map".into(),
                format!("0:{}", video.index),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "medium".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-vf".into(),
                "pad=ceil(iw/2)*2:ceil(ih/2)*2".into(),
                "-b:v".into(),
                video_bitrate.to_string(),
                "-pass".into(),
                pass.to_string(),
                "-passlogfile".into(),
                passlog.to_string_lossy().into_owned(),
                "-map_metadata".into(),
                "-1".into(),
                "-map_chapters".into(),
                "-1".into(),
                "-progress".into(),
                "pipe:1".into(),
                "-nostats".into(),
            ]);
            if pass == 1 {
                args.extend([
                    "-an".into(),
                    "-f".into(),
                    "null".into(),
                    if cfg!(windows) {
                        "NUL".into()
                    } else {
                        "/dev/null".into()
                    },
                ]);
            } else {
                if video.audio {
                    args.extend([
                        "-map".into(),
                        "0:a:0".into(),
                        "-c:a".into(),
                        "aac".into(),
                        "-b:a".into(),
                        audio_bitrate.to_string(),
                    ]);
                } else {
                    args.push("-an".into());
                }
                args.extend([
                    "-movflags".into(),
                    "+faststart".into(),
                    encoded.to_string_lossy().into_owned(),
                ]);
            }
            let middle = start + (end - start) / 2;
            process::run(
                &ffmpeg,
                &args,
                Some(video.duration),
                if pass == 1 {
                    (start, middle)
                } else {
                    (middle, end)
                },
                false,
            )?;
        }
        let bytes = std::fs::metadata(&encoded)?.len();
        if bytes < cap {
            let result_video = probe(&ffprobe, &encoded)?;
            check_duration(&video, &result_video)?;
            if video.audio && !result_video.audio {
                bail!("Encoded output lost its audio; output was not published");
            }
            let output = publish(&encoded, &destination, explicit, overwrite)?;
            eprintln!("progress=100");
            return Ok(
                json!({"success":true,"operation":"compress","output":crate::display_path(&output),"bytes":bytes,"duration":result_video.duration,"maxMb":max_mb}),
            );
        }
        eprintln!("Encoded output exceeded the limit; reducing bitrate and retrying");
        video_bitrate = (video_bitrate as f64 * (cap as f64 / bytes as f64) * 0.92) as u64;
        if video_bitrate < 10_000 {
            bail!("Size budget cannot be met for the full video; choose a larger --max-mb limit");
        }
    }
    bail!("Could not meet the size cap after three full-duration encoding attempts; choose a larger limit")
}

pub fn strip_audio(input: &Path, output: Option<&Path>, overwrite: bool) -> Result<Value> {
    let input = input_path(input)?;
    let ffmpeg = process::tool("ffmpeg")?;
    let ffprobe = process::tool("ffprobe")?;
    let video = probe(&ffprobe, &input)?;
    let extension=input.extension().and_then(|e|e.to_str()).context("Input requires a container extension for stream-copy output; pass a file with a supported video extension")?;
    let (destination, explicit) = output_path(&input, output, "no-audio", extension, overwrite)?;
    let output_extension = destination
        .extension()
        .and_then(|e| e.to_str())
        .context("Output requires a video container extension")?;
    let temporary = tempfile::Builder::new()
        .prefix(".rightclick-utilities-")
        .tempdir_in(destination.parent().unwrap())?;
    let encoded = temporary
        .path()
        .join(format!("no-audio.{output_extension}"));
    let mut args = base_args(&input);
    args.extend([
        "-map".into(),
        "0".into(),
        "-map".into(),
        "-0:a".into(),
        "-c".into(),
        "copy".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        encoded.to_string_lossy().into_owned(),
    ]);
    eprintln!("Removing audio with stream copy");
    eprintln!("progress=0");
    process::run(&ffmpeg, &args, Some(video.duration), (0, 98), false)
        .context("Stream copy failed; the output container may not support the source streams")?;
    let result_video = probe(&ffprobe, &encoded)?;
    check_duration(&video, &result_video)?;
    if result_video.audio {
        bail!("Output still has audio; output was not published");
    }
    let bytes = std::fs::metadata(&encoded)?.len();
    let output = publish(&encoded, &destination, explicit, overwrite)?;
    eprintln!("progress=100");
    Ok(
        json!({"success":true,"operation":"strip-audio","output":crate::display_path(&output),"bytes":bytes,"duration":result_video.duration}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data(format: &str, frames: &str, attached: u64, audio: bool) -> Value {
        let mut streams = vec![
            json!({"index":0,"codec_type":"video","duration":"10","avg_frame_rate":"30/1","nb_frames":frames,"disposition":{"attached_pic":attached}}),
        ];
        if audio {
            streams.push(json!({"index":1,"codec_type":"audio"}));
        }
        json!({"format":{"format_name":format,"duration":"10"},"streams":streams})
    }
    #[test]
    fn actual_video_is_required() {
        assert!(video_info(&data("mov,mp4,m4a,3gp,3g2,mj2", "300", 0, true)).is_ok());
        assert!(video_info(&data("png_pipe", "N/A", 0, false)).is_err());
        assert!(video_info(&data("gif", "300", 0, false)).is_err());
        assert!(video_info(&data("mp3", "300", 1, true)).is_err());
        assert!(video_info(&data("mp4", "1", 0, false)).is_err());
    }
    #[test]
    fn budget_uses_decimal_mb_and_rejects_impossible_requests() {
        let (cap, v, a) = budget(15.0, 60.0, true).unwrap();
        assert_eq!(cap, 15_000_000);
        assert_eq!(a, 96_000);
        assert!(v > 0);
        assert!(budget(f64::NAN, 1.0, false).is_err());
        assert!(budget(0.0001, 60.0, true).is_err());
    }
    #[test]
    fn publishing_never_replaces_existing_default_output() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("temporary");
        let target = dir.path().join("clip.mp4");
        std::fs::write(&temp, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        let path = publish(&temp, &target, false, false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(path.file_name().unwrap(), "clip (1).mp4");
        assert_eq!(std::fs::read(path).unwrap(), b"new");
    }
    #[test]
    fn explicit_collision_and_input_overwrite_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.mp4");
        std::fs::write(&input, b"input").unwrap();
        let input = input.canonicalize().unwrap();
        assert!(output_path(&input, Some(&input), "x", "mp4", true).is_err());
        let temporary = dir.path().join("temporary.mp4");
        std::fs::write(&temporary, b"encoded").unwrap();
        assert!(publish(&temporary, &input, true, false).is_err());
    }
}
