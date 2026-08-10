use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

mod i18n;

use i18n::{tr, tr_args};
use patchsplit::{split_patch_by_commit, PatchPart};

fn main() {
    i18n::init();

    match run() {
        Ok(()) => {}
        Err(AppError::Help) => {
            println!("{}", usage());
        }
        Err(AppError::Version) => {
            println!(
                "{}",
                tr_args(
                    "patchsplit {version}",
                    &[("version", patchsplit::version().to_string())]
                )
            );
        }
        Err(error) => {
            eprintln!(
                "{}",
                tr_args("error: {message}", &[("message", error.to_string())])
            );
            eprintln!();
            eprintln!("{}", usage());
            std::process::exit(error.exit_code());
        }
    }
}

fn run() -> Result<(), AppError> {
    let config = Config::parse(env::args().skip(1))?;
    let url = config.patch_url();
    let patch = download_patch(&url)?;
    let parts = split_patch_by_commit(&patch);

    if parts.is_empty() {
        return Err(AppError::EmptyPatch);
    }

    let written = write_parts(&parts, &config.output_dir, config.force)?;

    println!("{}", tr_args("downloaded {url}", &[("url", url)]));
    println!(
        "{}",
        tr_args(
            "wrote {count} patch file(s) to {directory}",
            &[
                ("count", written.len().to_string()),
                ("directory", config.output_dir.display().to_string()),
            ]
        )
    );
    for path in written {
        println!("{}", path.display());
    }

    Ok(())
}

#[derive(Debug)]
struct Config {
    owner: String,
    repo: String,
    pull_request: u64,
    output_dir: PathBuf,
    force: bool,
}

impl Config {
    fn parse<I>(args: I) -> Result<Self, AppError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut output_dir = PathBuf::from("patches");
        let mut force = false;
        let mut positionals = Vec::new();
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Err(AppError::Help),
                "-V" | "--version" => return Err(AppError::Version),
                "-f" | "--force" => force = true,
                "-o" | "--out" => {
                    let option = arg.as_str().to_string();
                    let value = args
                        .next()
                        .ok_or(AppError::MissingOptionValue(option))?;
                    output_dir = PathBuf::from(value);
                }
                value if value.starts_with("--out=") => {
                    output_dir = PathBuf::from(&value["--out=".len()..]);
                }
                value if value.starts_with('-') => {
                    return Err(AppError::UnknownOption(value.to_string()));
                }
                value => positionals.push(value.to_string()),
            }
        }

        let (owner, repo, pull_request) = match positionals.as_slice() {
            [repo_spec, pull_request] => {
                let (owner, repo) = parse_repo_spec(repo_spec)?;
                (owner, repo, parse_pull_request(pull_request)?)
            }
            [owner, repo, pull_request] => {
                validate_repo_segment("owner", owner)?;
                validate_repo_segment("repo", repo)?;
                (owner.clone(), repo.clone(), parse_pull_request(pull_request)?)
            }
            _ => return Err(AppError::InvalidArguments),
        };

        Ok(Self {
            owner,
            repo,
            pull_request,
            output_dir,
            force,
        })
    }

    fn patch_url(&self) -> String {
        format!(
            "https://github.com/{}/{}/pull/{}.patch",
            self.owner, self.repo, self.pull_request
        )
    }
}

fn parse_repo_spec(value: &str) -> Result<(String, String), AppError> {
    let Some((owner, repo)) = value.split_once('/') else {
        return Err(AppError::InvalidRepoSpec(value.to_string()));
    };

    if repo.contains('/') {
        return Err(AppError::InvalidRepoSpec(value.to_string()));
    }

    validate_repo_segment("owner", owner)?;
    validate_repo_segment("repo", repo)?;
    Ok((owner.to_string(), repo.to_string()))
}

fn validate_repo_segment(kind: &'static str, value: &str) -> Result<(), AppError> {
    if value.is_empty()
        || value
            .chars()
            .any(|character| {
                character == '/' || character.is_whitespace() || character.is_control()
            })
    {
        return Err(AppError::InvalidRepoSegment {
            kind,
            value: value.to_string(),
        });
    }

    Ok(())
}

fn parse_pull_request(value: &str) -> Result<u64, AppError> {
    let pull_request = value
        .parse::<u64>()
        .map_err(|_| AppError::InvalidPullRequest(value.to_string()))?;

    if pull_request == 0 {
        Err(AppError::InvalidPullRequest(value.to_string()))
    } else {
        Ok(pull_request)
    }
}

fn download_patch(url: &str) -> Result<String, AppError> {
    // Rust's standard library has no HTTPS client; calling curl keeps this crate dependency-free.
    let output = Command::new("curl")
        .arg("--fail")
        .arg("--location")
        .arg("--silent")
        .arg("--show-error")
        .arg("--user-agent")
        .arg(format!("patchsplit/{}", patchsplit::version()))
        .arg(url)
        .output()
        .map_err(AppError::DownloadCommand)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::DownloadFailed {
            status: output.status.code(),
            message: stderr,
        });
    }

    String::from_utf8(output.stdout).map_err(AppError::PatchNotUtf8)
}

fn write_parts(
    parts: &[PatchPart],
    output_dir: &Path,
    force: bool,
) -> Result<Vec<PathBuf>, AppError> {
    fs::create_dir_all(output_dir).map_err(|source| AppError::CreateOutputDir {
        path: output_dir.to_path_buf(),
        source,
    })?;

    let mut written = Vec::with_capacity(parts.len());
    for part in parts {
        let path = output_dir.join(&part.filename);
        let mut options = OpenOptions::new();
        options.write(true);

        // Refuse overwrites by default so reruns do not replace manually edited patches.
        if force {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }

        let mut file = options.open(&path).map_err(|source| AppError::WritePatch {
            path: path.clone(),
            source,
        })?;
        file.write_all(part.content.as_bytes())
            .map_err(|source| AppError::WritePatch {
                path: path.clone(),
                source,
            })?;
        written.push(path);
    }

    Ok(written)
}

#[derive(Debug)]
enum AppError {
    Help,
    Version,
    InvalidArguments,
    InvalidRepoSpec(String),
    InvalidRepoSegment {
        kind: &'static str,
        value: String,
    },
    InvalidPullRequest(String),
    MissingOptionValue(String),
    UnknownOption(String),
    DownloadCommand(std::io::Error),
    DownloadFailed {
        status: Option<i32>,
        message: String,
    },
    PatchNotUtf8(std::string::FromUtf8Error),
    EmptyPatch,
    CreateOutputDir {
        path: PathBuf,
        source: std::io::Error,
    },
    WritePatch {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl AppError {
    fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidArguments
            | Self::InvalidRepoSpec(_)
            | Self::InvalidRepoSegment { .. }
            | Self::InvalidPullRequest(_)
            | Self::MissingOptionValue(_)
            | Self::UnknownOption(_) => 2,
            _ => 1,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for AppError {}

impl AppError {
    fn message(&self) -> String {
        match self {
            Self::Help | Self::Version => String::new(),
            Self::InvalidArguments => {
                tr("expected a GitHub repository and pull request number")
            }
            Self::InvalidRepoSpec(value) => tr_args(
                "repository must use owner/repo form, got {value}",
                &[("value", quoted(value))],
            ),
            Self::InvalidRepoSegment { kind, value } => tr_args(
                "invalid GitHub repository {kind}: {value}",
                &[("kind", repo_segment_label(kind)), ("value", quoted(value))],
            ),
            Self::InvalidPullRequest(value) => tr_args(
                "pull request number must be a positive integer, got {value}",
                &[("value", quoted(value))],
            ),
            Self::MissingOptionValue(option) => {
                tr_args("missing value for {option}", &[("option", option.clone())])
            }
            Self::UnknownOption(option) => {
                tr_args("unknown option {option}", &[("option", option.clone())])
            }
            Self::DownloadCommand(source) if source.kind() == std::io::ErrorKind::NotFound => {
                tr("curl was not found in PATH")
            }
            Self::DownloadCommand(source) => tr_args(
                "failed to run curl: {source}",
                &[("source", source.to_string())],
            ),
            Self::DownloadFailed { status, message } => match (status, message.is_empty()) {
                (Some(code), false) => tr_args(
                    "download failed with status {status}: {message}",
                    &[
                        ("status", code.to_string()),
                        ("message", message.to_string()),
                    ],
                ),
                (Some(code), true) => tr_args(
                    "download failed with status {status}",
                    &[("status", code.to_string())],
                ),
                (None, false) => tr_args(
                    "download failed: {message}",
                    &[("message", message.to_string())],
                ),
                (None, true) => tr("download failed"),
            },
            Self::PatchNotUtf8(_) => tr("downloaded patch is not valid UTF-8"),
            Self::EmptyPatch => tr("downloaded patch is empty"),
            Self::CreateOutputDir { path, source } => tr_args(
                "failed to create output directory {path}: {source}",
                &[
                    ("path", path.display().to_string()),
                    ("source", source.to_string()),
                ],
            ),
            Self::WritePatch { path, source }
                if source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                tr_args(
                    "refusing to overwrite existing patch file {path}; pass --force to replace it",
                    &[("path", path.display().to_string())],
                )
            }
            Self::WritePatch { path, source } => tr_args(
                "failed to write patch file {path}: {source}",
                &[
                    ("path", path.display().to_string()),
                    ("source", source.to_string()),
                ],
            ),
        }
    }
}

fn quoted(value: &str) -> String {
    format!("{value:?}")
}

fn repo_segment_label(kind: &str) -> String {
    match kind {
        "owner" => tr("owner"),
        "repo" => tr("repo"),
        other => other.to_string(),
    }
}

fn usage() -> String {
    tr("Usage:\n  patchsplit <owner/repo> <pr-number> [--out <dir>] [--force]\n  patchsplit <owner> <repo> <pr-number> [--out <dir>] [--force]\n\nOptions:\n  -o, --out <dir>   Output directory for split patch files [default: patches]\n  -f, --force       Overwrite existing patch files\n  -h, --help        Show this help\n  -V, --version     Show version\n\nExamples:\n  patchsplit rust-lang/rust 12345\n  patchsplit openai codex 42 -o pr-42-patches")
}
