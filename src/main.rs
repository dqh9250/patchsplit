use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use patchsplit::{split_patch_by_commit, PatchPart};

fn main() {
    match run() {
        Ok(()) => {}
        Err(AppError::Help) => {
            println!("{}", usage());
        }
        Err(AppError::Version) => {
            println!("patchsplit {}", patchsplit::version());
        }
        Err(error) => {
            eprintln!("error: {error}");
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

    println!("downloaded {url}");
    println!(
        "wrote {} patch file(s) to {}",
        written.len(),
        config.output_dir.display()
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
                    let value = args.next().ok_or(AppError::MissingOptionValue(arg))?;
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
        match self {
            Self::Help | Self::Version => Ok(()),
            Self::InvalidArguments => write!(
                formatter,
                "expected a GitHub repository and pull request number"
            ),
            Self::InvalidRepoSpec(value) => {
                write!(formatter, "repository must use owner/repo form, got {value:?}")
            }
            Self::InvalidRepoSegment { kind, value } => {
                write!(formatter, "invalid GitHub repository {kind}: {value:?}")
            }
            Self::InvalidPullRequest(value) => {
                write!(formatter, "pull request number must be a positive integer, got {value:?}")
            }
            Self::MissingOptionValue(option) => write!(formatter, "missing value for {option}"),
            Self::UnknownOption(option) => write!(formatter, "unknown option {option}"),
            Self::DownloadCommand(source) if source.kind() == std::io::ErrorKind::NotFound => {
                write!(formatter, "curl was not found in PATH")
            }
            Self::DownloadCommand(source) => write!(formatter, "failed to run curl: {source}"),
            Self::DownloadFailed { status, message } => match (status, message.is_empty()) {
                (Some(code), false) => {
                    write!(formatter, "download failed with status {code}: {message}")
                }
                (Some(code), true) => write!(formatter, "download failed with status {code}"),
                (None, false) => write!(formatter, "download failed: {message}"),
                (None, true) => write!(formatter, "download failed"),
            },
            Self::PatchNotUtf8(_) => write!(formatter, "downloaded patch is not valid UTF-8"),
            Self::EmptyPatch => write!(formatter, "downloaded patch is empty"),
            Self::CreateOutputDir { path, source } => {
                write!(formatter, "failed to create output directory {}: {source}", path.display())
            }
            Self::WritePatch { path, source }
                if source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                write!(
                    formatter,
                    "refusing to overwrite existing patch file {}; pass --force to replace it",
                    path.display()
                )
            }
            Self::WritePatch { path, source } => {
                write!(formatter, "failed to write patch file {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for AppError {}

fn usage() -> &'static str {
    "Usage:
  patchsplit <owner/repo> <pr-number> [--out <dir>] [--force]
  patchsplit <owner> <repo> <pr-number> [--out <dir>] [--force]

Options:
  -o, --out <dir>   Output directory for split patch files [default: patches]
  -f, --force       Overwrite existing patch files
  -h, --help        Show this help
  -V, --version     Show version

Examples:
  patchsplit rust-lang/rust 12345
  patchsplit openai codex 42 -o pr-42-patches"
}
