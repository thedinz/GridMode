//! Rendering photos into cached JPEG thumbnails and display images.

use crate::metadata::read_orientation;
use crate::paths::path_to_string;
use image::{
    imageops::FilterType, io::Reader as ImageReader, ColorType, DynamicImage, GenericImageView,
};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{BufWriter, Read},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const IMAGE_CACHE_VERSION: &str = "v3-rust-jpeg-exif-orientation";
pub const THUMBNAIL_SIZE: u32 = 320;
const DISPLAY_MAX_DIMENSION: u32 = 3840;
const MIN_IMAGE_RENDER_JOB_LIMIT: usize = 2;
const MAX_IMAGE_RENDER_JOB_LIMIT: usize = 3;
/// Browser-native files above this size get a downscaled display render
/// instead of shipping the whole original through the protocol handler.
pub const LARGE_DISPLAY_FILE_BYTES: u64 = 24 * 1024 * 1024;

pub const SUPPORTED_EXTENSIONS: [&str; 13] = [
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "tif", "tiff", "heic", "heif",
    "avif",
];

const BROWSER_NATIVE_EXTENSIONS: [&str; 9] = [
    "jpg", "jpeg", "jpe", "jfif", "png", "webp", "gif", "bmp", "avif",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhotoRenderVariant {
    Image,
    Display,
    Thumb,
}

impl PhotoRenderVariant {
    pub fn token(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Display => "display",
            Self::Thumb => "thumb",
        }
    }

    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "image" => Some(Self::Image),
            "display" => Some(Self::Display),
            "thumb" => Some(Self::Thumb),
            _ => None,
        }
    }

    fn jpeg_quality(self) -> u8 {
        match self {
            Self::Thumb => 74,
            Self::Display | Self::Image => 88,
        }
    }

    fn settings_token(self) -> String {
        match self {
            Self::Thumb => format!("thumb:{}:{}:triangle", THUMBNAIL_SIZE, self.jpeg_quality()),
            Self::Display | Self::Image => format!(
                "display:{}:{}:lanczos3",
                DISPLAY_MAX_DIMENSION,
                self.jpeg_quality()
            ),
        }
    }

    fn target(self) -> RenderTarget {
        match self {
            Self::Thumb => RenderTarget::Cover(THUMBNAIL_SIZE),
            Self::Display | Self::Image => RenderTarget::Inside(DISPLAY_MAX_DIMENSION),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RenderTarget {
    /// Scale so both sides are at least this size, then center-crop to a square.
    Cover(u32),
    /// Scale so neither side exceeds this size.
    Inside(u32),
}

impl RenderTarget {
    /// Scale factor (<= 1) a decoder may apply before the final resize.
    pub fn prescale(self, width: u32, height: u32) -> f64 {
        let scale = match self {
            Self::Cover(size) => (size as f64 / width as f64).max(size as f64 / height as f64),
            Self::Inside(size) => (size as f64 / width as f64).min(size as f64 / height as f64),
        };
        scale.min(1.0)
    }
}

pub fn is_supported_photo_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| SUPPORTED_EXTENSIONS.contains(&extension.to_lowercase().as_str()))
}

struct ImageRenderQueue {
    active: Mutex<usize>,
    available: Condvar,
}

struct ImageRenderPermit(&'static ImageRenderQueue);

impl Drop for ImageRenderPermit {
    fn drop(&mut self) {
        let mut active = self
            .0
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *active = active.saturating_sub(1);
        self.0.available.notify_one();
    }
}

fn acquire_render_permit() -> ImageRenderPermit {
    static QUEUE: OnceLock<ImageRenderQueue> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| ImageRenderQueue {
        active: Mutex::new(0),
        available: Condvar::new(),
    });
    let limit = image_render_job_limit();
    let mut active = queue
        .active
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    while *active >= limit {
        active = queue
            .available
            .wait(active)
            .unwrap_or_else(|error| error.into_inner());
    }
    *active += 1;
    ImageRenderPermit(queue)
}

pub fn image_render_job_limit() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| {
            parallelism
                .get()
                .saturating_sub(1)
                .clamp(MIN_IMAGE_RENDER_JOB_LIMIT, MAX_IMAGE_RENDER_JOB_LIMIT)
        })
        .unwrap_or(MIN_IMAGE_RENDER_JOB_LIMIT)
}

pub fn image_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("image-cache")
}

fn metadata_modified_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn image_cache_file_path(
    data_dir: &Path,
    file_path: &str,
    metadata: &fs::Metadata,
    variant: PhotoRenderVariant,
) -> PathBuf {
    let mut hasher = Sha1::new();
    hasher.update(IMAGE_CACHE_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(variant.token().as_bytes());
    hasher.update([0]);
    hasher.update(variant.settings_token().as_bytes());
    hasher.update([0]);
    hasher.update(file_path.as_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(metadata_modified_ms(metadata).to_string().as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    image_cache_path(data_dir)
        .join(variant.token())
        .join(&hash[..2])
        .join(format!("{hash}.jpg"))
}

/// Returns the cached render for `file_path`, generating it if needed.
/// The flag is true when this call produced the file.
pub fn ensure_cached_render(
    data_dir: &Path,
    file_path: &str,
    variant: PhotoRenderVariant,
) -> Result<(PathBuf, bool), String> {
    let metadata = fs::metadata(file_path).map_err(|error| error.to_string())?;
    let output_path = image_cache_file_path(data_dir, file_path, &metadata, variant);

    if output_path.exists() {
        return Ok((output_path, false));
    }

    let _permit = acquire_render_permit();
    if output_path.exists() {
        return Ok((output_path, false));
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let temp_path = temp_image_cache_file_path(&output_path);
    render_image(file_path, &temp_path, variant).inspect_err(|_| {
        let _ = fs::remove_file(&temp_path);
    })?;
    let was_generated = publish_cached_render(&temp_path, &output_path)?;
    Ok((output_path, was_generated))
}

fn temp_image_cache_file_path(output_path: &Path) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("render.jpg");
    output_path.with_file_name(format!("{file_name}.{stamp}.tmp"))
}

pub fn publish_cached_render(temp_path: &Path, output_path: &Path) -> Result<bool, String> {
    if output_path.exists() {
        let _ = fs::remove_file(temp_path);
        return Ok(false);
    }

    match fs::rename(temp_path, output_path) {
        Ok(()) => Ok(true),
        Err(_) if output_path.exists() => {
            // An on-demand request and the background prebuilder can finish the same
            // image at the same time. Windows will not replace the winner's file.
            let _ = fs::remove_file(temp_path);
            Ok(false)
        }
        Err(error) => {
            let _ = fs::remove_file(temp_path);
            Err(error.to_string())
        }
    }
}

fn render_image(
    file_path: &str,
    output_path: &Path,
    variant: PhotoRenderVariant,
) -> Result<(), String> {
    let metadata = fs::metadata(file_path).map_err(|error| error.to_string())?;
    if metadata.len() == 0 {
        return Err("Image file is empty.".to_string());
    }

    let target = variant.target();
    let image = match decode_image(file_path) {
        Ok(image) => image,
        Err(primary_error) => {
            system_codec::decode(file_path, target).map_err(|fallback_error| {
                format!("{primary_error}; system image decoder fallback failed: {fallback_error}")
            })?
        }
    };
    let image = apply_exif_orientation(read_orientation(file_path), image);
    let rendered = match target {
        RenderTarget::Cover(size) => resize_to_cover(image, size),
        RenderTarget::Inside(size) => resize_inside(image, size),
    };
    write_jpeg(output_path, &rendered, variant.jpeg_quality())
}

fn decode_image(file_path: &str) -> Result<DynamicImage, String> {
    ImageReader::open(file_path)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?
        .decode()
        .map_err(|error| error.to_string())
}

fn apply_exif_orientation(orientation: Option<u32>, image: DynamicImage) -> DynamicImage {
    match orientation {
        Some(2) => image.fliph(),
        Some(3) => image.rotate180(),
        Some(4) => image.flipv(),
        Some(5) => image.rotate90().fliph(),
        Some(6) => image.rotate90(),
        Some(7) => image.rotate90().flipv(),
        Some(8) => image.rotate270(),
        _ => image,
    }
}

fn resize_to_cover(image: DynamicImage, size: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width <= size && height <= size {
        return image;
    }

    let scale = (size as f64 / width as f64).max(size as f64 / height as f64);
    let resized_width = ((width as f64 * scale).round() as u32).max(1);
    let resized_height = ((height as f64 * scale).round() as u32).max(1);
    let resized = image.resize(resized_width, resized_height, FilterType::Triangle);
    let crop_width = resized_width.min(size);
    let crop_height = resized_height.min(size);
    let x = resized_width.saturating_sub(crop_width) / 2;
    let y = resized_height.saturating_sub(crop_height) / 2;
    resized.crop_imm(x, y, crop_width, crop_height)
}

fn resize_inside(image: DynamicImage, max_dimension: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width <= max_dimension && height <= max_dimension {
        return image;
    }
    image.resize(max_dimension, max_dimension, FilterType::Lanczos3)
}

fn write_jpeg(output_path: &Path, image: &DynamicImage, quality: u8) -> Result<(), String> {
    let file = fs::File::create(output_path).map_err(|error| error.to_string())?;
    let mut writer = BufWriter::new(file);
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
    encoder
        .encode(&rgb, width, height, ColorType::Rgb8)
        .map_err(|error| error.to_string())
}

/// Whether the large photo view needs a rendered JPEG instead of the original bytes.
pub fn needs_rendered_display(file_path: &str, file_size: u64) -> bool {
    if file_size > LARGE_DISPLAY_FILE_BYTES || has_heif_content(file_path) {
        return true;
    }
    Path::new(file_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|extension| !BROWSER_NATIVE_EXTENSIONS.contains(&extension.to_lowercase().as_str()))
        .unwrap_or(true)
}

fn has_heif_content(file_path: &str) -> bool {
    let mut header = [0_u8; 64];
    let Ok(mut file) = fs::File::open(file_path) else {
        return false;
    };
    let Ok(bytes_read) = file.read(&mut header) else {
        return false;
    };
    has_heif_header(&header[..bytes_read])
}

pub fn has_heif_header(header: &[u8]) -> bool {
    const HEIF_BRANDS: [&[u8; 4]; 10] = [
        b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1", b"msf1",
    ];
    header
        .get(4..8)
        .is_some_and(|signature| signature == b"ftyp")
        && header
            .windows(4)
            .any(|brand| HEIF_BRANDS.iter().any(|known| brand == known.as_slice()))
}

pub fn mime_type_for_path(file_path: &str) -> &'static str {
    match Path::new(file_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg" | "jpe" | "jfif") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("tif" | "tiff") => "image/tiff",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
}

pub fn clear_image_cache(data_dir: &Path) -> Result<(), String> {
    remove_cache_directory(&image_cache_path(data_dir))
}

pub fn clear_thumbnail_cache(data_dir: &Path) -> Result<(), String> {
    remove_cache_directory(&image_cache_path(data_dir).join(PhotoRenderVariant::Thumb.token()))
}

fn remove_cache_directory(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Could not clear {}: {}",
            path_to_string(path),
            error
        )),
    }
}

/// Operating system decoders, used when the bundled decoders reject a file:
/// HEIC/AVIF, damaged JPEGs, or files whose extension does not match their contents.
#[cfg(target_os = "windows")]
mod system_codec {
    use super::RenderTarget;
    use image::{DynamicImage, RgbaImage};
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::GENERIC_READ,
            Graphics::Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICBitmapSource,
                IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant,
                WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
            },
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED,
            },
        },
    };

    /// Decodes through Windows Imaging Component, which also covers HEIC when the
    /// HEIF Image Extensions are installed. Large images are scaled inside WIC
    /// before pixels are copied out, so a 100 MP photo never needs a full buffer.
    pub fn decode(file_path: &str, target: RenderTarget) -> Result<DynamicImage, String> {
        // SAFETY: COM is initialized for this thread for the duration of the call
        // and every interface is released before CoUninitialize runs.
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let result = decode_with_wic(file_path, target);
            if initialized {
                CoUninitialize();
            }
            result.map_err(|error| format!("Windows image decoder: {}", error.message()))
        }
    }

    unsafe fn decode_with_wic(
        file_path: &str,
        target: RenderTarget,
    ) -> windows::core::Result<DynamicImage> {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let wide_path: Vec<u16> = file_path.encode_utf16().chain(std::iter::once(0)).collect();
        let decoder = factory.CreateDecoderFromFilename(
            PCWSTR(wide_path.as_ptr()),
            None,
            GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;

        let (mut width, mut height) = (0_u32, 0_u32);
        frame.GetSize(&mut width, &mut height)?;
        let mut source: IWICBitmapSource = frame.into();
        let scale = target.prescale(width.max(1), height.max(1));
        if scale < 1.0 {
            let scaled_width = ((width as f64 * scale).round() as u32).max(1);
            let scaled_height = ((height as f64 * scale).round() as u32).max(1);
            let scaler = factory.CreateBitmapScaler()?;
            scaler.Initialize(
                &source,
                scaled_width,
                scaled_height,
                WICBitmapInterpolationModeFant,
            )?;
            source = scaler.into();
        }

        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &source,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        converter.GetSize(&mut width, &mut height)?;
        let stride = width * 4;
        let mut pixels = vec![0_u8; stride as usize * height as usize];
        converter.CopyPixels(std::ptr::null(), stride, &mut pixels)?;

        RgbaImage::from_raw(width, height, pixels)
            .map(DynamicImage::ImageRgba8)
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
    }
}

#[cfg(target_os = "macos")]
mod system_codec {
    use super::RenderTarget;
    use image::DynamicImage;
    use std::{fs, process::Command};

    pub fn decode(file_path: &str, _target: RenderTarget) -> Result<DynamicImage, String> {
        let scratch = std::env::temp_dir().join(format!(
            "gridmode-decode-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let result = Command::new("sips")
            .args(["-s", "format", "png", file_path, "--out"])
            .arg(&scratch)
            .output()
            .map_err(|error| format!("Could not start the macOS image decoder: {error}"));
        let decoded = result.and_then(|output| {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(format!("macOS image decoder: {stderr}"));
            }
            image::open(&scratch)
                .map_err(|error| format!("Could not read system decoder output: {error}"))
        });
        let _ = fs::remove_file(&scratch);
        decoded
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod system_codec {
    use super::RenderTarget;
    use image::DynamicImage;

    pub fn decode(_file_path: &str, _target: RenderTarget) -> Result<DynamicImage, String> {
        Err("No system image decoder is available on this platform.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tests::{sample_jpeg_with_exif, write_temp_file};

    #[test]
    fn heif_content_is_detected_even_with_a_jpeg_file_name() {
        let header = b"\0\0\0\x18ftypheic\0\0\0\0mif1";
        assert!(has_heif_header(header));
        assert!(!has_heif_header(b"\xff\xd8\xff\xe0JFIF"));
    }

    #[test]
    fn publishing_a_thumbnail_accepts_a_concurrent_winner() {
        let output_path = write_temp_file("thumbnail.jpg", b"winner");
        let temp_path = output_path.with_file_name("thumbnail.tmp");
        fs::write(&temp_path, b"contender").unwrap();

        let generated = publish_cached_render(&temp_path, &output_path)
            .expect("an existing completed thumbnail should win the race");

        assert!(!generated);
        assert_eq!(fs::read(&output_path).unwrap(), b"winner");
        assert!(!temp_path.exists());
        fs::remove_dir_all(output_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn thumbnails_are_rotated_by_exif_orientation() {
        // The sample is 8x4 with orientation 6 (rotate 90), so the render is 4x8.
        let source = write_temp_file("rotated.jpg", &sample_jpeg_with_exif());
        let data_dir = source.parent().unwrap().join("data");
        let (path, generated) = ensure_cached_render(
            &data_dir,
            source.to_str().unwrap(),
            PhotoRenderVariant::Thumb,
        )
        .unwrap();
        assert!(generated);
        assert_eq!(image::image_dimensions(&path).unwrap(), (4, 8));

        let (_, generated_again) = ensure_cached_render(
            &data_dir,
            source.to_str().unwrap(),
            PhotoRenderVariant::Thumb,
        )
        .unwrap();
        assert!(!generated_again, "second request should reuse the cache");
        fs::remove_dir_all(source.parent().unwrap()).unwrap();
    }

    #[test]
    fn large_browser_native_files_use_a_rendered_display() {
        assert!(!needs_rendered_display("photo.jpg", 1024));
        assert!(needs_rendered_display(
            "photo.jpg",
            LARGE_DISPLAY_FILE_BYTES + 1
        ));
        assert!(needs_rendered_display("photo.tif", 1024));
    }

    #[test]
    fn prescale_never_upscales() {
        assert_eq!(RenderTarget::Cover(320).prescale(100, 100), 1.0);
        assert!((RenderTarget::Cover(320).prescale(6400, 3200) - 0.1).abs() < 1e-9);
        assert!((RenderTarget::Inside(3840).prescale(7680, 3840) - 0.5).abs() < 1e-9);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_codec_decodes_and_prescales() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(1280, 640)
            .write_to(&mut png, image::ImageOutputFormat::Png)
            .unwrap();
        let path = write_temp_file("wide.png", &png.into_inner());
        let image = system_codec::decode(path.to_str().unwrap(), RenderTarget::Cover(320)).unwrap();
        assert_eq!(image.dimensions(), (640, 320));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
