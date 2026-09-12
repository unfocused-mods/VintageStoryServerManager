use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub fn confined(root: &Path, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    anyhow::ensure!(
        !p.is_absolute() && !relative.contains(['\\', ':', '\0']),
        "Invalid relative path"
    );
    let root = root.canonicalize()?;
    let mut current = root.clone();
    for component in p.components() {
        match component {
            Component::Normal(name) => {
                current.push(name);
                if let Ok(meta) = std::fs::symlink_metadata(&current) {
                    anyhow::ensure!(!meta.file_type().is_symlink(), "Symlinks are not allowed");
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        anyhow::ensure!(
                            meta.file_attributes() & 0x400 == 0,
                            "Reparse points are not allowed"
                        );
                    }
                    anyhow::ensure!(
                        current.canonicalize()?.starts_with(&root),
                        "Path escapes managed directory"
                    );
                }
            }
            Component::CurDir => {}
            _ => anyhow::bail!("Path traversal is not allowed"),
        }
    }
    Ok(current)
}
#[derive(Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub directory: bool,
    pub bytes: u64,
}
pub fn list(root: &Path, path: &str) -> Result<Vec<FileEntry>> {
    let mut entries = vec![];
    for e in std::fs::read_dir(confined(root, path)?)? {
        let e = e?;
        let m = e.file_type()?;
        if m.is_symlink() {
            continue;
        }
        entries.push(FileEntry {
            name: e.file_name().to_string_lossy().into(),
            directory: m.is_dir(),
            bytes: e.metadata()?.len(),
        });
    }
    entries.sort_by(|a, b| b.directory.cmp(&a.directory).then(a.name.cmp(&b.name)));
    Ok(entries)
}
pub fn etag(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
/// Read a bounded regular file. Linux resolves the entire path in the kernel,
/// so replacing an intermediate directory with a symlink cannot escape root.
pub fn read(root: &Path, path: &str, limit: u64) -> Result<Vec<u8>> {
    let checked = confined(root, path)?;
    #[cfg(target_os = "linux")]
    let file = {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::fs::OpenOptionsExt;
        let dir = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)?;
        let relative = std::ffi::CString::new(path)?;
        #[repr(C)]
        struct OpenHow {
            flags: u64,
            mode: u64,
            resolve: u64,
        }
        // RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS (includes NO_MAGICLINKS).
        let how = OpenHow {
            flags: (libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK) as u64,
            mode: 0,
            resolve: 0x08 | 0x04,
        };
        // SAFETY: all pointers refer to live, correctly sized values. A successful
        // descriptor is immediately owned by File; errors never create a File.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_openat2,
                dir.as_raw_fd(),
                relative.as_ptr(),
                &how,
                std::mem::size_of::<OpenHow>(),
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let _ = checked;
        unsafe { std::fs::File::from_raw_fd(fd as i32) }
    };
    #[cfg(not(target_os = "linux"))]
    let file = std::fs::File::open(checked)?;
    anyhow::ensure!(file.metadata()?.is_file(), "Only regular files can be read");
    anyhow::ensure!(
        file.metadata()?.len() <= limit,
        "File exceeds transfer limit"
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit,
        "File grew beyond transfer limit"
    );
    Ok(bytes)
}
pub fn write(
    root: &Path,
    path: &str,
    data: &[u8],
    expected: Option<&str>,
    running: bool,
) -> Result<String> {
    anyhow::ensure!(!path.is_empty(), "A file path is required");
    anyhow::ensure!(
        !(running && (path.to_lowercase().contains("saves/") || path.ends_with(".vcdbs"))),
        "Stop the server before changing world files"
    );
    let dest = confined(root, path)?;
    if dest.exists() {
        let old = std::fs::read(&dest)?;
        anyhow::ensure!(
            expected == Some(etag(&old).as_str()),
            "File changed or If-Match missing; reload before saving"
        );
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension(format!("vsm-{}", uuid::Uuid::new_v4()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, &dest).context("Could not atomically replace file")?;
    Ok(etag(data))
}
pub fn extract_zip(root: &Path, data: &[u8], limit: u64) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data))?;
    let mut total = 0;
    anyhow::ensure!(archive.len() <= 10000, "Too many archive entries");
    for i in 0..archive.len() {
        let f = archive.by_index(i)?;
        total += f.size();
        anyhow::ensure!(total <= limit, "Archive exceeds extraction limit");
        anyhow::ensure!(
            f.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "Archive symlinks are forbidden"
        );
        zip_entry_path(root, f.name())?;
    }
    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let dest = zip_entry_path(root, f.name())?;
        if f.is_dir() {
            std::fs::create_dir_all(dest)?;
        } else {
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dest)?;
            std::io::copy(&mut f.by_ref().take(limit + 1), &mut out)?;
        }
    }
    Ok(())
}
fn zip_entry_path(root: &Path, name: &str) -> Result<PathBuf> {
    confined(root, name).with_context(|| {
        format!(
            "Unsafe ZIP entry {:?}",
            name.chars().take(180).collect::<String>()
        )
    })
}
pub fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        anyhow::ensure!(!ty.is_symlink(), "Recovery cannot include symlinks");
        let target = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escape_and_stale_edits() {
        let root = tempfile::tempdir().unwrap();
        for p in [
            "../private",
            "/etc/passwd",
            "C:/secret",
            "a/../../b",
            "a\\b",
        ] {
            assert!(confined(root.path(), p).is_err(), "{p}");
        }
        let etag = write(root.path(), "config.json", b"{}", None, false).unwrap();
        assert!(write(root.path(), "config.json", b"[]", None, false).is_err());
        assert!(write(root.path(), "config.json", b"[]", Some(&etag), false).is_ok());
        assert!(write(root.path(), "Saves/world.vcdbs", b"bad", None, true).is_err());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;
    #[test]
    fn confined_reads_reject_links_and_oversized_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("ok"), b"hello").unwrap();
        assert_eq!(read(root.path(), "ok", 5).unwrap(), b"hello");
        assert!(read(root.path(), "ok", 4).is_err());
        std::os::unix::fs::symlink("/etc", root.path().join("escape")).unwrap();
        assert!(read(root.path(), "escape/passwd", 10000).is_err());
        assert!(read(root.path(), "../outside", 100).is_err());
    }
}
