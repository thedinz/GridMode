use crate::model::Settings;
use crate::paths::{
    canonicalize_path, clean_path, is_inside_any_directory, is_inside_directory, same_directory,
};
use std::{fs, path::Path, path::PathBuf};

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

/// Loads settings and canonicalizes their paths once for the session.
pub fn read_settings(data_dir: &Path) -> Settings {
    let Ok(text) = fs::read_to_string(settings_path(data_dir)) else {
        return Settings::default();
    };
    let cleaned = text.trim_start_matches('\u{feff}');
    let mut settings = serde_json::from_str::<Settings>(cleaned).unwrap_or_default();
    settings.photo_directory = settings
        .photo_directory
        .map(|path| canonicalize_path(&path));
    settings.photo_directories = settings
        .photo_directories
        .iter()
        .map(|path| canonicalize_path(path))
        .collect();
    settings.excluded_directories = settings
        .excluded_directories
        .iter()
        .map(|path| canonicalize_path(path))
        .collect();
    normalize_settings_in_place(&mut settings);
    settings
}

pub fn write_settings(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let mut normalized = settings.clone();
    normalize_settings_in_place(&mut normalized);
    let text = serde_json::to_string_pretty(&normalized).map_err(|error| error.to_string())?;
    write_atomically(&settings_path(data_dir), text.as_bytes())
}

pub fn write_atomically(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = target.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    fs::rename(&temp, target).map_err(|error| error.to_string())
}

pub fn normalize_settings_in_place(settings: &mut Settings) {
    let photo_directories = get_photo_directories(settings);
    settings.photo_directory = photo_directories.first().cloned();
    settings.excluded_directories =
        normalize_excluded_directories(&photo_directories, &settings.excluded_directories);
    settings.photo_directories = photo_directories;
}

/// The primary directory followed by any additional ones, with duplicates and
/// directories nested inside another root removed.
pub fn get_photo_directories(settings: &Settings) -> Vec<String> {
    let mut dirs = Vec::new();
    if let Some(photo_directory) = &settings.photo_directory {
        dirs.push(photo_directory.clone());
    }
    dirs.extend(settings.photo_directories.iter().cloned());
    normalize_photo_directories(&dirs)
}

pub fn normalize_photo_directories(directories: &[String]) -> Vec<String> {
    let mut compacted: Vec<String> = Vec::new();
    for item in directories
        .iter()
        .filter(|item| !item.trim().is_empty())
        .map(|item| clean_path(item))
    {
        if compacted
            .iter()
            .any(|existing| is_inside_directory(existing, &item))
        {
            continue;
        }
        compacted.retain(|existing| !is_inside_directory(&item, existing));
        compacted.push(item);
    }
    compacted
}

pub fn normalize_excluded_directories(root_dirs: &[String], directories: &[String]) -> Vec<String> {
    if root_dirs.is_empty() {
        return Vec::new();
    }

    let mut normalized: Vec<String> = directories
        .iter()
        .filter(|item| !item.trim().is_empty())
        .map(|item| clean_path(item))
        .filter(|item| is_valid_excluded_directory(root_dirs, item))
        .collect();
    normalized.sort_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)));

    let mut compacted: Vec<String> = Vec::new();
    for item in normalized {
        if !compacted
            .iter()
            .any(|existing| is_inside_directory(existing, &item))
        {
            compacted.push(item);
        }
    }
    compacted
}

pub fn add_excluded_directory(
    root_dirs: &[String],
    existing: &[String],
    candidate: &str,
) -> Vec<String> {
    let mut next = existing.to_vec();
    next.push(candidate.to_string());
    normalize_excluded_directories(root_dirs, &next)
}

pub fn remove_excluded_directory(existing: &[String], candidate: &str) -> Vec<String> {
    existing
        .iter()
        .filter(|item| !same_directory(item, candidate))
        .cloned()
        .collect()
}

pub fn is_path_excluded(path: &str, excluded_directories: &[String]) -> bool {
    excluded_directories
        .iter()
        .any(|excluded| is_inside_directory(excluded, path))
}

pub fn is_valid_excluded_directory(root_dirs: &[String], candidate: &str) -> bool {
    is_inside_any_directory(root_dirs, candidate)
        && !is_configured_root_directory(root_dirs, candidate)
}

pub fn is_configured_root_directory(root_dirs: &[String], candidate: &str) -> bool {
    root_dirs.iter().any(|root| same_directory(root, candidate))
}

pub fn format_root_summary(root_dirs: &[String]) -> Option<String> {
    match root_dirs.len() {
        0 => None,
        1 => root_dirs.first().cloned(),
        count => Some(format!("{count} photo locations")),
    }
}

pub fn root_summary_label(root_dirs: &[String]) -> String {
    format_root_summary(root_dirs).unwrap_or_else(|| "Photo library".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(parts: &[&str]) -> String {
        let mut path = PathBuf::new();
        for part in parts {
            path.push(part);
        }
        crate::paths::path_to_string(&path)
    }

    #[test]
    fn nested_photo_directories_collapse_into_their_parent() {
        let dirs = normalize_photo_directories(&[
            p(&["lib", "Photos", "2024"]),
            p(&["lib", "Photos"]),
            p(&["lib", "Other"]),
            p(&["lib", "Photos"]),
        ]);
        assert_eq!(dirs, vec![p(&["lib", "Photos"]), p(&["lib", "Other"])]);
    }

    #[test]
    fn exclusions_must_sit_strictly_inside_a_root() {
        let roots = vec![p(&["lib", "Photos"])];
        let excluded = normalize_excluded_directories(
            &roots,
            &[
                p(&["lib", "Photos"]),
                p(&["lib", "Elsewhere"]),
                p(&["lib", "Photos", "Screenshots"]),
                p(&["lib", "Photos", "Screenshots", "Old"]),
            ],
        );
        assert_eq!(excluded, vec![p(&["lib", "Photos", "Screenshots"])]);
        assert!(is_path_excluded(
            &p(&["lib", "Photos", "Screenshots", "a.png"]),
            &excluded
        ));
        assert!(!is_path_excluded(
            &p(&["lib", "Photos", "a.png"]),
            &excluded
        ));
    }

    #[test]
    fn removing_an_exclusion_matches_equivalent_paths() {
        let existing = vec![p(&["lib", "Photos", "Screenshots"])];
        let with_slash = format!(
            "{}{}",
            p(&["lib", "Photos", "Screenshots"]),
            std::path::MAIN_SEPARATOR
        );
        assert!(remove_excluded_directory(&existing, &with_slash).is_empty());
    }
}
