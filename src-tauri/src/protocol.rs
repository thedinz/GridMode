//! The `gridmode-photo` URI scheme that serves thumbnails and display images.
//!
//! URLs look like `<origin>/<variant>/<base64url path>?v=<cache key>`. Only
//! photos in the current library index are served.

use crate::render::{
    ensure_cached_render, mime_type_for_path, needs_rendered_display, PhotoRenderVariant,
};
use crate::state::AppState;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::fs;
use tauri::{http, AppHandle, Manager};

pub fn handle_photo_request(app: &AppHandle, uri: &str) -> http::Response<Vec<u8>> {
    let (variant, file_path) = match parse_photo_request(uri) {
        Ok(request) => request,
        Err(error) => return text_response(http::StatusCode::BAD_REQUEST, &error),
    };
    if crate::licensing::require_access(app).is_err() {
        return text_response(
            http::StatusCode::FORBIDDEN,
            crate::licensing::LICENSE_REQUIRED,
        );
    }

    let state = app.state::<AppState>();
    let Some(photo) = state.find_photo(&file_path) else {
        return text_response(
            http::StatusCode::FORBIDDEN,
            "Photo is not part of the current library.",
        );
    };

    match read_photo_response(&state, &photo.path, variant) {
        Ok((bytes, content_type, immutable)) => binary_response(bytes, content_type, immutable),
        Err(error) => text_response(http::StatusCode::INTERNAL_SERVER_ERROR, &error),
    }
}

pub fn parse_photo_request(uri: &str) -> Result<(PhotoRenderVariant, String), String> {
    let uri: http::Uri = uri
        .parse()
        .map_err(|error| format!("Unsupported photo URL: {error}"))?;
    let authority = uri
        .authority()
        .map(|value| value.host())
        .unwrap_or_default();
    // convertFileSrc-style URLs percent-encode the slash after the variant.
    let path = percent_decode(uri.path().trim_start_matches('/'))?;

    let (variant_token, path_token) = match PhotoRenderVariant::from_token(authority) {
        Some(_) => (authority, path.as_str()),
        None => path.split_once('/').unwrap_or((path.as_str(), "")),
    };
    let variant = PhotoRenderVariant::from_token(variant_token)
        .ok_or_else(|| "Unsupported photo request variant.".to_string())?;
    if path_token.is_empty() {
        return Err("Photo request did not include a file path.".to_string());
    }

    let bytes = URL_SAFE_NO_PAD
        .decode(path_token)
        .map_err(|error| format!("Photo path token could not be decoded: {error}"))?;
    let file_path =
        String::from_utf8(bytes).map_err(|error| format!("Photo path was not UTF-8: {error}"))?;
    Ok((variant, file_path))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "Photo URL contained an invalid percent escape.".to_string())?;
            decoded.push(hex);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|error| format!("Photo URL path was not UTF-8: {error}"))
}

fn read_photo_response(
    state: &AppState,
    file_path: &str,
    variant: PhotoRenderVariant,
) -> Result<(Vec<u8>, &'static str, bool), String> {
    let file_size = fs::metadata(file_path)
        .map_err(|error| error.to_string())?
        .len();
    let use_render = variant == PhotoRenderVariant::Thumb
        || (variant == PhotoRenderVariant::Display && needs_rendered_display(file_path, file_size));

    if use_render {
        let (cached_path, _) = ensure_cached_render(&state.data_dir, file_path, variant)?;
        let bytes = fs::read(cached_path).map_err(|error| error.to_string())?;
        return Ok((bytes, "image/jpeg", true));
    }

    let bytes = fs::read(file_path).map_err(|error| error.to_string())?;
    Ok((bytes, mime_type_for_path(file_path), false))
}

fn binary_response(bytes: Vec<u8>, content_type: &str, immutable: bool) -> http::Response<Vec<u8>> {
    let cache_control = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, content_type)
        .header(http::header::CACHE_CONTROL, cache_control)
        .body(bytes)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn text_response(status: http::StatusCode, message: &str) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(message.as_bytes().to_vec())
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_convert_file_src_urls_with_an_encoded_slash() {
        let token = URL_SAFE_NO_PAD.encode("C:\\Photos\\a.jpg");
        let url = format!("http://gridmode-photo.localhost/display%2F{token}?v=1-2");
        let (variant, path) = parse_photo_request(&url).unwrap();
        assert_eq!(variant, PhotoRenderVariant::Display);
        assert_eq!(path, "C:\\Photos\\a.jpg");
    }

    #[test]
    fn parses_legacy_variant_authority_urls() {
        let token = URL_SAFE_NO_PAD.encode("/Users/me/a.heic");
        let (variant, path) =
            parse_photo_request(&format!("gridmode-photo://thumb/{token}")).unwrap();
        assert_eq!(variant, PhotoRenderVariant::Thumb);
        assert_eq!(path, "/Users/me/a.heic");
    }

    #[test]
    fn rejects_unknown_variants_and_bad_escapes() {
        assert!(parse_photo_request("gridmode-photo://localhost/raw/abc").is_err());
        assert!(parse_photo_request("gridmode-photo://localhost/thumb%2").is_err());
        assert!(parse_photo_request("gridmode-photo://localhost/thumb/").is_err());
    }
}
