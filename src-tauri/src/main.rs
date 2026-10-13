#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Function {
    id: String,
    label: String,
    enabled: bool,
    applies_to: String,
    extensions: Vec<String>,
    executable: String,
    args: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Config {
    version: u32,
    functions: Vec<Function>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    running: bool,
    progress: Option<u32>,
    message: String,
    logs: Vec<String>,
    result: Option<Value>,
    #[serde(skip)]
    pid: Option<u32>,
    #[serde(skip)]
    cancelled: bool,
}
#[derive(Default)]
struct Jobs(Mutex<Job>);

fn hidden_command(executable: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(executable);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    cmd
}
fn cli_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let sibling = exe.parent().unwrap().join("UtilitiesCli.exe");
    if sibling.is_file() {
        return Ok(sibling);
    }
    let dev =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cli/target/release/UtilitiesCli.exe");
    if dev.is_file() {
        return Ok(dev);
    }
    Err(
        "UtilitiesCli.exe is missing. Keep it beside Utilities.exe or run scripts/build.ps1."
            .into(),
    )
}
fn config_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("USERPROFILE").ok_or("USERPROFILE is unavailable.")?;
    Ok(PathBuf::from(base).join(".rightclick-utilities/functions.json"))
}
fn cli_check(args: &[&str]) -> Result<(), String> {
    let out = hidden_command(cli_path()?)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let value: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    Err(value["error"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_owned()))
}
#[tauri::command]
fn get_config() -> Result<Config, String> {
    let path = config_path()?;
    if !path.exists() {
        cli_check(&["functions", "--json"])?;
    }
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Cannot read functions: {e}"))
}
#[tauri::command]
fn save_config(config: Config, jobs: tauri::State<Jobs>) -> Result<(), String> {
    if jobs.0.lock().unwrap().running {
        return Err("Wait for the current function to finish before editing functions.".into());
    }
    let path = config_path()?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let checked = cli_check(&["functions", "--config", &temp.to_string_lossy(), "--json"]);
    if let Err(error) = checked {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    fs::rename(&temp, &path).map_err(|e| format!("Cannot save functions: {e}"))
}
#[tauri::command]
fn launch_context() -> Value {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    json!({"functionId":arg("--run-function"),"input":arg("--input")})
}
#[tauri::command]
fn job_status(jobs: tauri::State<Jobs>) -> Job {
    jobs.0.lock().unwrap().clone()
}
#[tauri::command]
fn start_job(
    app: tauri::AppHandle,
    jobs: tauri::State<Jobs>,
    function_id: String,
    input: String,
) -> Result<(), String> {
    let mut job = jobs.0.lock().unwrap();
    if job.running {
        return Err("A function is already running in this window.".into());
    }
    if input.trim().is_empty() {
        return Err("Choose a file or folder first.".into());
    }
    let mut child = hidden_command(cli_path()?)
        .args(["run", &function_id, &input, "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    *job = Job {
        running: true,
        message: "Starting…".into(),
        pid: Some(child.id()),
        ..Job::default()
    };
    std::thread::spawn(move || {
        let output_thread = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stdout).read_to_string(&mut text);
            text
        });
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let state = app.state::<Jobs>();
            let mut job = state.0.lock().unwrap();
            if let Some(percent) = line
                .strip_prefix("progress=")
                .and_then(|s| s.parse::<u32>().ok())
            {
                job.progress = Some(percent.min(100));
            } else {
                job.message = line.clone();
                job.logs.push(line);
                if job.logs.len() > 100 {
                    job.logs.remove(0);
                }
            }
        }
        let exit = child.wait();
        let text = output_thread.join().unwrap_or_default();
        let state = app.state::<Jobs>();
        let mut job = state.0.lock().unwrap();
        let result = if job.cancelled {
            json!({"success":false,"error":"Cancelled. The original is unchanged."})
        } else {
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({"success":false,"error":format!("The runner did not return a valid result. {}",text.trim())}))
        };
        let success = exit.is_ok_and(|s| s.success()) && result["success"] == true;
        job.message = if success {
            "Complete".into()
        } else {
            result["error"]
                .as_str()
                .unwrap_or("Function failed.")
                .into()
        };
        if success {
            job.progress = Some(100);
        }
        job.result = Some(result);
        job.running = false;
        job.pid = None;
    });
    Ok(())
}
fn cancel(jobs: &Jobs) -> Result<(), String> {
    let mut job = jobs.0.lock().unwrap();
    if let Some(pid) = job.pid {
        let status = hidden_command("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("The process could not be cancelled; it may have just finished.".into());
        }
        job.cancelled = true;
        job.message = "Cancelling…".into();
    }
    Ok(())
}
#[tauri::command]
fn cancel_job(jobs: tauri::State<Jobs>) -> Result<(), String> {
    cancel(&jobs)
}
#[tauri::command]
fn open_output(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err("The output no longer exists.".into());
    }
    let arg = if path.is_dir() {
        path.to_string_lossy().into_owned()
    } else {
        format!("/select,{}", path.display())
    };
    hidden_command("explorer.exe")
        .arg(arg)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Jobs::default())
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            launch_context,
            job_status,
            start_job,
            cancel_job,
            open_output
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                let _ = cancel(&window.state::<Jobs>());
            }
        })
        .run(tauri::generate_context!())
        .expect("Could not start RightClick Utilities");
}
