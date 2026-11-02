use crate::sanitize::clean;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Codebase {
    pub name: String,
    pub path: PathBuf,
    pub exists: bool,
    pub is_git: bool,
}

pub fn list_codebases(workspace: &Path) -> Vec<Codebase> {
    let root = workspace.join("codebase-symlinks");
    let meta = match std::fs::symlink_metadata(&root) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    if !meta.is_dir() || meta.is_symlink() {
        return Vec::new();
    }

    let read_dir = match std::fs::read_dir(&root) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<std::fs::DirEntry> = read_dir.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    let mut found = Vec::new();
    for entry in entries {
        let entry_meta = match entry.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !entry_meta.is_symlink() && !entry_meta.is_dir() {
            continue;
        }

        let entry_path = entry.path();
        let target = if entry_meta.is_symlink() {
            match std::fs::canonicalize(&entry_path) {
                Ok(canon) => canon,
                Err(_) => match std::fs::read_link(&entry_path) {
                    Ok(link) => {
                        if link.is_relative() {
                            normalize_resolved_path(&root.join(link))
                        } else {
                            normalize_resolved_path(&link)
                        }
                    }
                    Err(_) => entry_path.clone(),
                },
            }
        } else {
            entry_path.clone()
        };

        let exists = target.is_dir();
        let is_git = exists && target.join(".git").exists();
        let name = clean(&entry.file_name().to_string_lossy());

        found.push(Codebase {
            name,
            path: target,
            exists,
            is_git,
        });
    }

    found
}

fn normalize_resolved_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}
