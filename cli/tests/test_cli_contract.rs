use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Command, Output},
};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_UtilitiesCli"))
        .args(args)
        .output()
        .unwrap()
}

fn result(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout must be exactly one JSON result")
}

#[test]
fn help_and_version_work_without_media_dependencies() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("--json"));
    assert!(text.contains("FFmpeg"));
    let version = cli(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8(version.stdout).unwrap().contains("0.1.0"));
}

#[test]
fn parse_errors_are_machine_readable() {
    let output = cli(&["compress", "--json"]);
    assert!(!output.status.success());
    assert_eq!(result(&output)["success"], false);
    assert!(result(&output)["error"].is_string());
}

#[test]
fn missing_input_fails_without_polluting_json_stdout() {
    let output = cli(&["strip-audio", "missing-video.mp4", "--json"]);
    assert!(!output.status.success());
    assert_eq!(result(&output)["success"], false);
    assert!(String::from_utf8_lossy(&output.stderr).contains("existing video file"));
}

#[test]
fn config_defaults_and_invalid_config_contract() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("Unicode ü config.json");
    let path = config.to_str().unwrap();
    let output = cli(&["functions", "--config", path, "--json"]);
    assert!(output.status.success());
    assert!(config.is_file());
    let data = result(&output);
    assert_eq!(data["functions"][0]["id"], "video-15mb");
    assert_eq!(data["functions"].as_array().unwrap().len(), 3);
    std::fs::write(&config, b"{invalid}").unwrap();
    let output = cli(&["functions", "--config", path, "--json"]);
    assert!(!output.status.success());
    assert_eq!(result(&output)["success"], false);
}

fn write_config(path: &Path, functions: Value) {
    std::fs::write(
        path,
        serde_json::to_vec(&json!({"version":1,"functions":functions})).unwrap(),
    )
    .unwrap();
}

#[test]
fn disabled_and_duplicate_functions_fail_before_execution() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    let input = dir.path().join("input.mp4");
    std::fs::write(&input, b"not a video").unwrap();
    let f = json!({"id":"test","label":"Test","enabled":false,"appliesTo":"file","extensions":[],"executable":"{cli}","args":["strip-audio","{input}","--json"]});
    write_config(&config, json!([f]));
    let output = cli(&[
        "run",
        "test",
        input.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
        "--json",
    ]);
    assert!(!output.status.success());
    assert!(result(&output)["error"]
        .as_str()
        .unwrap()
        .contains("disabled"));
    write_config(&config, json!([f, f]));
    let output = cli(&["functions", "--config", config.to_str().unwrap(), "--json"]);
    assert!(result(&output)["error"]
        .as_str()
        .unwrap()
        .contains("Duplicate"));
}

#[cfg(windows)]
#[test]
fn custom_functions_receive_exact_arguments_and_relay_output_to_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    let input = dir.path().join("Unicode ü test file.txt");
    std::fs::write(&input, b"file").unwrap();
    let script = dir.path().join("check.ps1");
    std::fs::write(&script,"if ($args.Count -ne 1 -or -not (Test-Path -LiteralPath $args[0])) { exit 4 }; Write-Output 'custom output'").unwrap();
    let f = json!({"id":"custom","label":"Custom","enabled":true,"appliesTo":"file","extensions":[".txt"],"executable":"powershell.exe","args":["-NoProfile", "-File", script.to_str().unwrap(), "{input}"]});
    write_config(&config, json!([f]));
    let output = cli(&[
        "run",
        "custom",
        input.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(result(&output)["success"], true);
    assert!(result(&output).get("output").is_none());
    assert!(String::from_utf8_lossy(&output.stderr).contains("custom output"));
}
