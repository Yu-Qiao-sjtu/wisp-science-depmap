//! Read-only package browsing, independent of the agent's enabled-skill filter.
use std::io::Read;
use std::path::{Component, Path};

const MAX_FILES: usize = 2000;
const MAX_TEXT_BYTES: u64 = 1024 * 1024;

pub fn list_skill_files(root: &Path) -> Result<Vec<String>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !entry.file_name().to_string_lossy().starts_with('.')
        })
    {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.depth() > 32 {
            return Err("Skill package exceeds the browsing depth limit (32).".into());
        }
        if entry.file_type().is_file() {
            let relative = entry
                .path()
                .strip_prefix(&root)
                .map_err(|e| e.to_string())?;
            files.push(relative.to_string_lossy().replace('\\', "/"));
            if files.len() > MAX_FILES {
                return Err("Skill package has too many files to browse (limit: 2000).".into());
            }
        }
    }
    files.sort_by(|a, b| (a != "SKILL.md", a).cmp(&(b != "SKILL.md", b)));
    Ok(files)
}

pub fn read_skill_file(root: &Path, relative: &str) -> Result<String, String> {
    // Check both separators on every platform, including Windows drive/ADS syntax.
    if relative.is_empty() || relative.contains(['\\', ':']) {
        return Err("Invalid skill file path.".into());
    }
    let path = Path::new(relative);
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Invalid skill file path.".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let target = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    if !target.starts_with(&root) {
        return Err("File is outside this skill package.".into());
    }
    if !target.is_file() {
        return Err("Select a regular skill file.".into());
    }
    let file = std::fs::File::open(&target).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Select a regular skill file.".into());
    }
    if metadata.len() > MAX_TEXT_BYTES {
        return Err("File is too large to preview (limit: 1 MiB).".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_TEXT_BYTES {
        return Err("File is too large to preview (limit: 1 MiB).".into());
    }
    if bytes.contains(&0) {
        return Err("Binary files cannot be previewed as text.".into());
    }
    String::from_utf8(bytes)
        .map_err(|_| "This file is not UTF-8 text and cannot be previewed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Package(std::path::PathBuf);
    impl Package {
        fn new() -> Self {
            static NEXT_PACKAGE: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "wisp-skill-browse-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_PACKAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(path.join("scripts/nested")).unwrap();
            Self(path)
        }
    }
    impl Drop for Package {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn browses_full_markdown_and_nested_scripts() {
        let package = Package::new();
        let markdown = "---\nname: demo\ndescription: Demo\n---\n# Demo";
        std::fs::write(package.0.join("SKILL.md"), markdown).unwrap();
        std::fs::write(package.0.join("scripts/nested/分析.py"), "print('分析')").unwrap();
        std::fs::create_dir(package.0.join(".git")).unwrap();
        std::fs::write(package.0.join(".git/config"), "hidden").unwrap();
        assert_eq!(
            list_skill_files(&package.0).unwrap(),
            ["SKILL.md", "scripts/nested/分析.py"]
        );
        assert_eq!(read_skill_file(&package.0, "SKILL.md").unwrap(), markdown);
        assert_eq!(
            read_skill_file(&package.0, "scripts/nested/分析.py").unwrap(),
            "print('分析')"
        );
    }
    #[test]
    fn rejects_unsafe_paths_and_unpreviewable_files() {
        let package = Package::new();
        for path in [
            "../outside",
            "/etc/passwd",
            "C:/secret",
            "scripts\\..\\secret",
            "file:stream",
            "",
        ] {
            assert!(read_skill_file(&package.0, path).is_err(), "{path}");
        }
        std::fs::write(package.0.join("binary"), [0, 1, 2]).unwrap();
        std::fs::write(package.0.join("invalid"), [0xff]).unwrap();
        std::fs::write(
            package.0.join("large"),
            vec![b'a'; MAX_TEXT_BYTES as usize + 1],
        )
        .unwrap();
        for path in ["binary", "invalid", "large", "missing", "scripts"] {
            assert!(read_skill_file(&package.0, path).is_err(), "{path}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn symlink_cannot_escape_package() {
        let package = Package::new();
        let outside = Package::new();
        std::fs::write(outside.0.join("secret"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.0.join("secret"), package.0.join("link")).unwrap();
        assert!(list_skill_files(&package.0).unwrap().is_empty());
        assert!(read_skill_file(&package.0, "link").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_junction_cannot_escape_package() {
        let package = Package::new();
        let outside = Package::new();
        std::fs::write(outside.0.join("secret"), "secret").unwrap();
        // Directory junctions do not require developer mode or symlink privileges.
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(package.0.join("linked"))
            .arg(&outside.0)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(list_skill_files(&package.0).unwrap().is_empty());
        assert!(read_skill_file(&package.0, "linked/secret").is_err());
    }
}
