use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub version: u32,
    pub functions: Vec<Function>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Function {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub applies_to: String,
    pub extensions: Vec<String>,
    pub executable: String,
    pub args: Vec<String>,
}

pub fn config_path(path: Option<PathBuf>) -> Result<PathBuf> {
    path.map(Ok).unwrap_or_else(|| {
        Ok(PathBuf::from(
            std::env::var_os("USERPROFILE")
                .context("USERPROFILE is unset; pass --config explicitly")?,
        )
        .join(".rightclick-utilities/functions.json"))
    })
}

pub fn defaults() -> Config {
    let extensions = [
        ".mp4", ".mov", ".mkv", ".avi", ".webm", ".m4v", ".mts", ".m2ts", ".wmv", ".flv", ".mpg",
        ".mpeg", ".ts", ".3gp", ".ogv",
    ]
    .map(str::to_owned)
    .to_vec();
    let mut functions = Vec::new();
    for mb in [15, 20] {
        functions.push(Function {
            id: format!("video-{mb}mb"),
            label: format!("Compress video below {mb} MB"),
            enabled: true,
            applies_to: "file".into(),
            extensions: extensions.clone(),
            executable: "{cli}".into(),
            args: vec![
                "compress".into(),
                "{input}".into(),
                "--max-mb".into(),
                mb.to_string(),
                "--json".into(),
            ],
        });
    }
    functions.push(Function {
        id: "convert-iphone".into(),
        label: "Convert for iPhone".into(),
        enabled: true,
        applies_to: "file".into(),
        extensions: extensions.clone(),
        executable: "{cli}".into(),
        args: vec![
            "compress".into(),
            "{input}".into(),
            "--max-mb".into(),
            "20".into(),
            "--json".into(),
        ],
    });
    functions.push(Function {
        id: "strip-audio".into(),
        label: "Remove audio from video".into(),
        enabled: true,
        applies_to: "file".into(),
        extensions,
        executable: "{cli}".into(),
        args: vec!["strip-audio".into(), "{input}".into(), "--json".into()],
    });
    Config {
        version: 1,
        functions,
    }
}

fn validate(config: &Config) -> Result<()> {
    if config.version != 1 {
        bail!("Unsupported config version {} (expected 1)", config.version);
    }
    let mut ids = HashSet::new();
    for f in &config.functions {
        if f.id.is_empty()
            || !f
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            bail!(
                "Invalid function ID '{}'; use letters, numbers, '-' and '_'",
                f.id
            );
        }
        if !ids.insert(&f.id) {
            bail!("Duplicate function ID '{}'", f.id);
        }
        if f.label.trim().is_empty() || f.executable.trim().is_empty() {
            bail!("Function '{}' requires a label and executable", f.id);
        }
        if !["file", "folder", "any"].contains(&f.applies_to.as_str()) {
            bail!("Invalid appliesTo for '{}'", f.id);
        }
        if f.extensions.iter().any(|e| {
            !e.starts_with('.')
                || e.len() < 2
                || e != &e.to_lowercase()
                || e.contains(['/', '\\', ' '])
        }) {
            bail!("Function '{}' requires lowercase dotted extensions", f.id);
        }
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<Config> {
    if !path.exists() {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).context("Cannot create config directory")?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(serde_json::to_string_pretty(&defaults())?.as_bytes())?;
        file.as_file().sync_all()?;
        // Another first launch can win this race; never overwrite its settings.
        if let Err(error) = file.persist_noclobber(path) {
            if !path.exists() {
                return Err(error.error.into());
            }
        }
    }
    let config =
        serde_json::from_slice(&std::fs::read(path).context("Cannot read function config")?)
            .context("Invalid function config JSON")?;
    validate(&config)?;
    Ok(config)
}

pub fn expand(value: &str, input: &Path, cli: &Path) -> String {
    let parent = input.parent().unwrap_or(Path::new(""));
    let stem = input.file_stem().unwrap_or_default();
    let tools = cli.parent().unwrap_or(Path::new(""));
    // Replace placeholders in the original template once, so braces in user filenames remain literal.
    let mut result = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(end) = tail.find('}') else {
            result.push_str(tail);
            return result;
        };
        let replacement = match &tail[..=end] {
            "{input}" => Some(crate::display_path(input)),
            "{parent}" => Some(crate::display_path(parent)),
            "{stem}" => Some(stem.to_string_lossy().into_owned()),
            "{tools}" => Some(crate::display_path(tools)),
            "{cli}" => Some(crate::display_path(cli)),
            _ => None,
        };
        result.push_str(replacement.as_deref().unwrap_or(&tail[..=end]));
        rest = &tail[end + 1..];
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_config_is_in_user_profile_and_override_is_preserved() {
        let profile = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
        assert_eq!(
            config_path(None).unwrap(),
            profile.join(".rightclick-utilities/functions.json")
        );
        let explicit = PathBuf::from("custom-functions.json");
        assert_eq!(config_path(Some(explicit.clone())).unwrap(), explicit);
    }
    #[test]
    fn defaults_valid_and_shared() {
        let config = defaults();
        validate(&config).unwrap();
        assert_eq!(config.functions.len(), 4);
        assert_eq!(config.functions[0].id, "video-15mb");
    }
    #[test]
    fn rejects_duplicate_ids_and_empty_executable() {
        let mut config = defaults();
        config.functions[1].id = config.functions[0].id.clone();
        assert!(validate(&config).is_err());
        config.functions[1].id = "different".into();
        config.functions[1].executable.clear();
        assert!(validate(&config).is_err());
    }
    #[test]
    fn placeholders_do_not_expand_inside_filenames() {
        assert_eq!(
            expand(
                "{input}",
                Path::new("clip-{cli}.mp4"),
                Path::new("UtilitiesCli.exe")
            ),
            "clip-{cli}.mp4"
        );
    }
    #[test]
    fn defaults_are_only_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("functions.json");
        load(&path).unwrap();
        let mut c = defaults();
        c.functions[0].enabled = false;
        std::fs::write(&path, serde_json::to_vec(&c).unwrap()).unwrap();
        assert!(!load(&path).unwrap().functions[0].enabled);
    }
}
