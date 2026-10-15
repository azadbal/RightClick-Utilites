use anyhow::{bail, Context, Result};
use image::{codecs::jpeg::JpegEncoder, ImageFormat};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn convert(input: &Path) -> Result<Value> {
    crate::process::check_cancelled()?;
    let input = input.canonicalize().context("Cannot access PNG file")?;
    let bytes = std::fs::read(&input).context("Cannot read PNG file")?;
    if image::guess_format(&bytes).ok() != Some(ImageFormat::Png) {
        bail!("Input is not a PNG image");
    }
    let rgba = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .context("Cannot decode PNG image")?
        .to_rgba8();
    let (width, height) = rgba.dimensions();
    let rgb = image::RgbImage::from_fn(width, height, |x, y| {
        let pixel = rgba.get_pixel(x, y);
        let alpha = u32::from(pixel[3]);
        image::Rgb(
            [0, 1, 2]
                .map(|c| ((u32::from(pixel[c]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8),
        )
    });
    let parent = input.parent().context("Input has no parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    JpegEncoder::new_with_quality(temporary.as_file_mut(), 90)
        .encode_image(&rgb)
        .context("Cannot encode JPEG image")?;
    temporary.as_file().sync_all()?;
    let output =
        crate::media::publish(temporary.path(), &input.with_extension("jpg"), false, false)?;
    Ok(
        json!({"input":crate::display_path(&input),"output":crate::display_path(&output),
        "bytes":output.metadata()?.len(),"width":width,"height":height}),
    )
}

pub fn png_to_jpeg(inputs: &[PathBuf]) -> Result<Value> {
    crate::process::check_cancelled()?;
    eprintln!("Converting PNG images to JPEG (quality 90)");
    eprintln!("progress=0");
    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    let mut files = Vec::new();
    for input in inputs {
        crate::process::check_cancelled()?;
        if input.is_dir() {
            match std::fs::read_dir(input) {
                Ok(entries) => {
                    let mut children = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(entry) => {
                                let path = entry.path();
                                if path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
                                    children.push(path);
                                }
                            }
                            Err(error) => errors.push(json!({"input":crate::display_path(input),"error":error.to_string()})),
                        }
                    }
                    if children.is_empty() {
                        errors.push(json!({"input":crate::display_path(input),"error":"Folder contains no direct PNG files"}));
                    }
                    children.sort();
                    files.extend(children);
                }
                Err(error) => errors
                    .push(json!({"input":crate::display_path(input),"error":error.to_string()})),
            }
        } else {
            files.push(input.clone());
        }
    }
    let total = files.len();
    for (index, input) in files.into_iter().enumerate() {
        crate::process::check_cancelled()?;
        eprintln!("Converting {}", crate::display_path(&input));
        match convert(&input) {
            Ok(output) => outputs.push(output),
            Err(error) => {
                crate::process::check_cancelled()?;
                eprintln!("{}: {error:#}", crate::display_path(&input));
                errors.push(
                    json!({"input":crate::display_path(&input),"error":format!("{error:#}")}),
                );
            }
        }
        eprintln!("progress={}", ((index + 1) * 100 / total).min(99));
    }
    if errors.is_empty() {
        eprintln!("progress=100");
    }
    let mut result = json!({"success":errors.is_empty(),"operation":"png-to-jpeg","outputs":outputs,"errors":errors});
    if !errors.is_empty() {
        result["error"] = json!(format!(
            "{} conversion error(s); {} JPEG file(s) saved. {}",
            errors.len(),
            outputs.len(),
            errors[0]["error"].as_str().unwrap_or("Conversion failed")
        ));
    }
    Ok(result)
}
