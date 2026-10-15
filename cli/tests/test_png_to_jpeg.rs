use image::{codecs::jpeg::JpegEncoder, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};

fn cli(args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_UtilitiesCli"))
        .args(args)
        .output()
        .unwrap();
    let result = serde_json::from_slice(&output.stdout).expect("one JSON result");
    (output, result)
}

fn png(path: &Path, pixel: [u8; 4]) {
    RgbaImage::from_pixel(24, 17, Rgba(pixel))
        .save_with_format(path, ImageFormat::Png)
        .unwrap();
}

#[test]
fn transparency_dimensions_and_quality_90_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    for (name, pixel, rgb) in [
        ("transparent ü {cli}", [90, 20, 10, 0], [255, 255, 255]),
        ("partial", [255, 0, 0, 128], [255, 127, 127]),
        ("opaque", [30, 60, 90, 255], [30, 60, 90]),
    ] {
        let input = dir.path().join(format!("{name}.png"));
        png(&input, pixel);
        let original = std::fs::read(&input).unwrap();
        let (output, result) = cli(&["png-to-jpeg", input.to_str().unwrap(), "--json"]);
        assert!(output.status.success(), "{result}");
        let path = Path::new(result["outputs"][0]["output"].as_str().unwrap());
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (24, 17));
        let mut expected = Vec::new();
        JpegEncoder::new_with_quality(&mut expected, 90)
            .encode_image(&RgbImage::from_pixel(24, 17, Rgb(rgb)))
            .unwrap();
        assert_eq!(bytes, expected, "must encode flattened image at quality 90");
        assert_eq!(std::fs::read(&input).unwrap(), original);
    }
}

#[test]
fn folders_only_convert_direct_png_children_and_collisions_are_safe() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    png(&dir.path().join("one.PNG"), [0, 0, 0, 255]);
    png(&nested.join("nested.png"), [0, 0, 0, 255]);
    std::fs::write(dir.path().join("ignore.txt"), b"ignored").unwrap();
    std::fs::write(dir.path().join("one.jpg"), b"existing").unwrap();
    for expected in ["one (1).jpg", "one (2).jpg"] {
        let (output, result) = cli(&["png-to-jpeg", dir.path().to_str().unwrap(), "--json"]);
        assert!(output.status.success(), "{result}");
        assert_eq!(result["outputs"].as_array().unwrap().len(), 1);
        assert!(dir.path().join(expected).is_file());
    }
    assert_eq!(
        std::fs::read(dir.path().join("one.jpg")).unwrap(),
        b"existing"
    );
    assert!(!nested.join("nested.jpg").exists());
}

#[test]
fn empty_folder_reports_an_actionable_error() {
    let dir = tempfile::tempdir().unwrap();
    let (output, result) = cli(&["png-to-jpeg", dir.path().to_str().unwrap(), "--json"]);
    assert!(!output.status.success());
    assert_eq!(result["success"], false);
    assert!(result["errors"][0]["error"]
        .as_str()
        .unwrap()
        .contains("no direct PNG"));
}

#[test]
fn mixed_inputs_continue_after_invalid_and_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.png");
    let invalid = dir.path().join("invalid.png");
    let wrong_format = dir.path().join("jpeg.png");
    let missing = dir.path().join("missing.png");
    let last = dir.path().join("last.png");
    png(&first, [0, 0, 0, 255]);
    png(&last, [255, 255, 255, 255]);
    std::fs::write(&invalid, b"\x89PNG\r\n\x1a\ninvalid").unwrap();
    RgbImage::new(2, 2)
        .save_with_format(&wrong_format, ImageFormat::Jpeg)
        .unwrap();
    let (output, result) = cli(&[
        "png-to-jpeg",
        first.to_str().unwrap(),
        invalid.to_str().unwrap(),
        missing.to_str().unwrap(),
        wrong_format.to_str().unwrap(),
        last.to_str().unwrap(),
        "--json",
    ]);
    assert!(!output.status.success());
    assert_eq!(result["success"], false);
    assert_eq!(result["outputs"].as_array().unwrap().len(), 2);
    assert_eq!(result["errors"].as_array().unwrap().len(), 3);
    assert!(first.with_extension("jpg").is_file());
    assert!(last.with_extension("jpg").is_file());
    assert!(!invalid.with_extension("jpg").exists());
    assert!(!wrong_format.with_extension("jpg").exists());
}

#[test]
fn preset_accepts_multiple_files_and_folders_and_old_presets_reject_multiple() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("folder.with.extension");
    std::fs::create_dir(&folder).unwrap();
    let first = dir.path().join("first.png");
    png(&first, [0, 0, 0, 255]);
    png(&folder.join("second.png"), [0, 0, 0, 255]);
    let config = dir.path().join("config.json");
    let (output, result) = cli(&[
        "run",
        "png-jpeg",
        first.to_str().unwrap(),
        folder.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
        "--json",
    ]);
    assert!(output.status.success(), "{result}");
    assert_eq!(result["outputs"].as_array().unwrap().len(), 2);
    let settings: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
    assert_eq!(settings["functions"][4]["supportsMultiple"], true);
    assert!(settings["functions"][0].get("supportsMultiple").is_none());
    let (output, result) = cli(&[
        "run",
        "video-15mb",
        first.to_str().unwrap(),
        first.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
        "--json",
    ]);
    assert!(!output.status.success());
    assert!(result["error"]
        .as_str()
        .unwrap()
        .contains("multiple inputs"));
}
