//! Vintage Story mod manifests are UTF-8 JSON; Windows writers may include a BOM.
use anyhow::{Context, Result};
use serde_json::Value;

pub fn parse(bytes: &[u8]) -> Result<Value> {
    anyhow::ensure!(
        bytes.len() <= 1024 * 1024,
        "modinfo.json exceeds the 1 MiB manifest limit"
    );
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes)
        .context("modinfo.json must be UTF-8 JSON (an optional UTF-8 BOM is supported)")?;
    let value: Value = serde_json::from_str(text)
        .map_err(|error| anyhow::anyhow!("Could not parse modinfo.json as JSON: {error}"))?;
    anyhow::ensure!(value.is_object(), "modinfo.json must contain a JSON object");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_utf8_with_or_without_bom_and_preserves_unknown_fields() {
        let raw=br#"{"type":"code","modid":"vigor","version":"122.7.4","dependencies":{"game":"1.22.0"},"unknown":{"preserve":true}}"#;
        let mut bom = b"\xef\xbb\xbf".to_vec();
        bom.extend_from_slice(raw);
        assert_eq!(parse(&bom).unwrap(), parse(raw).unwrap());
        assert_eq!(parse(&bom).unwrap()["unknown"]["preserve"], true);
    }
    #[test]
    fn malformed_manifests_keep_file_and_parser_context() {
        for raw in [
            b"".as_slice(),
            b"\xef\xbb\xbf",
            b"\xff\xfe{\0}",
            b"{invalid}",
            b"[]",
            b"\xef\xbb\xbf\xef\xbb\xbf{}",
        ] {
            assert!(parse(raw).unwrap_err().to_string().contains("modinfo.json"));
        }
        assert!(format!("{:#}", parse(b"{invalid}").unwrap_err()).contains("line 1 column"));
    }
    #[tokio::test]
    #[ignore = "Downloads the exact public ModDB release; no server mutation"]
    async fn exact_vigor_122_7_4_release_with_bom() {
        let (_, bytes) = crate::moddb::archive("4425", 55847).await.unwrap();
        assert_eq!(
            crate::files::etag(&bytes),
            "8dd4f078919587ccbb0ad37d044f021b7640fc37e86f7ba282a9973f970dac31"
        );
        let stage = tempfile::tempdir().unwrap();
        crate::files::extract_zip(stage.path(), &bytes, 512 * 1024 * 1024).unwrap();
        let raw = std::fs::read(stage.path().join("modinfo.json")).unwrap();
        assert!(raw.starts_with(b"\xef\xbb\xbf"));
        let parsed = parse(&raw).unwrap();
        assert_eq!(parsed["modid"], "vigor");
        assert_eq!(parsed["version"], "122.7.4");
    }
}
