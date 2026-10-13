use anyhow::{bail, Context, Result};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

static CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn initialize() -> Result<()> {
    ctrlc::set_handler(|| CANCELLED.store(true, Ordering::SeqCst))?;
    Ok(())
}

pub fn check_cancelled() -> Result<()> {
    if CANCELLED.load(Ordering::SeqCst) {
        bail!("Operation cancelled");
    }
    Ok(())
}

pub fn tool(name: &str) -> Result<PathBuf> {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let beside = std::env::current_exe()?
        .parent()
        .context("Executable directory unavailable")?
        .join(&filename);
    if beside.is_file() {
        return Ok(beside);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(&filename);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    bail!("{filename} was not found. Put FFmpeg and ffprobe beside UtilitiesCli.exe or add their folder to PATH")
}

fn command(executable: &Path) -> Command {
    let mut cmd = Command::new(executable);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

// The kernel kills children if the CLI is forcibly stopped, including from the GUI.
#[cfg(windows)]
struct ChildJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ChildJob {
    fn attach(child: &std::process::Child) -> Result<Self> {
        use std::{
            mem::{size_of, zeroed},
            os::windows::io::AsRawHandle,
        };
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle()) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(error.into());
            }
            Ok(Self(job))
        }
    }
}
#[cfg(windows)]
impl Drop for ChildJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub fn run(
    executable: &Path,
    args: &[String],
    duration: Option<f64>,
    range: (u32, u32),
    log_stdout: bool,
) -> Result<Vec<u8>> {
    check_cancelled()?;
    let mut child = command(executable)
        .args(args)
        .spawn()
        .with_context(|| format!("Cannot start {}", executable.display()))?;
    #[cfg(windows)]
    let _job = match ChildJob::attach(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.context("Cannot attach child cancellation job"));
        }
    };
    let stdout = child.stdout.take().context("Missing process stdout")?;
    let stderr = child.stderr.take().context("Missing process stderr")?;
    let reader = thread::spawn(move || {
        let mut data = Vec::new();
        let mut last = range.0;
        for line in BufReader::new(stdout).split(b'\n') {
            let Ok(line) = line else {
                break;
            };
            if let Some(duration) = duration {
                if let Some(value) = String::from_utf8_lossy(&line)
                    .trim()
                    .strip_prefix("out_time_us=")
                    .and_then(|v| v.parse::<f64>().ok())
                {
                    let progress = range.0
                        + ((range.1 - range.0) as f64
                            * (value / 1_000_000.0 / duration).clamp(0.0, 1.0))
                            as u32;
                    if progress > last {
                        eprintln!("progress={progress}");
                        last = progress;
                    }
                }
            } else if log_stdout {
                eprintln!("{}", String::from_utf8_lossy(&line));
            } else {
                data.extend_from_slice(&line);
                data.push(b'\n');
            }
        }
        data
    });
    let errors = thread::spawn(move || {
        let mut data = Vec::new();
        // Drain continually to avoid pipe deadlocks; keep only a useful diagnostic tail.
        for line in BufReader::new(stderr).split(b'\n') {
            let Ok(line) = line else {
                break;
            };
            if log_stdout {
                eprintln!("{}", String::from_utf8_lossy(&line));
            }
            data.extend_from_slice(&line);
            data.push(b'\n');
            if data.len() > 16_384 {
                data.drain(..data.len() - 8192);
            }
        }
        data
    });
    let status = loop {
        if CANCELLED.load(Ordering::SeqCst) {
            let _ = child.kill();
            break child.wait()?;
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(80));
    };
    #[cfg(windows)]
    drop(_job);
    let stdout = reader
        .join()
        .map_err(|_| anyhow::anyhow!("Process output reader failed"))?;
    let stderr = errors
        .join()
        .map_err(|_| anyhow::anyhow!("Process error reader failed"))?;
    check_cancelled()?;
    if !status.success() {
        bail!(
            "{} exited with {}: {}",
            executable.file_name().unwrap_or_default().to_string_lossy(),
            status,
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    Ok(stdout)
}

pub fn custom(executable: &str, args: &[String]) -> Result<()> {
    run(Path::new(executable), args, None, (0, 100), true)?;
    Ok(())
}
