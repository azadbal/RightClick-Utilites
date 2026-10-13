mod config;
mod media;
mod process;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else {
        value.strip_prefix("\\\\?\\").unwrap_or(&value).to_owned()
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;
    #[test]
    fn display_paths_preserve_drive_and_unc_paths() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Videos\clip.mp4")),
            r"C:\Videos\clip.mp4"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\clip.mp4")),
            r"\\server\share\clip.mp4"
        );
    }
}

#[derive(Parser)]
#[command(
    name = "UtilitiesCli",
    version,
    about = "Configurable Windows file utilities",
    after_help = "Examples:\n  UtilitiesCli compress \"C:\\Videos\\clip.mp4\" --max-mb 20 --json\n  UtilitiesCli strip-audio \"C:\\Videos\\clip.mp4\"\n  UtilitiesCli functions --json\n  UtilitiesCli run video-15mb \"C:\\Videos\\clip.mp4\" --json\n\nFFmpeg and ffprobe are required for video operations; put them beside this executable or on PATH.\nRelative paths resolve from the current directory. Outputs default beside the input, with numbered\ncollision-safe names. Limits use decimal MB (1,000,000 bytes). Originals are always preserved.\nJSON mode emits one final stdout object; stderr progress lines are progress=0 through progress=100."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Encode full video to H.264/AAC under a decimal MB limit
    Compress {
        input: PathBuf,
        #[arg(long)]
        max_mb: f64,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        overwrite: bool,
        #[arg(long)]
        json: bool,
    },
    /// Remove audio without re-encoding the video
    StripAudio {
        input: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        overwrite: bool,
        #[arg(long)]
        json: bool,
    },
    /// Run an enabled configured function on one file or folder
    Run {
        function_id: String,
        input: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// List configured functions; create defaults when no config exists
    Functions {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

fn execute(command: Commands) -> Result<Value> {
    match command {
        Commands::Compress {
            input,
            max_mb,
            output,
            overwrite,
            ..
        } => media::compress(&input, max_mb, output.as_deref(), overwrite),
        Commands::StripAudio {
            input,
            output,
            overwrite,
            ..
        } => media::strip_audio(&input, output.as_deref(), overwrite),
        Commands::Functions { config, .. } => {
            let path = config::config_path(config)?;
            let config = config::load(&path)?;
            Ok(json!({"success":true,"config":path,"functions":config.functions}))
        }
        Commands::Run {
            function_id,
            input,
            config,
            ..
        } => run_function(&function_id, &input, config),
    }
}

fn run_function(id: &str, input: &Path, config_path: Option<PathBuf>) -> Result<Value> {
    let config = config::load(&config::config_path(config_path)?)?;
    let function = config
        .functions
        .iter()
        .find(|f| f.id == id)
        .context("Unknown function ID; use 'functions' to list available functions")?;
    if !function.enabled {
        bail!("Function '{id}' is disabled");
    }
    let input = input
        .canonicalize()
        .context("Input does not exist or cannot be accessed")?;
    if (function.applies_to == "file" && !input.is_file())
        || (function.applies_to == "folder" && !input.is_dir())
    {
        bail!("Function does not apply to this input type");
    }
    let extension = input
        .extension()
        .map(|v| format!(".{}", v.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    if !function.extensions.is_empty() && !function.extensions.contains(&extension) {
        bail!("Function does not support this file extension");
    }
    let cli = std::env::current_exe()?;
    let args: Vec<String> = function
        .args
        .iter()
        .map(|s| config::expand(s, &input, &cli))
        .collect();
    if function.executable == "{cli}" {
        // Parsing the same argument contract keeps the GUI and direct CLI behavior identical.
        let parsed =
            Cli::try_parse_from(std::iter::once(cli.to_string_lossy().into_owned()).chain(args))
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        match parsed.command {
            Commands::Compress { .. } | Commands::StripAudio { .. } => execute(parsed.command),
            _ => bail!("Built-in functions may only invoke compress or strip-audio"),
        }
    } else {
        let executable = config::expand(&function.executable, &input, &cli);
        process::custom(&executable, &args)?;
        Ok(json!({"success":true,"function":id,"input":display_path(&input)}))
    }
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let help = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            let json_mode = std::env::args().any(|arg| arg == "--json");
            if json_mode && !help {
                println!("{}", json!({"success":false,"error":error.to_string()}));
            } else {
                let _ = error.print();
            }
            std::process::exit(if help { 0 } else { 2 });
        }
    };
    let json_mode = match &cli.command {
        Commands::Compress { json, .. }
        | Commands::StripAudio { json, .. }
        | Commands::Run { json, .. }
        | Commands::Functions { json, .. } => *json,
    };
    if let Err(error) = process::initialize() {
        eprintln!("Cannot initialize cancellation: {error}");
        if json_mode {
            println!("{}", json!({"success":false,"error":error.to_string()}));
        }
        std::process::exit(1);
    }
    match execute(cli.command) {
        Ok(result) => {
            if json_mode {
                println!("{result}");
            } else if let Some(path) = result.get("output") {
                println!("Saved {}", path.as_str().unwrap_or_default());
            } else if let Some(functions) = result.get("functions").and_then(Value::as_array) {
                for f in functions {
                    println!(
                        "{}\t{}\t{}",
                        f["id"].as_str().unwrap_or_default(),
                        f["label"].as_str().unwrap_or_default(),
                        if f["enabled"] == true {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                }
            } else {
                println!("Completed successfully");
            }
        }
        Err(error) => {
            let message = format!("{error:#}");
            eprintln!("Error: {message}");
            if json_mode {
                println!("{}", json!({"success":false,"error":message}));
            }
            std::process::exit(1);
        }
    }
}
