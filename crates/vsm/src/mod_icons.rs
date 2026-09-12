//! Optional, bounded thumbnails from installed mod archives. Never extract paths to disk.
use anyhow::{ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const MAX_ICON_BYTES: u64 = 256 * 1024;
const MAX_DIMENSION: u32 = 1024;
const THUMBNAIL: u32 = 64;
type Key = (PathBuf, u64, Option<SystemTime>, String);
static CACHE: OnceLock<Mutex<HashMap<Key, Option<String>>>> = OnceLock::new();

pub fn enrich(root: &Path, manifest: &mut Value) {
    let Some(mods) = manifest.as_array_mut() else {
        return;
    };
    // Bound the entire response as well as each thumbnail (about 3 MiB worst case).
    for (index, entry) in mods.iter_mut().enumerate() {
        let icon = if index < 128 { icon(root, entry) } else { None };
        if let Some(object) = entry.as_object_mut() {
            object.insert(
                "icon_data_url".into(),
                icon.map(Value::String).unwrap_or(Value::Null),
            );
        }
    }
}

fn icon(root: &Path, entry: &Value) -> Option<String> {
    let directory = root.join("data").join(if entry["enabled"] == false {
        "ModsDisabled"
    } else {
        "Mods"
    });
    let path = crate::files::confined(&directory, entry["filename"].as_str()?).ok()?;
    let metadata = std::fs::metadata(&path).ok()?;
    let key = (
        path.clone(),
        metadata.len(),
        metadata.modified().ok(),
        entry["sha256"].as_str().unwrap_or_default().to_owned(),
    );
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    if let Some(value) = cache.get(&key) {
        return value.clone();
    }
    let value = thumbnail(&path)
        .ok()
        .map(|bytes| format!("data:image/png;base64,{}", STANDARD.encode(bytes)));
    if cache.len() >= 128 {
        cache.clear();
    }
    cache.insert(key, value.clone());
    value
}

fn thumbnail(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    ensure!(
        length <= 256 * 1024 * 1024,
        "Archive exceeds icon scan limit"
    );
    file.seek(SeekFrom::End(-(length.min(65557) as i64)))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    let offset = tail
        .windows(4)
        .rposition(|v| v == b"PK\x05\x06")
        .ok_or_else(|| anyhow::anyhow!("Missing ZIP directory"))?;
    ensure!(
        offset < 20 || &tail[offset - 20..offset - 16] != b"PK\x06\x07",
        "ZIP64 icon scans are unsupported"
    );
    let end = tail
        .get(offset..offset + 22)
        .ok_or_else(|| anyhow::anyhow!("Truncated ZIP directory"))?;
    let entries = u16::from_le_bytes([end[10], end[11]]);
    let directory_bytes = u32::from_le_bytes(end[12..16].try_into()?);
    ensure!(
        entries <= 10000 && directory_bytes <= 4 * 1024 * 1024,
        "ZIP directory exceeds icon scan limit"
    );
    file.rewind()?;
    let mut archive = zip::ZipArchive::new(file)?;
    let entry = archive.by_name("modicon.png")?;
    ensure!(
        entry.is_file() && entry.unix_mode().unwrap_or(0) & 0o170000 != 0o120000,
        "Icon must be a regular file"
    );
    ensure!(entry.size() <= MAX_ICON_BYTES, "Icon is too large");
    let mut source = Vec::new();
    entry.take(MAX_ICON_BYTES + 1).read_to_end(&mut source)?;
    ensure!(source.len() as u64 <= MAX_ICON_BYTES, "Icon is too large");
    let mut decoder = png::Decoder::new(Cursor::new(source));
    decoder.set_limits(png::Limits {
        bytes: 8 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let (width, height) = (reader.info().width, reader.info().height);
    ensure!(
        width > 0 && height > 0 && width <= MAX_DIMENSION && height <= MAX_DIMENSION,
        "Icon dimensions exceed limit"
    );
    ensure!(
        reader.info().animation_control.is_none(),
        "Animated icons are unsupported"
    );
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels)?;
    ensure!(
        info.bit_depth == png::BitDepth::Eight,
        "Unsupported icon bit depth"
    );
    let channels = info.color_type.samples();
    let scale = width.max(height).max(THUMBNAIL);
    let w = (width * THUMBNAIL / scale).max(1);
    let h = (height * THUMBNAIL / scale).max(1);
    let mut output = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let offset = ((y * height / h * width + x * width / w) as usize) * channels;
            let p = &pixels[offset..offset + channels];
            output.extend_from_slice(&match info.color_type {
                png::ColorType::Rgba => [p[0], p[1], p[2], p[3]],
                png::ColorType::Rgb => [p[0], p[1], p[2], 255],
                png::ColorType::GrayscaleAlpha => [p[0], p[0], p[0], p[1]],
                png::ColorType::Grayscale => [p[0], p[0], p[0], 255],
                _ => anyhow::bail!("Unsupported icon color type"),
            });
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&output)?;
    }
    ensure!(bytes.len() <= 20 * 1024, "Thumbnail exceeds limit");
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn archive(path: &Path, name: &str, bytes: &[u8]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap();
    }
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![128; (width * height * 4) as usize])
                .unwrap();
        }
        bytes
    }
    #[test]
    fn existing_disabled_archive_gets_bounded_png_without_manifest_changes() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("data/ModsDisabled");
        std::fs::create_dir_all(&dir).unwrap();
        archive(&dir.join("test.zip"), "modicon.png", &png_bytes(128, 64));
        let mut value =
            serde_json::json!([{"filename":"test.zip","enabled":false,"custom":"preserved"}]);
        enrich(root.path(), &mut value);
        let encoded = value[0]["icon_data_url"]
            .as_str()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        let reader = png::Decoder::new(Cursor::new(decoded)).read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (64, 32));
        assert_eq!(value[0]["custom"], "preserved");
    }
    #[test]
    fn untrusted_missing_oversized_and_misnamed_icons_are_optional() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("mod.zip");
        for (name, data) in [
            ("modicon.png", b"<svg onload='alert(1)'/>".to_vec()),
            ("../modicon.png", png_bytes(1, 1)),
            ("modicon.png", vec![0; MAX_ICON_BYTES as usize + 1]),
            ("modicon.png", png_bytes(1025, 1)),
        ] {
            archive(&path, name, &data);
            assert!(thumbnail(&path).is_err());
        }
    }
    #[tokio::test]
    #[ignore = "Reads exact public ModDB archive without changing any server"]
    async fn exact_vigor_installed_icon() {
        let (_, bytes) = crate::moddb::archive("4425", 55847).await.unwrap();
        assert_eq!(
            crate::files::etag(&bytes),
            "8dd4f078919587ccbb0ad37d044f021b7640fc37e86f7ba282a9973f970dac31"
        );
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("vigor.zip");
        std::fs::write(&path, bytes).unwrap();
        let result = thumbnail(&path).unwrap();
        assert!(result.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(result.len() < 20 * 1024);
    }
}
