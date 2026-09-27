//! The photo library: scanning folders, the persistent index, and queries.

use crate::metadata::read_photo_metadata;
use crate::model::{
    DateSource, DirectoryBreadcrumb, DirectoryPayload, FolderSummary, FoldersPayload,
    LibrarySummary, MonthPayload, MonthSummary, PhotoAsset, ScanPhase, ScanProgress, SearchPayload,
    SearchQuery, YearPayload, YearSummary,
};
use crate::paths::{
    clean_path, directory_display_name, extension_lowercase, file_name, is_inside_directory,
    parent_directory, path_to_string, same_directory, DirectoryMatcher,
};
use crate::render::{is_supported_photo_path, PhotoRenderVariant};
use crate::settings::{
    format_root_summary, is_configured_root_directory, is_path_excluded,
    normalize_excluded_directories, normalize_photo_directories, write_atomically,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

/// Bumped to 2 when EXIF dates, camera, and GPS were added: older indexes are
/// rebuilt so every photo gets its real capture date.
const LIBRARY_INDEX_VERSION: u32 = 2;
const SUMMARY_SAMPLE_SIZE: usize = 20;
const MAX_WARNINGS: usize = 20;
pub const HOME_PHOTO_COUNT: usize = 260;
pub const ON_THIS_DAY_LIMIT: usize = 24;
pub const SEARCH_RESULT_LIMIT: usize = 3000;

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhotoFileSnapshot {
    pub path: String,
    pub size: u64,
    pub mtime_ms: i64,
}

/// An immutable snapshot of the indexed library. Cloning is cheap: the photo
/// list and lookup tables are shared, so readers never hold the state lock
/// while filtering and scans can build a replacement in the background.
#[derive(Clone, Default)]
pub struct Library {
    pub photos: Arc<Vec<PhotoAsset>>,
    by_path: Arc<HashMap<String, usize>>,
    stats: Arc<HashMap<String, PhotoFileSnapshot>>,
    pub summary: LibrarySummary,
    pub has_index: bool,
}

impl Library {
    pub fn empty(root_dirs: &[String]) -> Self {
        Self {
            summary: empty_summary(root_dirs),
            ..Self::default()
        }
    }

    fn build(
        root_dirs: &[String],
        mut photos: Vec<PhotoAsset>,
        stats: HashMap<String, PhotoFileSnapshot>,
        warnings: &[String],
        last_scan_at: Option<String>,
    ) -> Self {
        photos.sort_by(|left, right| right.captured_at.cmp(&left.captured_at));
        let by_path = photos
            .iter()
            .enumerate()
            .map(|(index, photo)| (photo.path.clone(), index))
            .collect();
        let summary = LibrarySummary {
            root_dir: format_root_summary(root_dirs),
            root_dirs: root_dirs.to_vec(),
            photo_count: photos.len(),
            years: group_years(&photos),
            last_scan_at: Some(last_scan_at.unwrap_or_else(now_iso)),
            warnings: warnings.iter().take(MAX_WARNINGS).cloned().collect(),
        };
        Self {
            photos: Arc::new(photos),
            by_path: Arc::new(by_path),
            stats: Arc::new(stats),
            summary,
            has_index: true,
        }
    }

    pub fn photo(&self, path: &str) -> Option<&PhotoAsset> {
        self.by_path
            .get(path)
            .and_then(|index| self.photos.get(*index))
    }
}

fn empty_summary(root_dirs: &[String]) -> LibrarySummary {
    LibrarySummary {
        root_dir: format_root_summary(root_dirs),
        root_dirs: root_dirs.to_vec(),
        ..LibrarySummary::default()
    }
}

pub fn now_iso() -> String {
    DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn month_name(month: u32) -> String {
    MONTH_NAMES
        .get(month.saturating_sub(1) as usize)
        .unwrap_or(&"Unknown")
        .to_string()
}

// ---------------------------------------------------------------------------
// Progress reporting

/// Rate-limited `scan:progress` emitter. Quiet emitters (background rescans
/// triggered by the folder watcher) report nothing.
pub struct ProgressEmitter {
    app: Option<AppHandle>,
    root_dir: String,
    last_sent_at: Mutex<Instant>,
    quiet: bool,
}

impl ProgressEmitter {
    pub fn new(app: Option<AppHandle>, root_dir: String, quiet: bool) -> Self {
        Self {
            app,
            root_dir,
            last_sent_at: Mutex::new(Instant::now()),
            quiet,
        }
    }

    pub fn send(&self, mut progress: ScanProgress, immediate: bool) {
        let Some(app) = self.app.as_ref().filter(|_| !self.quiet) else {
            return;
        };
        {
            let mut last_sent_at = self
                .last_sent_at
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if !immediate && last_sent_at.elapsed().as_millis() < 250 {
                return;
            }
            *last_sent_at = Instant::now();
        }
        progress.root_dir = Some(self.root_dir.clone());
        let _ = app.emit("scan:progress", progress);
    }
}

// ---------------------------------------------------------------------------
// Scanning

pub struct ScanOutcome {
    pub library: Library,
    pub changed: usize,
    pub removed: usize,
}

impl ScanOutcome {
    pub fn has_changes(&self) -> bool {
        self.changed > 0 || self.removed > 0
    }
}

#[derive(Debug, Default)]
struct ScanPlan {
    reused: Vec<PhotoAsset>,
    reused_stats: Vec<PhotoFileSnapshot>,
    to_index: Vec<PhotoFileSnapshot>,
    removed: usize,
}

/// Decides which files can reuse their indexed metadata and which must be read again.
fn plan_scan(previous: &Library, found: Vec<PhotoFileSnapshot>, force: bool) -> ScanPlan {
    let mut plan = ScanPlan::default();
    let mut found_paths = HashSet::with_capacity(found.len());

    for file in found {
        found_paths.insert(file.path.clone());
        let cached = (!force)
            .then(|| {
                previous
                    .photo(&file.path)
                    .zip(previous.stats.get(&file.path))
            })
            .flatten();
        match cached {
            Some((photo, stat)) if stat.size == file.size && stat.mtime_ms == file.mtime_ms => {
                plan.reused.push(with_urls(photo.clone(), &file));
                plan.reused_stats.push(file);
            }
            _ => plan.to_index.push(file),
        }
    }

    if !force {
        plan.removed = previous
            .photos
            .iter()
            .filter(|photo| !found_paths.contains(&photo.path))
            .count();
    }
    plan
}

/// `ignored` lists directories never indexed, such as GridMode's own data
/// folder, which matters when a library root contains the thumbnail cache.
pub fn scan_library(
    root_dirs: &[String],
    excluded_directories: &[String],
    ignored: &[String],
    previous: &Library,
    force: bool,
    emitter: &ProgressEmitter,
) -> ScanOutcome {
    let excluded = normalize_excluded_directories(root_dirs, excluded_directories);
    let mut warnings = Vec::new();

    let mut discovering = ScanProgress::new(ScanPhase::Discovering);
    discovering.folders_scanned = Some(0);
    discovering.photos_found = Some(0);
    discovering.folders_excluded = Some(0);
    discovering.message = Some(if previous.has_index && !force {
        "Checking folders for changes".to_string()
    } else {
        "Finding photos".to_string()
    });
    emitter.send(discovering, true);

    let mut skipped = excluded.clone();
    skipped.extend(ignored.iter().cloned());
    let files = find_photo_files(root_dirs, &skipped, &mut warnings, emitter);
    let found_count = files.len();
    let plan = plan_scan(previous, files, force);
    let reused = plan.reused.len();
    let changed = plan.to_index.len();
    let removed = plan.removed;

    let metadata_progress = |processed: usize, current_path: Option<String>| {
        let mut progress = ScanProgress::new(ScanPhase::ReadingMetadata);
        progress.photos_found = Some(found_count);
        progress.photos_processed = Some(processed);
        progress.photos_reused = Some(reused);
        progress.photos_changed = Some(changed);
        progress.photos_removed = Some(removed);
        progress.total_photos = Some(changed);
        progress.current_path = current_path;
        progress.message = Some(if changed == 0 {
            "No photo metadata changes found".to_string()
        } else {
            "Reading metadata for new and changed photos".to_string()
        });
        progress
    };
    emitter.send(metadata_progress(0, None), true);

    let indexed = read_metadata_parallel(&plan.to_index, |processed, path| {
        emitter.send(metadata_progress(processed, Some(path.to_string())), false);
    });

    let mut photos = plan.reused;
    let mut stats: HashMap<String, PhotoFileSnapshot> = plan
        .reused_stats
        .into_iter()
        .map(|stat| (stat.path.clone(), stat))
        .collect();
    for (photo, stat) in indexed {
        stats.insert(stat.path.clone(), stat);
        photos.push(photo);
    }

    let library = Library::build(root_dirs, photos, stats, &warnings, None);

    let mut complete = ScanProgress::new(ScanPhase::Complete);
    complete.photos_found = Some(found_count);
    complete.photos_processed = Some(changed);
    complete.photos_reused = Some(reused);
    complete.photos_changed = Some(changed);
    complete.photos_removed = Some(removed);
    complete.total_photos = Some(changed);
    complete.message = Some(format_scan_complete_message(
        library.photos.len(),
        reused,
        changed,
        removed,
    ));
    emitter.send(complete, true);

    ScanOutcome {
        library,
        changed,
        removed,
    }
}

/// Reads EXIF for new and changed files on several threads; the work is
/// dominated by disk latency, so it overlaps well.
fn read_metadata_parallel(
    files: &[PhotoFileSnapshot],
    report: impl Fn(usize, &str) + Sync,
) -> Vec<(PhotoAsset, PhotoFileSnapshot)> {
    if files.is_empty() {
        return Vec::new();
    }
    let workers = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(2)
        .clamp(2, 8)
        .min(files.len());
    let next_index = AtomicUsize::new(0);
    let processed = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut local = Vec::new();
                    loop {
                        let index = next_index.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = files.get(index) else {
                            break;
                        };
                        local.push((build_photo_asset(file), file.clone()));
                        let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                        report(done, &file.path);
                    }
                    local
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    })
}

fn find_photo_files(
    root_dirs: &[String],
    excluded_directories: &[String],
    warnings: &mut Vec<String>,
    emitter: &ProgressEmitter,
) -> Vec<PhotoFileSnapshot> {
    let mut found = Vec::new();
    let mut pending: Vec<String> = root_dirs.iter().rev().cloned().collect();
    let mut folders_scanned = 0usize;
    let mut folders_excluded = 0usize;

    let progress = |folders_scanned, found: usize, folders_excluded, current: String| {
        let mut progress = ScanProgress::new(ScanPhase::Discovering);
        progress.folders_scanned = Some(folders_scanned);
        progress.photos_found = Some(found);
        progress.folders_excluded = Some(folders_excluded);
        progress.current_path = Some(current);
        progress.message = Some("Finding photos".to_string());
        progress
    };

    while let Some(current) = pending.pop() {
        if !is_configured_root_directory(root_dirs, &current)
            && is_path_excluded(&current, excluded_directories)
        {
            folders_excluded += 1;
            continue;
        }

        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) => {
                warnings.push(format!("{current}: {error}"));
                continue;
            }
        };
        folders_scanned += 1;

        for entry in entries.flatten() {
            let path = entry.path();
            let path_string = path_to_string(&path);
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    warnings.push(format!("{path_string}: {error}"));
                    continue;
                }
            };

            if file_type.is_dir() {
                if is_path_excluded(&path_string, excluded_directories) {
                    folders_excluded += 1;
                } else {
                    pending.push(path_string);
                }
            } else if file_type.is_file() && is_supported_photo_path(&path) {
                match entry.metadata() {
                    Ok(metadata) => found.push(PhotoFileSnapshot {
                        path: path_string,
                        size: metadata.len(),
                        mtime_ms: modified_ms(&metadata),
                    }),
                    Err(error) => warnings.push(format!("{path_string}: {error}")),
                }
            }
        }

        emitter.send(
            progress(folders_scanned, found.len(), folders_excluded, current),
            false,
        );
    }

    found
}

pub fn modified_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Local wall-clock time of a file's modification, used when EXIF has no date.
fn local_time_from_mtime_ms(mtime_ms: i64) -> NaiveDateTime {
    DateTime::<Utc>::from_timestamp_millis(mtime_ms)
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local)
        .naive_local()
}

fn format_local_time(value: NaiveDateTime) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
}

fn build_photo_asset(file: &PhotoFileSnapshot) -> PhotoAsset {
    let metadata = read_photo_metadata(&file.path);
    let (captured, date_source) = match metadata.captured {
        Some(captured) => (captured, DateSource::Exif),
        None => (local_time_from_mtime_ms(file.mtime_ms), DateSource::File),
    };

    let photo = PhotoAsset {
        id: photo_id(&file.path),
        name: file_name(&file.path),
        path: file.path.clone(),
        directory: parent_directory(&file.path),
        extension: extension_lowercase(&file.path),
        size: file.size,
        cache_key: String::new(),
        url: String::new(),
        thumbnail_url: String::new(),
        captured_at: format_local_time(captured),
        date_source,
        year: captured.year(),
        month: captured.month(),
        month_name: month_name(captured.month()),
        camera: metadata.camera,
        location: metadata.location,
        width: metadata.width,
        height: metadata.height,
    };
    with_urls(photo, file)
}

fn photo_id(file_path: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(file_path.as_bytes());
    format!("{:x}", hasher.finalize())
        .chars()
        .take(16)
        .collect()
}

/// Fills in the cache key and protocol URLs, which are derived from the file on disk.
fn with_urls(mut photo: PhotoAsset, file: &PhotoFileSnapshot) -> PhotoAsset {
    photo.size = file.size;
    photo.cache_key = format!("{}-{}", file.size, file.mtime_ms);
    photo.url = make_photo_url(&photo.path, PhotoRenderVariant::Display, &photo.cache_key);
    photo.thumbnail_url = make_photo_url(&photo.path, PhotoRenderVariant::Thumb, &photo.cache_key);
    photo
}

/// Builds the same URL Tauri's `convertFileSrc` would for the `gridmode-photo` scheme.
pub fn make_photo_url(file_path: &str, variant: PhotoRenderVariant, cache_key: &str) -> String {
    let origin = if cfg!(any(target_os = "windows", target_os = "android")) {
        "http://gridmode-photo.localhost"
    } else {
        "gridmode-photo://localhost"
    };
    let token = URL_SAFE_NO_PAD.encode(file_path.as_bytes());
    format!("{origin}/{}/{token}?v={cache_key}", variant.token())
}

pub fn format_scan_complete_message(
    total: usize,
    reused: usize,
    changed: usize,
    removed: usize,
) -> String {
    let mut parts = if changed == 0 && removed == 0 && reused > 0 {
        vec![format!(
            "Library up to date - reused {reused} cached photos"
        )]
    } else {
        vec![format!("Indexed {total} photos")]
    };
    if reused > 0 && (changed > 0 || removed > 0) {
        parts.push(format!("{reused} cached"));
    }
    if changed > 0 {
        parts.push(format!("{changed} new or changed"));
    }
    if removed > 0 {
        parts.push(format!("{removed} removed"));
    }
    parts.join(" - ")
}

pub fn merge_thumbnail_warnings(summary: &mut LibrarySummary, warnings: Vec<String>) {
    summary
        .warnings
        .retain(|warning| !warning.starts_with("Thumbnail "));
    if warnings.is_empty() {
        return;
    }
    let mut merged = warnings;
    merged.append(&mut summary.warnings);
    merged.truncate(MAX_WARNINGS);
    summary.warnings = merged;
}

// ---------------------------------------------------------------------------
// Persistent index

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndexFile {
    version: u32,
    #[serde(default)]
    root_dirs: Vec<String>,
    #[serde(default)]
    excluded_directories: Vec<String>,
    scanned_at: String,
    #[serde(default)]
    warnings: Vec<String>,
    photos: Vec<LibraryIndexEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndexEntry {
    path: String,
    size: u64,
    mtime_ms: i64,
    photo: PhotoAsset,
}

fn library_index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("library-index.json")
}

/// Loads the index written by the previous session, if it still matches the
/// configured roots and exclusions.
pub fn load_library(data_dir: &Path, root_dirs: &[String], excluded: &[String]) -> Library {
    if root_dirs.is_empty() {
        return Library::empty(root_dirs);
    }
    let index = fs::read_to_string(library_index_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str::<LibraryIndexFile>(&text).ok());
    let Some(index) = index.filter(|index| is_usable_library_index(index, root_dirs, excluded))
    else {
        return Library::empty(root_dirs);
    };

    let mut photos = Vec::with_capacity(index.photos.len());
    let mut stats = HashMap::with_capacity(index.photos.len());
    for entry in index.photos {
        if !is_supported_photo_path(Path::new(&entry.path))
            || !root_dirs
                .iter()
                .any(|root| is_inside_directory(root, &entry.path))
        {
            continue;
        }
        let snapshot = PhotoFileSnapshot {
            path: entry.path,
            size: entry.size,
            mtime_ms: entry.mtime_ms,
        };
        photos.push(with_urls(entry.photo, &snapshot));
        stats.insert(snapshot.path.clone(), snapshot);
    }
    Library::build(
        root_dirs,
        photos,
        stats,
        &index.warnings,
        Some(index.scanned_at),
    )
}

fn is_usable_library_index(
    index: &LibraryIndexFile,
    root_dirs: &[String],
    excluded: &[String],
) -> bool {
    let index_roots = normalize_photo_directories(&index.root_dirs);
    index.version == LIBRARY_INDEX_VERSION
        && same_directories(&index_roots, root_dirs)
        && same_directories(
            &normalize_excluded_directories(root_dirs, &index.excluded_directories),
            &normalize_excluded_directories(root_dirs, excluded),
        )
}

fn same_directories(left: &[String], right: &[String]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| same_directory(left, right))
}

pub fn write_library_index(
    data_dir: &Path,
    library: &Library,
    root_dirs: &[String],
    excluded: &[String],
) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let entries: Vec<LibraryIndexEntry> = library
        .photos
        .iter()
        .filter_map(|photo| {
            let stat = library.stats.get(&photo.path)?;
            let mut stored = photo.clone();
            stored.url.clear();
            stored.thumbnail_url.clear();
            Some(LibraryIndexEntry {
                path: photo.path.clone(),
                size: stat.size,
                mtime_ms: stat.mtime_ms,
                photo: stored,
            })
        })
        .collect();
    let index = LibraryIndexFile {
        version: LIBRARY_INDEX_VERSION,
        root_dirs: root_dirs.to_vec(),
        excluded_directories: normalize_excluded_directories(root_dirs, excluded),
        scanned_at: library.summary.last_scan_at.clone().unwrap_or_else(now_iso),
        warnings: library.summary.warnings.clone(),
        photos: entries,
    };
    let text = serde_json::to_string(&index).map_err(|error| error.to_string())?;
    write_atomically(&library_index_path(data_dir), text.as_bytes())
}

// ---------------------------------------------------------------------------
// Grouping and queries

fn group_years(photos: &[PhotoAsset]) -> Vec<YearSummary> {
    let mut groups: BTreeMap<i32, Vec<&PhotoAsset>> = BTreeMap::new();
    for photo in photos {
        groups.entry(photo.year).or_default().push(photo);
    }
    groups
        .into_iter()
        .rev()
        .map(|(year, year_photos)| YearSummary {
            year,
            count: year_photos.len(),
            sample: sample_photos(&year_photos, SUMMARY_SAMPLE_SIZE),
        })
        .collect()
}

fn sample_photos(photos: &[&PhotoAsset], count: usize) -> Vec<PhotoAsset> {
    let step = photos.len().div_ceil(count).max(1);
    photos
        .iter()
        .step_by(step)
        .take(count)
        .map(|photo| (*photo).clone())
        .collect()
}

pub fn year_payload(photos: &[PhotoAsset], year: i32) -> YearPayload {
    let mut groups: BTreeMap<u32, Vec<&PhotoAsset>> = BTreeMap::new();
    for photo in photos.iter().filter(|photo| photo.year == year) {
        groups.entry(photo.month).or_default().push(photo);
    }
    YearPayload {
        year,
        months: groups
            .into_iter()
            .rev()
            .map(|(month, month_photos)| MonthSummary {
                year,
                month,
                month_name: month_name(month),
                count: month_photos.len(),
                sample: sample_photos(&month_photos, SUMMARY_SAMPLE_SIZE),
            })
            .collect(),
    }
}

pub fn month_payload(photos: &[PhotoAsset], year: i32, month: u32) -> MonthPayload {
    MonthPayload {
        year,
        month,
        month_name: month_name(month),
        photos: photos
            .iter()
            .filter(|photo| photo.year == year && photo.month == month)
            .cloned()
            .collect(),
    }
}

/// A random selection, drawn with a partial Fisher-Yates shuffle.
pub fn random_photos(photos: &[PhotoAsset], count: usize, seed: u64) -> Vec<PhotoAsset> {
    let mut state = seed | 1;
    let mut next = move || {
        // xorshift64*: plenty for picking a grid, and needs no extra dependency.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let mut indices: Vec<usize> = (0..photos.len()).collect();
    let take = count.min(photos.len());
    for position in 0..take {
        let remaining = (indices.len() - position) as u64;
        let pick = position + (next() % remaining) as usize;
        indices.swap(position, pick);
    }
    indices[..take]
        .iter()
        .map(|index| photos[*index].clone())
        .collect()
}

pub fn time_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
}

/// Photos taken on `today`'s month and day in earlier years, newest year first.
pub fn on_this_day(photos: &[PhotoAsset], today: NaiveDate, limit: usize) -> Vec<PhotoAsset> {
    let month_day = format!("-{:02}-{:02}T", today.month(), today.day());
    photos
        .iter()
        .filter(|photo| {
            photo.year < today.year() && photo.captured_at.get(4..11) == Some(&month_day)
        })
        .take(limit)
        .cloned()
        .collect()
}

pub fn today_local() -> NaiveDate {
    Local::now().date_naive()
}

/// Breadcrumbs from the containing library root down to `directory_path`.
pub fn directory_breadcrumbs(
    root_dirs: &[String],
    directory_path: &str,
) -> Option<Vec<DirectoryBreadcrumb>> {
    let directory_path = clean_path(directory_path);
    let root_dir = root_dirs
        .iter()
        .find(|root_dir| is_inside_directory(root_dir, &directory_path))?;
    let root_component_count = Path::new(root_dir).components().count();

    let mut breadcrumbs = vec![DirectoryBreadcrumb {
        name: directory_display_name(root_dir),
        path: root_dir.clone(),
    }];
    let mut current_path = PathBuf::from(root_dir);
    for component in Path::new(&directory_path)
        .components()
        .skip(root_component_count)
    {
        let Component::Normal(name) = component else {
            continue;
        };
        current_path.push(name);
        breadcrumbs.push(DirectoryBreadcrumb {
            name: name.to_string_lossy().to_string(),
            path: path_to_string(&current_path),
        });
    }
    Some(breadcrumbs)
}

fn folder_summary(name: String, path: String, photos: &[&PhotoAsset]) -> FolderSummary {
    FolderSummary {
        name,
        path,
        photo_count: photos.len(),
        cover: photos.first().map(|photo| (*photo).clone()),
    }
}

pub fn folders_payload(photos: &[PhotoAsset], root_dirs: &[String]) -> FoldersPayload {
    FoldersPayload {
        roots: root_dirs
            .iter()
            .map(|root| {
                let matcher = DirectoryMatcher::new(root);
                let inside: Vec<&PhotoAsset> = photos
                    .iter()
                    .filter(|photo| matcher.contains(&photo.path))
                    .collect();
                folder_summary(directory_display_name(root), root.clone(), &inside)
            })
            .collect(),
    }
}

pub fn directory_payload(
    photos: &[PhotoAsset],
    root_dirs: &[String],
    excluded: &[String],
    directory_path: &str,
) -> Result<DirectoryPayload, String> {
    let directory_path = clean_path(directory_path);
    if is_path_excluded(&directory_path, excluded) {
        return Err("Directory is excluded from the photo library.".to_string());
    }
    let breadcrumbs = directory_breadcrumbs(root_dirs, &directory_path)
        .ok_or_else(|| "Directory is outside the configured photo library.".to_string())?;

    let matcher = DirectoryMatcher::new(&directory_path);
    let inside: Vec<PhotoAsset> = photos
        .iter()
        .filter(|photo| matcher.contains(&photo.path))
        .cloned()
        .collect();

    // Group by the first path component below this directory; keys are
    // lowercased so the listing sorts naturally regardless of case.
    let mut children: BTreeMap<String, (String, Vec<&PhotoAsset>)> = BTreeMap::new();
    for photo in &inside {
        let Some(child) = matcher.child_component(&photo.directory) else {
            continue;
        };
        children
            .entry(child.to_lowercase())
            .or_insert_with(|| (child.to_string(), Vec::new()))
            .1
            .push(photo);
    }
    let subfolders = children
        .into_values()
        .map(|(name, child_photos)| {
            let path = path_to_string(&Path::new(&directory_path).join(&name));
            folder_summary(name, path, &child_photos)
        })
        .collect();

    let name = breadcrumbs
        .last()
        .map(|breadcrumb| breadcrumb.name.clone())
        .unwrap_or_else(|| directory_path.clone());
    Ok(DirectoryPayload {
        path: directory_path,
        name,
        photo_count: inside.len(),
        breadcrumbs,
        subfolders,
        photos: inside,
    })
}

pub fn search(photos: &[PhotoAsset], query: &SearchQuery, limit: usize) -> SearchPayload {
    let terms: Vec<String> = query
        .text
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    let camera = query.camera.as_deref().filter(|camera| !camera.is_empty());
    let from = query.from.as_deref().filter(|value| !value.is_empty());
    let to = query.to.as_deref().filter(|value| !value.is_empty());

    let matches = |photo: &&PhotoAsset| {
        let date = photo.captured_at.get(..10).unwrap_or_default();
        if from.is_some_and(|from| date < from) || to.is_some_and(|to| date > to) {
            return false;
        }
        if camera.is_some_and(|camera| photo.camera.as_deref() != Some(camera)) {
            return false;
        }
        if terms.is_empty() {
            return true;
        }
        let haystack = format!(
            "{}\n{}\n{}",
            photo.name,
            photo.directory,
            photo.camera.as_deref().unwrap_or_default()
        )
        .to_lowercase();
        terms.iter().all(|term| haystack.contains(term))
    };

    let mut total = 0;
    let mut results = Vec::new();
    for photo in photos.iter().filter(matches) {
        total += 1;
        if results.len() < limit {
            results.push(photo.clone());
        }
    }

    let mut cameras: Vec<String> = photos
        .iter()
        .filter_map(|photo| photo.camera.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    cameras.sort_by_key(|camera| camera.to_lowercase());

    SearchPayload {
        truncated: total > results.len(),
        photos: results,
        total,
        cameras,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tests::{sample_jpeg_with_exif, write_temp_file};

    fn p(parts: &[&str]) -> String {
        let mut path = PathBuf::new();
        for part in parts {
            path.push(part);
        }
        path_to_string(&path)
    }

    fn photo(path: &str, captured_at: &str, camera: Option<&str>) -> PhotoAsset {
        let date = NaiveDateTime::parse_from_str(captured_at, "%Y-%m-%dT%H:%M:%S%.3f").unwrap();
        PhotoAsset {
            id: photo_id(path),
            name: file_name(path),
            path: path.to_string(),
            directory: parent_directory(path),
            extension: extension_lowercase(path),
            size: 10,
            cache_key: String::new(),
            url: String::new(),
            thumbnail_url: String::new(),
            captured_at: captured_at.to_string(),
            date_source: DateSource::Exif,
            year: date.year(),
            month: date.month(),
            month_name: month_name(date.month()),
            camera: camera.map(str::to_string),
            location: None,
            width: None,
            height: None,
        }
    }

    fn snapshot(path: &str, size: u64, mtime_ms: i64) -> PhotoFileSnapshot {
        PhotoFileSnapshot {
            path: path.to_string(),
            size,
            mtime_ms,
        }
    }

    fn library_of(root: &str, entries: Vec<(PhotoAsset, PhotoFileSnapshot)>) -> Library {
        let mut photos = Vec::new();
        let mut stats = HashMap::new();
        for (photo, stat) in entries {
            stats.insert(stat.path.clone(), stat);
            photos.push(photo);
        }
        Library::build(&[root.to_string()], photos, stats, &[], None)
    }

    #[test]
    fn scan_plan_reuses_unchanged_files_and_counts_removals() {
        let root = p(&["lib"]);
        let kept = p(&["lib", "kept.jpg"]);
        let edited = p(&["lib", "edited.jpg"]);
        let deleted = p(&["lib", "deleted.jpg"]);
        let added = p(&["lib", "added.jpg"]);
        let previous = library_of(
            &root,
            vec![
                (
                    photo(&kept, "2020-01-01T00:00:00.000", None),
                    snapshot(&kept, 10, 1),
                ),
                (
                    photo(&edited, "2020-01-02T00:00:00.000", None),
                    snapshot(&edited, 10, 1),
                ),
                (
                    photo(&deleted, "2020-01-03T00:00:00.000", None),
                    snapshot(&deleted, 10, 1),
                ),
            ],
        );

        let found = vec![
            snapshot(&kept, 10, 1),
            snapshot(&edited, 10, 2),
            snapshot(&added, 5, 1),
        ];
        let plan = plan_scan(&previous, found.clone(), false);
        assert_eq!(plan.reused.len(), 1);
        assert_eq!(plan.reused[0].path, kept);
        let to_index: Vec<&str> = plan
            .to_index
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(to_index, vec![edited.as_str(), added.as_str()]);
        assert_eq!(plan.removed, 1);

        let forced = plan_scan(&previous, found, true);
        assert!(forced.reused.is_empty());
        assert_eq!(forced.to_index.len(), 3);
        assert_eq!(forced.removed, 0);
    }

    #[test]
    fn index_round_trips_and_rejects_changed_roots() {
        let source = write_temp_file("a.jpg", &sample_jpeg_with_exif());
        let root = path_to_string(source.parent().unwrap());
        let data_dir = source.parent().unwrap().join("data");
        let emitter = ProgressEmitter::new(None, root.clone(), true);
        let roots = vec![root.clone()];

        let outcome = scan_library(&roots, &[], &[], &Library::empty(&roots), false, &emitter);
        assert_eq!(outcome.library.photos.len(), 1);
        let scanned = &outcome.library.photos[0];
        assert_eq!(scanned.date_source, DateSource::Exif);
        assert_eq!(scanned.captured_at, "2021-07-04T21:15:09.000");
        assert_eq!((scanned.year, scanned.month), (2021, 7));
        assert_eq!(scanned.camera.as_deref(), Some("Canon EOS R5"));
        assert!(scanned.thumbnail_url.contains("/thumb/"));

        write_library_index(&data_dir, &outcome.library, &roots, &[]).unwrap();
        let loaded = load_library(&data_dir, &roots, &[]);
        assert!(loaded.has_index);
        assert_eq!(loaded.photos.len(), 1);
        assert_eq!(loaded.photos[0].captured_at, scanned.captured_at);
        assert_eq!(loaded.photos[0].thumbnail_url, scanned.thumbnail_url);

        let rescan = scan_library(&roots, &[], &[], &loaded, false, &emitter);
        assert_eq!((rescan.changed, rescan.removed), (0, 0));
        assert_eq!(rescan.library.photos.len(), 1);
        assert!(!rescan.has_changes());

        let other_roots = vec![p(&[&root, "elsewhere"])];
        assert!(!load_library(&data_dir, &other_roots, &[]).has_index);
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn on_this_day_matches_month_and_day_in_earlier_years() {
        let photos = vec![
            photo("a.jpg", "2025-09-26T08:00:00.000", None),
            photo("b.jpg", "2023-09-26T23:59:00.000", None),
            photo("c.jpg", "2023-09-25T12:00:00.000", None),
            photo("d.jpg", "2026-09-26T12:00:00.000", None),
        ];
        let today = NaiveDate::from_ymd_opt(2026, 9, 26).unwrap();
        let names: Vec<String> = on_this_day(&photos, today, 10)
            .into_iter()
            .map(|photo| photo.name)
            .collect();
        assert_eq!(names, vec!["a.jpg", "b.jpg"]);
    }

    #[test]
    fn search_filters_by_text_camera_and_date_range() {
        let photos = vec![
            photo(
                &p(&["lib", "Beach", "sunset.jpg"]),
                "2024-06-01T10:00:00.000",
                Some("Pixel 8"),
            ),
            photo(
                &p(&["lib", "Beach", "waves.jpg"]),
                "2023-06-01T10:00:00.000",
                Some("iPhone 15"),
            ),
            photo(
                &p(&["lib", "City", "night.jpg"]),
                "2024-01-01T10:00:00.000",
                Some("Pixel 8"),
            ),
        ];
        let query = |text: &str, camera: Option<&str>, from: Option<&str>| SearchQuery {
            text: Some(text.to_string()),
            camera: camera.map(str::to_string),
            from: from.map(str::to_string),
            to: None,
        };

        assert_eq!(search(&photos, &query("beach", None, None), 10).total, 2);
        assert_eq!(
            search(&photos, &query("beach pixel", None, None), 10).total,
            1
        );
        assert_eq!(
            search(&photos, &query("", Some("Pixel 8"), None), 10).total,
            2
        );
        assert_eq!(
            search(&photos, &query("", None, Some("2024-01-02")), 10).total,
            1
        );

        let limited = search(&photos, &query("", None, None), 2);
        assert_eq!(
            (limited.photos.len(), limited.total, limited.truncated),
            (2, 3, true)
        );
        assert_eq!(limited.cameras, vec!["iPhone 15", "Pixel 8"]);
    }

    #[test]
    fn directory_payload_lists_subfolders_with_counts() {
        let root = p(&["lib"]);
        let photos = vec![
            photo(
                &p(&["lib", "Trips", "Rome", "a.jpg"]),
                "2024-06-03T10:00:00.000",
                None,
            ),
            photo(
                &p(&["lib", "Trips", "Rome", "b.jpg"]),
                "2024-06-02T10:00:00.000",
                None,
            ),
            photo(
                &p(&["lib", "Trips", "oslo", "c.jpg"]),
                "2024-06-01T10:00:00.000",
                None,
            ),
            photo(
                &p(&["lib", "Trips", "d.jpg"]),
                "2024-05-01T10:00:00.000",
                None,
            ),
            photo(
                &p(&["lib", "Other", "e.jpg"]),
                "2024-04-01T10:00:00.000",
                None,
            ),
        ];
        let payload = directory_payload(
            &photos,
            std::slice::from_ref(&root),
            &[],
            &p(&["lib", "Trips"]),
        )
        .unwrap();
        assert_eq!(payload.photo_count, 4);
        let folders: Vec<(String, usize)> = payload
            .subfolders
            .iter()
            .map(|folder| (folder.name.clone(), folder.photo_count))
            .collect();
        assert_eq!(
            folders,
            vec![("oslo".to_string(), 1), ("Rome".to_string(), 2)]
        );
        assert_eq!(payload.subfolders[1].path, p(&["lib", "Trips", "Rome"]));
        let crumbs: Vec<&str> = payload
            .breadcrumbs
            .iter()
            .map(|crumb| crumb.name.as_str())
            .collect();
        assert_eq!(crumbs, vec!["lib", "Trips"]);

        assert!(directory_payload(
            &photos,
            std::slice::from_ref(&root),
            &[],
            &p(&["elsewhere"])
        )
        .is_err());
        let excluded = vec![p(&["lib", "Trips"])];
        assert!(
            directory_payload(&photos, &[root], &excluded, &p(&["lib", "Trips", "Rome"])).is_err()
        );
    }

    #[test]
    fn random_photos_returns_distinct_picks() {
        let photos: Vec<PhotoAsset> = (0..50)
            .map(|index| photo(&format!("{index}.jpg"), "2024-01-01T00:00:00.000", None))
            .collect();
        let picked = random_photos(&photos, 20, 42);
        let unique: HashSet<&str> = picked.iter().map(|photo| photo.path.as_str()).collect();
        assert_eq!((picked.len(), unique.len()), (20, 20));
        assert_eq!(random_photos(&photos, 100, 7).len(), 50);
    }

    #[test]
    fn photo_urls_match_the_protocol_parser() {
        let url = make_photo_url(&p(&["lib", "a b.jpg"]), PhotoRenderVariant::Thumb, "1-2");
        let (variant, path) = crate::protocol::parse_photo_request(&url).unwrap();
        assert_eq!(variant, PhotoRenderVariant::Thumb);
        assert_eq!(path, p(&["lib", "a b.jpg"]));
    }
}
