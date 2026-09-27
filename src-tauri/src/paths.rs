//! Path helpers. Library roots are canonicalized once when they are chosen or
//! loaded; every photo path is built by joining onto a root, so containment
//! checks here are purely lexical and never touch the filesystem.

use std::path::{Component, Path, PathBuf};

/// Resolves symlinks and `..` against the filesystem. Only for paths that come
/// from the user (folder pickers, settings.json); falls back to a lexical clean
/// when the path does not exist (e.g. an unplugged drive).
pub fn canonicalize_path(path: &str) -> String {
    match PathBuf::from(path).canonicalize() {
        Ok(canonical) => path_to_string(&canonical),
        Err(_) => clean_path(path),
    }
}

/// Lexically normalizes separators, `.` components, and trailing slashes.
/// `..` components are preserved so callers can reject them.
pub fn clean_path(path: &str) -> String {
    let stripped = strip_windows_verbatim_prefix(path);
    let cleaned: PathBuf = Path::new(&stripped).components().collect();
    path_to_string(&cleaned)
}

pub fn path_to_string(path: &Path) -> String {
    strip_windows_verbatim_prefix(&path.to_string_lossy())
}

fn strip_windows_verbatim_prefix(path: &str) -> String {
    if let Some(path) = path.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{path}")
    } else if let Some(path) = path.strip_prefix("\\\\?\\") {
        path.to_string()
    } else if let Some(path) = path.strip_prefix("\\??\\") {
        path.to_string()
    } else {
        path.to_string()
    }
}

fn comparable(path: &str) -> String {
    let cleaned = clean_path(path);
    if cfg!(target_os = "windows") {
        cleaned.to_lowercase()
    } else {
        cleaned
    }
}

fn has_parent_component(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
}

/// True when `path` is `root` itself or somewhere beneath it. Paths containing
/// `..` are never considered inside, so they cannot escape a library root.
pub fn is_inside_directory(root: &str, path: &str) -> bool {
    if has_parent_component(path) {
        return false;
    }
    let root = comparable(root);
    let path = comparable(path);
    path == root || Path::new(&path).starts_with(Path::new(&root))
}

pub fn same_directory(left: &str, right: &str) -> bool {
    comparable(left) == comparable(right)
}

pub fn is_inside_any_directory(roots: &[String], path: &str) -> bool {
    roots.iter().any(|root| is_inside_directory(root, path))
}

/// Precomputed containment test for filtering many paths against one directory.
pub struct DirectoryMatcher {
    root: PathBuf,
}

impl DirectoryMatcher {
    pub fn new(root: &str) -> Self {
        Self {
            root: PathBuf::from(comparable(root)),
        }
    }

    pub fn contains(&self, path: &str) -> bool {
        if cfg!(target_os = "windows") {
            Path::new(&path.to_lowercase()).starts_with(&self.root)
        } else {
            Path::new(path).starts_with(&self.root)
        }
    }

    /// First path component of `path` below this directory, if `path` is strictly inside it.
    pub fn child_component<'a>(&self, path: &'a str) -> Option<&'a str> {
        let root_components = self.root.components().count();
        let mut components = Path::new(path).components().skip(root_components);
        match components.next()? {
            Component::Normal(name) => name.to_str(),
            _ => None,
        }
    }
}

pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string()
}

pub fn parent_directory(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(path_to_string)
        .unwrap_or_default()
}

pub fn extension_lowercase(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase()
}

pub fn directory_display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(parts: &[&str]) -> String {
        let mut path = PathBuf::new();
        for part in parts {
            path.push(part);
        }
        path_to_string(&path)
    }

    #[test]
    fn inside_directory_matches_descendants_and_self() {
        let root = p(&["library", "Photos"]);
        assert!(is_inside_directory(&root, &root));
        assert!(is_inside_directory(
            &root,
            &p(&["library", "Photos", "2024", "a.jpg"])
        ));
        assert!(!is_inside_directory(
            &root,
            &p(&["library", "Photos Extra", "a.jpg"])
        ));
        assert!(!is_inside_directory(&root, &p(&["library"])));
    }

    #[test]
    fn parent_components_cannot_escape_a_root() {
        let root = p(&["library", "Photos"]);
        let escape = p(&["library", "Photos", "..", "secret.jpg"]);
        assert!(!is_inside_directory(&root, &escape));
    }

    #[test]
    fn trailing_separators_are_ignored() {
        let root = p(&["library", "Photos"]);
        let with_slash = format!("{root}{}", std::path::MAIN_SEPARATOR);
        assert!(same_directory(&root, &with_slash));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_comparisons_ignore_case_and_verbatim_prefixes() {
        assert!(same_directory(r"C:\Photos", r"c:\photos"));
        assert!(is_inside_directory(r"\\?\C:\Photos", r"C:\PHOTOS\a.jpg"));
        assert!(DirectoryMatcher::new(r"C:\Photos").contains(r"c:\photos\trip\a.jpg"));
    }

    #[test]
    fn matcher_reports_the_child_component() {
        let root = p(&["library", "Photos"]);
        let matcher = DirectoryMatcher::new(&root);
        let photo = p(&["library", "Photos", "Trip", "Day 1", "a.jpg"]);
        assert!(matcher.contains(&photo));
        assert_eq!(matcher.child_component(&photo), Some("Trip"));
    }
}
