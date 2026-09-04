use std::path::Path;

use crate::error::{ApiError, Result};
use crate::types::{ConfigOverwrite, Overwrite};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchAction {
    Open,
    ExtractHere,
    /// `dest_dir` is `None` when omitted (`--extract-to -- <archive>` or a single
    /// remaining path). The archive is never treated as the destination.
    ExtractTo {
        dest_dir: Option<String>,
    },
    IndexOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchIntent {
    pub action: LaunchAction,
    pub archives: Vec<String>,
    pub silent: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActionKind {
    Open,
    ExtractHere,
    ExtractTo,
    IndexOnly,
}

pub fn parse_argv<I, S>(args: I) -> Result<LaunchIntent>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    let mut silent = false;
    let mut action: Option<ActionKind> = None;
    let mut extract_to_omitted = false;
    let mut positionals = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--silent" => silent = true,
            "--extract-here" => set_action(&mut action, ActionKind::ExtractHere)?,
            "--index-only" => set_action(&mut action, ActionKind::IndexOnly)?,
            "--extract-to" => {
                set_action(&mut action, ActionKind::ExtractTo)?;
                match args.get(i + 1).map(String::as_str) {
                    None | Some("--") => {
                        extract_to_omitted = true;
                        if args.get(i + 1).map(String::as_str) == Some("--") {
                            i += 1;
                        }
                    }
                    Some(next) if next.starts_with("--") => {
                        extract_to_omitted = true;
                    }
                    Some(_) => {}
                }
            }
            "--" => {
                positionals.extend(args[i + 1..].iter().cloned());
                break;
            }
            s if s.starts_with('-') && s != "-" => {
                return Err(ApiError::internal(format!("unknown option '{s}'")));
            }
            s => positionals.push(s.to_string()),
        }
        i += 1;
    }

    let action = match action.unwrap_or(ActionKind::Open) {
        ActionKind::Open => LaunchAction::Open,
        ActionKind::ExtractHere => LaunchAction::ExtractHere,
        ActionKind::IndexOnly => LaunchAction::IndexOnly,
        ActionKind::ExtractTo => {
            // One remaining path is the archive. Treating it as destDir is the
            // Windows ExtractTo (`--extract-to -- "%1"`) bug class.
            if extract_to_omitted || positionals.len() <= 1 {
                LaunchAction::ExtractTo { dest_dir: None }
            } else {
                let dest_dir = positionals.remove(0);
                LaunchAction::ExtractTo {
                    dest_dir: Some(dest_dir),
                }
            }
        }
    };
    Ok(LaunchIntent {
        action,
        archives: positionals,
        silent,
    })
}

fn set_action(current: &mut Option<ActionKind>, next: ActionKind) -> Result<()> {
    match *current {
        None | Some(ActionKind::Open) => {
            *current = Some(next);
            Ok(())
        }
        Some(prev) if prev == next => Ok(()),
        Some(prev) => Err(ApiError::internal(format!(
            "conflicting actions {} and {}",
            action_kind_name(prev),
            action_kind_name(next)
        ))),
    }
}

fn action_kind_name(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Open => "open",
        ActionKind::ExtractHere => "extract-here",
        ActionKind::ExtractTo => "extract-to",
        ActionKind::IndexOnly => "index-only",
    }
}

/// `--silent` always maps to skip (never replace, never a hidden dialog).
/// Config `ask` also maps to skip on the headless path so native extract is never `'ask'`.
pub fn native_overwrite_for_launch(silent: bool, configured: ConfigOverwrite) -> Overwrite {
    if silent {
        Overwrite::Skip
    } else {
        match configured {
            ConfigOverwrite::Replace => Overwrite::Replace,
            ConfigOverwrite::Skip | ConfigOverwrite::Ask => Overwrite::Skip,
        }
    }
}

pub fn overwrite_wire(overwrite: Overwrite) -> &'static str {
    match overwrite {
        Overwrite::Skip => "skip",
        Overwrite::Replace => "replace",
    }
}

pub fn resolve_extract_dest(
    action: &LaunchAction,
    archive: &str,
    picked_dir: Option<&str>,
) -> Result<String> {
    match action {
        LaunchAction::ExtractHere => extract_here_dest(archive),
        LaunchAction::ExtractTo {
            dest_dir: Some(dir),
        } => {
            refuse_archive_as_dest(dir, archive)?;
            Ok(dir.clone())
        }
        LaunchAction::ExtractTo { dest_dir: None } => {
            let dest = picked_dir.ok_or_else(|| {
                ApiError::internal("extract-to destination omitted; folder picker required")
            })?;
            refuse_archive_as_dest(dest, archive)?;
            Ok(dest.to_string())
        }
        LaunchAction::Open | LaunchAction::IndexOnly => Err(ApiError::internal(
            "resolve_extract_dest called for a non-extract action",
        )),
    }
}

fn extract_here_dest(archive: &str) -> Result<String> {
    let path = Path::new(archive);
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_string_lossy().into_owned()),
        _ => Ok(".".to_string()),
    }
}

fn refuse_archive_as_dest(dest: &str, archive: &str) -> Result<()> {
    if same_path(dest, archive) {
        return Err(ApiError::internal(
            "extract-to dest omitted; archive path is not destDir",
        ));
    }
    Ok(())
}

fn same_path(a: &str, b: &str) -> bool {
    Path::new(a) == Path::new(b) || a.replace('\\', "/") == b.replace('\\', "/")
}
