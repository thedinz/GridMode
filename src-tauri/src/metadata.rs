//! EXIF extraction: capture dates, camera, GPS, dimensions, and orientation.

use crate::model::{ExifRow, PhotoLocation};
use chrono::{NaiveDate, NaiveDateTime};
use exif::{Exif, Field, In, Tag, Value};
use std::{
    fs,
    io::{BufReader, Read},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhotoMetadata {
    pub captured: Option<NaiveDateTime>,
    pub camera: Option<String>,
    pub location: Option<PhotoLocation>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: Option<u32>,
}

pub fn read_exif(path: &str) -> Option<Exif> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    exif::Reader::new().read_from_container(&mut reader).ok()
}

/// Reads the metadata GridMode stores in its library index.
pub fn read_photo_metadata(path: &str) -> PhotoMetadata {
    let mut metadata = read_exif(path)
        .map(|exif| summarize(&exif))
        .unwrap_or_default();

    // Header-only decode gives exact pixel dimensions for formats the image crate knows.
    if let Ok((width, height)) = image::image_dimensions(path) {
        metadata.width = Some(width);
        metadata.height = Some(height);
    }
    if matches!(metadata.orientation, Some(5..=8)) {
        std::mem::swap(&mut metadata.width, &mut metadata.height);
    }
    metadata
}

pub fn summarize(exif: &Exif) -> PhotoMetadata {
    PhotoMetadata {
        captured: capture_time(exif),
        camera: camera_name(exif),
        location: gps_location(exif),
        width: uint_field(exif, Tag::PixelXDimension),
        height: uint_field(exif, Tag::PixelYDimension),
        orientation: uint_field(exif, Tag::Orientation),
    }
}

/// EXIF orientation for files whose decoders do not apply it themselves.
/// HEIF/AVIF carry rotation in their container, which system decoders already honor.
pub fn read_orientation(path: &str) -> Option<u32> {
    let mut signature = [0_u8; 2];
    fs::File::open(path).ok()?.read_exact(&mut signature).ok()?;
    let is_jpeg_or_tiff = signature == [0xff, 0xd8] || signature == *b"II" || signature == *b"MM";
    if !is_jpeg_or_tiff {
        return None;
    }
    uint_field(&read_exif(path)?, Tag::Orientation)
}

fn capture_time(exif: &Exif) -> Option<NaiveDateTime> {
    [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime]
        .into_iter()
        .find_map(|tag| {
            let field = exif.get_field(tag, In::PRIMARY)?;
            let Value::Ascii(values) = &field.value else {
                return None;
            };
            parse_exif_datetime(values.first()?)
        })
}

pub fn parse_exif_datetime(bytes: &[u8]) -> Option<NaiveDateTime> {
    let value = exif::DateTime::from_ascii(bytes).ok()?;
    if value.year < 1900 {
        return None;
    }
    NaiveDate::from_ymd_opt(value.year.into(), value.month.into(), value.day.into())?.and_hms_opt(
        value.hour.into(),
        value.minute.into(),
        value.second.into(),
    )
}

fn camera_name(exif: &Exif) -> Option<String> {
    let make = ascii_field(exif, Tag::Make);
    let model = ascii_field(exif, Tag::Model);
    match (make, model) {
        (Some(make), Some(model)) => {
            // Many cameras repeat the maker in the model ("Canon" + "Canon EOS R5").
            let brand = make
                .split_whitespace()
                .next()
                .unwrap_or(&make)
                .to_lowercase();
            if model.to_lowercase().starts_with(&brand) {
                Some(model)
            } else {
                Some(format!("{make} {model}"))
            }
        }
        (make, model) => model.or(make),
    }
}

fn gps_location(exif: &Exif) -> Option<PhotoLocation> {
    let latitude = gps_coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, "S")?;
    let longitude = gps_coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, "W")?;
    let in_range = (-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude);
    // 0,0 is what many devices write when they have no fix.
    let is_null_island = latitude == 0.0 && longitude == 0.0;
    (in_range && !is_null_island).then_some(PhotoLocation {
        latitude,
        longitude,
    })
}

fn gps_coordinate(exif: &Exif, tag: Tag, ref_tag: Tag, negative_ref: &str) -> Option<f64> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let Value::Rational(parts) = &field.value else {
        return None;
    };
    let degrees = parts.first()?.to_f64();
    let minutes = parts.get(1).map(|value| value.to_f64()).unwrap_or(0.0);
    let seconds = parts.get(2).map(|value| value.to_f64()).unwrap_or(0.0);
    let magnitude = degrees + minutes / 60.0 + seconds / 3600.0;
    if !magnitude.is_finite() {
        return None;
    }
    let is_negative = ascii_field(exif, ref_tag)
        .is_some_and(|reference| reference.eq_ignore_ascii_case(negative_ref));
    Some(if is_negative { -magnitude } else { magnitude })
}

/// Human-readable rows for the photo detail panel.
pub fn detail_rows(exif: &Exif) -> Vec<ExifRow> {
    let mut rows = Vec::new();
    let mut push = |label: &str, value: Option<String>| {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            rows.push(ExifRow {
                label: label.to_string(),
                value,
            });
        }
    };

    push("Camera", camera_name(exif));
    push("Lens", ascii_field(exif, Tag::LensModel));
    push(
        "Aperture",
        rational_field(exif, Tag::FNumber).map(|value| format!("f/{}", trim_float(value, 1))),
    );
    push(
        "Shutter",
        rational_field(exif, Tag::ExposureTime).map(format_exposure),
    );
    push(
        "ISO",
        uint_field(exif, Tag::PhotographicSensitivity).map(|value| value.to_string()),
    );
    push(
        "Focal length",
        rational_field(exif, Tag::FocalLength).map(|value| {
            match uint_field(exif, Tag::FocalLengthIn35mmFilm).filter(|value| *value > 0) {
                Some(equivalent) => format!("{} mm ({equivalent} mm equiv.)", trim_float(value, 1)),
                None => format!("{} mm", trim_float(value, 1)),
            }
        }),
    );
    rows
}

fn format_exposure(seconds: f64) -> String {
    if seconds <= 0.0 || !seconds.is_finite() {
        return String::new();
    }
    if seconds < 1.0 {
        format!("1/{} s", (1.0 / seconds).round())
    } else {
        format!("{} s", trim_float(seconds, 1))
    }
}

fn trim_float(value: f64, decimals: usize) -> String {
    let formatted = format!("{value:.decimals$}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn field(exif: &Exif, tag: Tag) -> Option<&Field> {
    exif.get_field(tag, In::PRIMARY)
}

fn ascii_field(exif: &Exif, tag: Tag) -> Option<String> {
    let Value::Ascii(values) = &field(exif, tag)?.value else {
        return None;
    };
    let text = String::from_utf8_lossy(values.first()?);
    let trimmed = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn uint_field(exif: &Exif, tag: Tag) -> Option<u32> {
    field(exif, tag)?.value.get_uint(0)
}

fn rational_field(exif: &Exif, tag: Tag) -> Option<f64> {
    match &field(exif, tag)?.value {
        Value::Rational(values) => values.first().map(|value| value.to_f64()),
        Value::SRational(values) => values.first().map(|value| value.to_f64()),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use exif::{experimental::Writer, Rational};
    use std::io::Cursor;

    fn ascii(tag: Tag, value: &str) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![value.as_bytes().to_vec()]),
        }
    }

    fn rationals(tag: Tag, values: &[(u32, u32)]) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(
                values
                    .iter()
                    .map(|(num, denom)| Rational {
                        num: *num,
                        denom: *denom,
                    })
                    .collect(),
            ),
        }
    }

    fn short(tag: Tag, value: u16) -> Field {
        Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Short(vec![value]),
        }
    }

    /// Builds a small JPEG carrying a realistic EXIF block, for tests elsewhere too.
    pub fn sample_jpeg_with_exif() -> Vec<u8> {
        let fields = vec![
            ascii(Tag::Make, "Canon"),
            ascii(Tag::Model, "Canon EOS R5"),
            ascii(Tag::DateTimeOriginal, "2021:07:04 21:15:09"),
            ascii(Tag::LensModel, "RF24-105mm F4 L IS USM"),
            short(Tag::Orientation, 6),
            rationals(Tag::FNumber, &[(28, 10)]),
            rationals(Tag::ExposureTime, &[(1, 250)]),
            short(Tag::PhotographicSensitivity, 400),
            rationals(Tag::FocalLength, &[(50, 1)]),
            ascii(Tag::GPSLatitudeRef, "N"),
            rationals(Tag::GPSLatitude, &[(40, 1), (26, 1), (4620, 100)]),
            ascii(Tag::GPSLongitudeRef, "W"),
            rationals(Tag::GPSLongitude, &[(79, 1), (58, 1), (5616, 100)]),
        ];
        let mut writer = Writer::new();
        for field in &fields {
            writer.push_field(field);
        }
        let mut tiff = Cursor::new(Vec::new());
        writer
            .write(&mut tiff, false)
            .expect("EXIF should serialize");

        let mut image = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8, 4)
            .write_to(&mut image, image::ImageOutputFormat::Jpeg(80))
            .expect("JPEG should encode");
        let image = image.into_inner();

        let tiff = tiff.into_inner();
        let segment_length = u16::try_from(tiff.len() + 8).expect("EXIF fits in one segment");
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&segment_length.to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        jpeg.extend_from_slice(&image[2..]);
        jpeg
    }

    pub fn write_temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gridmode-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn reads_capture_time_camera_gps_and_orientation() {
        let path = write_temp_file("sample.jpg", &sample_jpeg_with_exif());
        let path_text = path.to_str().unwrap();
        let metadata = read_photo_metadata(path_text);

        assert_eq!(
            metadata.captured,
            NaiveDate::from_ymd_opt(2021, 7, 4)
                .unwrap()
                .and_hms_opt(21, 15, 9)
        );
        assert_eq!(metadata.camera.as_deref(), Some("Canon EOS R5"));
        let location = metadata.location.expect("GPS should be read");
        assert!((location.latitude - 40.4462).abs() < 0.001);
        assert!((location.longitude + 79.9823).abs() < 0.001);
        assert_eq!(metadata.orientation, Some(6));
        // Orientation 6 rotates the 8x4 image, so reported dimensions swap.
        assert_eq!((metadata.width, metadata.height), (Some(4), Some(8)));
        assert_eq!(read_orientation(path_text), Some(6));

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn detail_rows_are_human_readable() {
        let path = write_temp_file("sample.jpg", &sample_jpeg_with_exif());
        let exif = read_exif(path.to_str().unwrap()).unwrap();
        let rows: Vec<(String, String)> = detail_rows(&exif)
            .into_iter()
            .map(|row| (row.label, row.value))
            .collect();

        assert!(rows.contains(&("Aperture".into(), "f/2.8".into())));
        assert!(rows.contains(&("Shutter".into(), "1/250 s".into())));
        assert!(rows.contains(&("ISO".into(), "400".into())));
        assert!(rows.contains(&("Focal length".into(), "50 mm".into())));
        assert!(rows.contains(&("Lens".into(), "RF24-105mm F4 L IS USM".into())));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn blank_and_ancient_dates_are_rejected() {
        assert_eq!(parse_exif_datetime(b"0000:00:00 00:00:00"), None);
        assert_eq!(parse_exif_datetime(b"    :  :     :  :  "), None);
        assert!(parse_exif_datetime(b"2003:02:01 10:00:00").is_some());
    }

    #[test]
    fn files_without_exif_yield_no_metadata() {
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(3, 2)
            .write_to(&mut png, image::ImageOutputFormat::Png)
            .unwrap();
        let path = write_temp_file("plain.png", &png.into_inner());
        let metadata = read_photo_metadata(path.to_str().unwrap());
        assert_eq!(metadata.captured, None);
        assert_eq!((metadata.width, metadata.height), (Some(3), Some(2)));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
