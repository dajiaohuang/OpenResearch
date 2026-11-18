//! Per-project artifacts directory — a plain folder on the user's machine
//! (`<data dir>/files/<project slug>/`). The filesystem is the source of
//! truth: no registry, no upload step. The dashboard's Artifacts tab is an
//! explorer over this folder. Files may live directly at the root or in any
//! user-chosen nested layout; no user-facing filename receives special handling.
//!
//! Serving is contained to the artifacts dir: requested paths are relative
//! (`is_safe_rel_path`) and must still resolve inside it once symlinks are
//! followed (`resolve_contained`), so nothing outside can be listed, read,
//! or deleted through the API.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{anyhow, Result};
use crate::store::data_dir;

use super::model::LocalProject;

/// Files surfaced by the OS that aren't the user's or the agent's.
const IGNORED: &[&str] = &[".DS_Store", "Thumbs.db"];

/// Listing cap — a runaway directory shouldn't stall the 2Hz event loop.
const MAX_ENTRIES: usize = 2000;

/// `<data dir>/files/`. The physical name stays stable for compatibility even
/// though the product surface is called Artifacts. A pre-v0.1.48 `artifacts/`
/// root is still migrated in place when no `files/` root exists; when both
/// exist, `files/` remains authoritative and the legacy root is untouched.
pub(super) fn files_root() -> PathBuf {
    let root = data_dir().join("files");
    let legacy = data_dir().join("artifacts");
    if !root.exists() && legacy.is_dir() {
        // Same filesystem (sibling dirs), so a plain rename; on failure fall
        // through — ensure_dir will create the new root and the legacy dir
        // simply stops being served.
        let _ = std::fs::rename(&legacy, &root);
    }
    root
}

/// `<data dir>/files/<slug>/` — slugs are unique per store and filesystem-safe.
pub fn files_dir(project: &LocalProject) -> PathBuf {
    files_root().join(&project.slug)
}

/// The artifacts dir as the UI sees it, for recognizing artifact paths in chat
/// links. Deliberately NOT canonicalized: the absolute path the agent inlines
/// into the transcript comes from the un-canonicalized `files_dir` (the
/// `{artifacts}` playbook token in `opencode.rs` and report-writing guidance),
/// so the surfaced string must match it byte-for-byte or the UI's prefix match
/// misses on symlinked data dirs (e.g. `/tmp` → `/private/tmp`).
pub fn files_dir_display(project: &LocalProject) -> String {
    files_dir(project).to_string_lossy().into_owned()
}

/// Create the project's artifacts dir if missing and return it.
pub fn ensure_dir(project: &LocalProject) -> Result<PathBuf> {
    let dir = files_dir(project);
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("Could not create {}: {}", dir.display(), e))?;
    Ok(dir)
}

/// What project deletion does with the artifacts folder.
#[must_use]
pub enum ArtifactCleanup {
    Keep,
    PruneIfEmpty(PathBuf),
    DeleteAll(PathBuf),
}

impl ArtifactCleanup {
    pub fn apply(self) -> Result<()> {
        match self {
            Self::Keep => Ok(()),
            Self::PruneIfEmpty(dir) => {
                let _ = std::fs::remove_dir(dir);
                Ok(())
            }
            Self::DeleteAll(dir) => std::fs::remove_dir_all(&dir).map_err(|error| {
                anyhow!("Could not delete artifacts at {}: {error}", dir.display())
            }),
        }
    }
}

pub fn plan_project_artifact_cleanup(
    project: &LocalProject,
    delete_artifacts: bool,
    projects: &[LocalProject],
) -> Result<ArtifactCleanup> {
    plan_artifact_cleanup_at(
        &files_dir(project),
        delete_artifacts,
        projects.iter().map(|project| Path::new(&project.repo_path)),
    )
}

fn plan_artifact_cleanup_at<'a>(
    dir: &Path,
    delete_artifacts: bool,
    repositories: impl Iterator<Item = &'a Path>,
) -> Result<ArtifactCleanup> {
    let metadata = match std::fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ArtifactCleanup::Keep)
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        if !delete_artifacts {
            return Ok(ArtifactCleanup::Keep);
        }
        return Err(anyhow!(
            "The artifacts folder is a symbolic link. Keep artifacts or remove it manually."
        ));
    }
    let canonical_dir = crate::paths::canonicalize(dir)?;
    for repository in repositories {
        let repository = match crate::paths::canonicalize(repository) {
            Ok(repository) => repository,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) if !delete_artifacts => continue,
            Err(error) => return Err(error.into()),
        };
        if repository.starts_with(&canonical_dir) {
            if !delete_artifacts {
                return Ok(ArtifactCleanup::Keep);
            }
            return Err(anyhow!("The artifacts folder contains a project source folder. Keep artifacts to preserve it."));
        }
    }
    Ok(if delete_artifacts {
        ArtifactCleanup::DeleteAll(dir.to_path_buf())
    } else {
        ArtifactCleanup::PruneIfEmpty(dir.to_path_buf())
    })
}

/// Relative, no `..`/`.` segments, no backslashes — a requested path can't
/// escape the artifacts dir. Lexical only; symlink containment is enforced by
/// `resolve_contained`.
pub fn is_safe_rel_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p
            .split('/')
            .any(|seg| seg == ".." || seg == "." || seg.is_empty())
}

/// True when `path` resolves (following symlinks) to a location inside
/// `canonical_base`. Anything that fails to resolve is treated as outside.
fn resolves_inside(canonical_base: &Path, path: &Path) -> bool {
    crate::paths::canonicalize(path)
        .map(|c| c.starts_with(canonical_base))
        .unwrap_or(false)
}

/// Metadata for a listed entry: an escaping symlink yields `None` (skip), a
/// contained symlink its *followed* target metadata (`DirEntry::metadata`
/// never follows links). Shared by the tree walk and the fingerprint so both
/// see exactly the same entries.
fn followed_metadata(
    canonical_base: &Path,
    entry: &std::fs::DirEntry,
) -> Option<std::fs::Metadata> {
    let ft = entry.file_type().ok()?;
    if ft.is_symlink() {
        if !resolves_inside(canonical_base, &entry.path()) {
            return None;
        }
        std::fs::metadata(entry.path()).ok()
    } else {
        entry.metadata().ok()
    }
}

/// Join `rel_path` onto `base` and resolve it, requiring the result to stay
/// inside `base` once every symlink is followed. `is_safe_rel_path` already
/// blocks lexical escapes (`..`); this closes the remaining hole — a symlink
/// inside the dir pointing outside it. Internal symlinks still work.
///
/// Check and use are separate syscalls, so a link swapped in between could
/// still escape — accepted: the API is localhost-only and the artifacts dir is
/// written by the same user it would expose.
fn resolve_contained(base: &Path, rel_path: &str) -> Result<PathBuf> {
    if !is_safe_rel_path(rel_path) {
        return Err(anyhow!("invalid file path: {rel_path}"));
    }
    let canonical_base = crate::paths::canonicalize(base)
        .map_err(|e| anyhow!("Could not resolve {}: {}", base.display(), e))?;
    let path = canonical_base.join(rel_path);
    let canonical = crate::paths::canonicalize(&path)
        .map_err(|e| anyhow!("Could not read {}: {}", path.display(), e))?;
    if !canonical.starts_with(&canonical_base) {
        return Err(anyhow!("path escapes the artifacts directory: {rel_path}"));
    }
    Ok(canonical)
}

/// Copy one explicitly chosen checkout file to the artifact root without
/// replacing an existing entry. The staging file is outside the artifact
/// listing, and the final name appears only after the copy is complete.
type PublishFence<'a> = dyn Fn(&mut dyn FnMut() -> Result<()>) -> Result<()> + 'a;

#[cfg(test)]
fn publish_checkout_file(checkout: &Path, rel_path: &str, artifacts: &Path) -> Result<PathBuf> {
    publish_checkout_file_fenced(checkout, rel_path, artifacts, &|install| install())
}

pub(crate) fn publish_checkout_file_fenced(
    checkout: &Path,
    rel_path: &str,
    artifacts: &Path,
    fence: &PublishFence<'_>,
) -> Result<PathBuf> {
    if Path::new(rel_path).components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(".git")
    }) {
        return Err(anyhow!("cannot publish files under .git"));
    }
    let source = resolve_contained(checkout, rel_path)?;
    let checkout_root = crate::paths::canonicalize(checkout)?;
    if source
        .strip_prefix(&checkout_root)?
        .components()
        .any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(".git")
        })
    {
        return Err(anyhow!("cannot publish files under .git"));
    }
    let name = Path::new(rel_path)
        .file_name()
        .ok_or_else(|| anyhow!("invalid file name"))?;
    let root = crate::paths::canonicalize(artifacts)?;
    let metadata = std::fs::metadata(&source)?;
    if metadata.is_dir() {
        return publish_directory_fenced(&source, &root, name, fence);
    }
    if !metadata.is_file() {
        return Err(anyhow!(
            "only regular files and directories can be published"
        ));
    }
    #[cfg(unix)]
    let mut input = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&source)?
    };
    #[cfg(not(unix))]
    let mut input = std::fs::File::open(&source)?;
    if !input.metadata()?.is_file() {
        return Err(anyhow!(
            "only regular files and directories can be published"
        ));
    }
    publish_reader_fenced(&mut input, &root, name, fence)
}

#[cfg(test)]
fn publish_directory(source: &Path, root: &Path, name: &std::ffi::OsStr) -> Result<PathBuf> {
    publish_directory_fenced(source, root, name, &|install| install())
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
fn publish_directory_fenced(
    source: &Path,
    root: &Path,
    name: &std::ffi::OsStr,
    fence: &PublishFence<'_>,
) -> Result<PathBuf> {
    fn copy_contents(source: &Path, destination: &Path) -> Result<()> {
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink()
                || entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(".git")
            {
                return Err(anyhow!("directory contains a link or .git entry"));
            }
            let target = destination.join(entry.file_name());
            if kind.is_dir() {
                std::fs::create_dir(&target)?;
                copy_contents(&entry.path(), &target)?;
            } else if kind.is_file() {
                std::fs::copy(entry.path(), target)?;
            } else {
                return Err(anyhow!("directory contains a non-regular file"));
            }
        }
        Ok(())
    }
    let parent = root
        .parent()
        .ok_or_else(|| anyhow!("artifact directory has no parent"))?;
    let temp = parent.join(format!(".orx-publish-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temp)?;
    let dest = root.join(name);
    let result = copy_contents(source, &temp).and_then(|()| {
        fence(&mut || {
            install_published_directory(&temp, &dest).map_err(|error| {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
                ) && std::fs::symlink_metadata(&dest).is_ok()
                {
                    std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "artifact name already exists",
                    )
                    .into()
                } else {
                    error.into()
                }
            })
        })
    });
    if temp.exists() {
        std::fs::remove_dir_all(&temp)?;
    }
    result?;
    Ok(dest)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn publish_directory_fenced(
    _source: &Path,
    _root: &Path,
    _name: &std::ffi::OsStr,
    _fence: &PublishFence<'_>,
) -> Result<PathBuf> {
    Err(anyhow!(
        "directory publishing is not available on this platform"
    ))
}

#[cfg(windows)]
fn install_published_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    install_published_file(source, destination)
}

#[cfg(target_os = "linux")]
fn install_published_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let from = std::ffi::CString::new(source.as_os_str().as_bytes())?;
    let to = std::ffi::CString::new(destination.as_os_str().as_bytes())?;
    let code = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if code == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn install_published_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let from = std::ffi::CString::new(source.as_os_str().as_bytes())?;
    let to = std::ffi::CString::new(destination.as_os_str().as_bytes())?;
    let code = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if code == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
fn publish_reader(
    input: &mut impl std::io::Read,
    root: &Path,
    name: &std::ffi::OsStr,
) -> Result<PathBuf> {
    publish_reader_fenced(input, root, name, &|install| install())
}

fn publish_reader_fenced(
    input: &mut impl std::io::Read,
    root: &Path,
    name: &std::ffi::OsStr,
    fence: &PublishFence<'_>,
) -> Result<PathBuf> {
    use std::io::Write as _;
    let dest = root.join(name);
    let parent = root
        .parent()
        .ok_or_else(|| anyhow!("artifact directory has no parent"))?;
    let temp = parent.join(format!(".orx-publish-{}", uuid::Uuid::new_v4()));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| -> Result<()> {
        std::io::copy(input, &mut output)?;
        output.flush()?;
        drop(output);
        fence(&mut || {
            if let Err(error) = install_published_file(&temp, &dest) {
                // Some filesystems report a same-name directory as access denied
                // rather than already-exists. Report a collision only when the
                // destination can actually be observed after the failed move.
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::PermissionDenied
                ) && std::fs::symlink_metadata(&dest).is_ok()
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "artifact name already exists",
                    )
                    .into());
                }
                return Err(error.into());
            }
            Ok(())
        })
    })();
    if let Err(error) = std::fs::remove_file(&temp) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "Could not remove temporary artifact copy {}: {error}",
                temp.display()
            );
        }
    }
    result?;
    Ok(dest)
}

#[cfg(windows)]
fn install_published_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // No REPLACE_EXISTING and no COPY_ALLOWED: refuse existing destinations
    // and cross-volume copies rather than expose an incomplete final file.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn install_published_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::hard_link(source, destination)
}

#[cfg(test)]
mod publish_tests {
    use super::*;

    #[test]
    fn published_file_keeps_bytes_staged_before_another_writer_changes_source() {
        let root =
            std::env::temp_dir().join(format!("orx-publish-change-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        let source = checkout.join("result.txt");
        std::fs::write(&source, b"before").unwrap();
        let published =
            publish_checkout_file_fenced(&checkout, "result.txt", &artifacts, &|install| {
                std::fs::write(&source, b"after")?;
                install()
            })
            .unwrap();
        assert_eq!(std::fs::read(source).unwrap(), b"after");
        assert_eq!(std::fs::read(published).unwrap(), b"before");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn directory_with_fifo_is_rejected_and_staging_removed() {
        use std::os::unix::ffi::OsStrExt;
        let root =
            std::env::temp_dir().join(format!("orx-publish-dir-fifo-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(checkout.join("output")).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        let fifo =
            std::ffi::CString::new(checkout.join("output/pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(publish_checkout_file(&checkout, "output", &artifacts).is_err());
        assert!(!artifacts.join("output").exists());
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".orx-publish-")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn publish_rejects_fifo_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let root = std::env::temp_dir().join(format!("orx-publish-fifo-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        let fifo = std::ffi::CString::new(checkout.join("pipe").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(publish_checkout_file(&checkout, "pipe", &artifacts).is_err());
        assert!(!artifacts.join("pipe").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn directory_publish_rejects_nested_junction() {
        let root =
            std::env::temp_dir().join(format!("orx-publish-junction-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let source = checkout.join("output");
        let outside = root.join("outside");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(outside.join("data.txt"), b"outside").unwrap();
        let junction = source.join("junction");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = publish_checkout_file(&checkout, "output", &artifacts);
        assert!(result.is_err());
        assert!(!artifacts.join("output").exists());
        assert_eq!(std::fs::read(outside.join("data.txt")).unwrap(), b"outside");
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".orx-publish-")));
        std::fs::remove_dir(&junction).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn publish_rejects_junction_into_git_directory() {
        let root = std::env::temp_dir().join(format!("orx-publish-git-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let git = checkout.join(".git");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(git.join("config"), b"metadata").unwrap();
        let junction = checkout.join("alias");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&git)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(publish_checkout_file(&checkout, "alias", &artifacts).is_err());
        assert!(!artifacts.join("alias").exists());
        std::fs::remove_dir(&junction).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn concurrent_directory_publish_preserves_one_complete_tree() {
        use std::sync::{Arc, Barrier};
        let root =
            std::env::temp_dir().join(format!("orx-publish-dir-race-{}", uuid::Uuid::new_v4()));
        let artifacts = root.join("artifacts");
        let first = root.join("first");
        let second = root.join("second");
        for dir in [&artifacts, &first, &second] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(first.join("payload"), vec![1; 4096]).unwrap();
        std::fs::write(second.join("payload"), vec![2; 4096]).unwrap();
        let start = Arc::new(Barrier::new(2));
        let workers: Vec<_> = [first, second]
            .into_iter()
            .map(|source| {
                let artifacts = artifacts.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    publish_directory(&source, &artifacts, std::ffi::OsStr::new("result"))
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let error = results.into_iter().find_map(Result::err).unwrap();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        let payload = std::fs::read(artifacts.join("result/payload")).unwrap();
        assert!(payload == vec![1; 4096] || payload == vec![2; 4096]);
        assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".orx-publish-")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_install_does_not_replace_existing_directory() {
        let root = std::env::temp_dir().join(format!("orx-publish-dir-{}", uuid::Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        let destination = root.join("result");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        std::fs::write(first.join("one"), b"first").unwrap();
        std::fs::write(second.join("two"), b"second").unwrap();
        install_published_file(&first, &destination).unwrap();
        assert!(install_published_file(&second, &destination).is_err());
        assert_eq!(std::fs::read(destination.join("one")).unwrap(), b"first");
        assert_eq!(std::fs::read(second.join("two")).unwrap(), b"second");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_publish_does_not_replace_the_winner() {
        use std::sync::{Arc, Barrier};
        let root = std::env::temp_dir().join(format!("orx-publish-race-{}", uuid::Uuid::new_v4()));
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let start = Arc::new(Barrier::new(2));
        let workers: Vec<_> = [b'1', b'2']
            .into_iter()
            .map(|byte| {
                let artifacts = artifacts.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    publish_reader(
                        &mut std::io::Cursor::new(vec![byte; 4096]),
                        &artifacts,
                        std::ffi::OsStr::new("result"),
                    )
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let bytes = std::fs::read(artifacts.join("result")).unwrap();
        assert!(bytes == vec![b'1'; 4096] || bytes == vec![b'2'; 4096]);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copy_in_progress_is_not_visible_as_an_artifact() {
        struct ObservedReader {
            artifacts: PathBuf,
            consumed: bool,
        }
        impl std::io::Read for ObservedReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                assert_eq!(std::fs::read_dir(&self.artifacts)?.count(), 0);
                if self.consumed {
                    return Ok(0);
                }
                buffer[..3].copy_from_slice(&[0, 255, 42]);
                self.consumed = true;
                Ok(3)
            }
        }
        let root = std::env::temp_dir().join(format!("orx-publish-stage-{}", uuid::Uuid::new_v4()));
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let mut reader = ObservedReader {
            artifacts: artifacts.clone(),
            consumed: false,
        };
        let result =
            publish_reader(&mut reader, &artifacts, std::ffi::OsStr::new("result.zip")).unwrap();
        assert_eq!(std::fs::read(result).unwrap(), [0, 255, 42]);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        struct BrokenReader(bool);
        impl std::io::Read for BrokenReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.0 {
                    return Err(std::io::Error::other("synthetic read failure"));
                }
                self.0 = true;
                buffer[..3].copy_from_slice(&[0, 255, 42]);
                Ok(3)
            }
        }
        assert!(publish_reader(
            &mut BrokenReader(false),
            &artifacts,
            std::ffi::OsStr::new("failed.zip")
        )
        .is_err());
        assert!(!artifacts.join("failed.zip").exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copies_binary_without_overwriting_or_allowing_traversal() {
        let root = std::env::temp_dir().join(format!("orx-publish-test-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let artifacts = root.join("artifacts");
        std::fs::create_dir_all(checkout.join("figures")).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(checkout.join("figures/report.pdf"), [0, 1, 2, 255]).unwrap();
        let dest = publish_checkout_file(&checkout, "figures/report.pdf", &artifacts).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), [0, 1, 2, 255]);
        std::fs::write(checkout.join("figures/report.pdf"), [9]).unwrap();
        assert!(publish_checkout_file(&checkout, "figures/report.pdf", &artifacts).is_err());
        assert_eq!(std::fs::read(dest).unwrap(), [0, 1, 2, 255]);
        std::fs::write(checkout.join("figures/directory"), b"source").unwrap();
        std::fs::create_dir(artifacts.join("directory")).unwrap();
        let error = publish_checkout_file(&checkout, "figures/directory", &artifacts).unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert!(artifacts.join("directory").is_dir());
        assert!(publish_checkout_file(&checkout, "../outside", &artifacts).is_err());
        assert!(publish_checkout_file(&checkout, ".git/config", &artifacts).is_err());
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            let folder = publish_checkout_file(&checkout, "figures", &artifacts).unwrap();
            assert_eq!(std::fs::read(folder.join("report.pdf")).unwrap(), [9]);
            assert!(publish_checkout_file(&checkout, "figures", &artifacts).is_err());
            std::fs::create_dir(checkout.join("figures/.git")).unwrap();
            assert!(publish_checkout_file(&checkout, "figures", &artifacts).is_err());
            assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".orx-publish-")));
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        assert!(publish_checkout_file(&checkout, "figures", &artifacts).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FilePresentation {
    Image,
    Audio,
    Video,
    Pdf,
    Text,
    Unknown,
    Download,
}

impl FilePresentation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Pdf => "pdf",
            Self::Text => "text",
            Self::Unknown => "unknown",
            Self::Download => "download",
        }
    }
}

/// Best-effort content type from a file extension (serving files).
pub fn content_type_for_path(path: &str) -> &'static str {
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "md" | "markdown" | "mdx" => "text/markdown; charset=utf-8",
        "apng" => "image/apng",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "ico" => "image/x-icon",
        "jfif" | "jpg" | "jpeg" => "image/jpeg",
        "jxl" => "image/jxl",
        "pbm" => "image/x-portable-bitmap",
        "pgm" => "image/x-portable-graymap",
        "png" => "image/png",
        "pnm" => "image/x-portable-anymap",
        "ppm" => "image/x-portable-pixmap",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "aac" => "audio/aac",
        "aif" | "aiff" => "audio/aiff",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "mid" | "midi" => "audio/midi",
        "mp3" => "audio/mpeg",
        "oga" | "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "weba" => "audio/webm",
        "3gp" => "video/3gpp",
        "avi" => "video/x-msvideo",
        "m4v" | "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "mpeg" | "mpg" => "video/mpeg",
        "ogv" => "video/ogg",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "eps" | "ps" => "application/postscript",
        "7z" => "application/x-7z-compressed",
        "bz2" => "application/x-bzip2",
        "gz" | "tgz" => "application/gzip",
        "rar" => "application/vnd.rar",
        "tar" => "application/x-tar",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "eot" => "application/vnd.ms-fontobject",
        "otf" => "font/otf",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "json" | "jsonl" | "ipynb" => "application/json",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "txt" | "log" | "rst" | "bib" | "cfg" | "cmake" | "conf" | "dart" | "dockerignore"
        | "editorconfig" | "env" | "ex" | "exs" | "fs" | "fsx" | "gitattributes" | "gitignore"
        | "gitmodules" | "gql" | "graphql" | "gradle" | "hs" | "ini" | "jl" | "lock" | "lua"
        | "m" | "mm" | "nix" | "npmrc" | "properties" | "proto" | "qmd" | "r" | "rmd" | "swift"
        | "tex" => "text/plain; charset=utf-8",
        "htm" | "html" => "text/html; charset=utf-8",
        "xml" => "application/xml",
        "c" | "cc" | "cpp" | "cs" | "go" | "h" | "hpp" | "java" | "kt" | "php" | "pl" | "py"
        | "rb" | "rs" | "scala" | "sh" | "sql" | "toml" | "ts" | "tsx" | "vue" | "yaml" | "yml" => {
            "text/plain; charset=utf-8"
        }
        "css" => "text/css; charset=utf-8",
        "js" | "jsx" | "mjs" => "text/javascript; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn presentation_for_path(path: &str) -> FilePresentation {
    let content_type = content_type_for_path(path);
    if content_type.starts_with("image/") {
        FilePresentation::Image
    } else if content_type.starts_with("audio/") {
        FilePresentation::Audio
    } else if content_type.starts_with("video/") {
        FilePresentation::Video
    } else if content_type == "application/pdf" {
        FilePresentation::Pdf
    } else if content_type.starts_with("text/")
        || matches!(content_type, "application/json" | "application/xml")
        || matches!(
            path.rsplit('/')
                .next()
                .unwrap_or(path)
                .to_ascii_lowercase()
                .as_str(),
            "cargo.lock"
                | "dockerfile"
                | "gemfile"
                | "justfile"
                | "license"
                | "makefile"
                | "readme"
        )
        || path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with(".env"))
    {
        FilePresentation::Text
    } else if content_type != "application/octet-stream" {
        FilePresentation::Download
    } else {
        FilePresentation::Unknown
    }
}

pub fn content_disposition_for_path(path: &str) -> &'static str {
    if matches!(
        presentation_for_path(path),
        FilePresentation::Download | FilePresentation::Unknown
    ) || matches!(
        content_type_for_path(path),
        "application/xml" | "text/html; charset=utf-8"
    ) {
        "attachment"
    } else {
        "inline"
    }
}

/// One node of the artifacts tree: a file or a directory with its children.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEntry {
    pub name: String,
    /// Directory-relative, `/`-joined — the id for read/delete endpoints.
    pub path: String,
    pub is_dir: bool,
    /// 0 for directories.
    pub size: u64,
    pub modified_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<FilePresentation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ArtifactEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactsListing {
    /// Absolute path of the artifacts dir, shown in the UI so the user can
    /// write or drop files into it.
    pub dir: String,
    pub entries: Vec<ArtifactEntry>,
    pub truncated: bool,
}

fn mtime_ms(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(super) fn is_ignored(name: &str) -> bool {
    name.starts_with('.') || IGNORED.contains(&name)
}

/// Recursively build the tree under `dir`, counting nodes against
/// `MAX_ENTRIES`. Returns (children, hit_cap). Symlinks resolving outside
/// `canonical_base` are skipped — the serve endpoints would refuse them.
fn collect_tree(
    canonical_base: &Path,
    dir: &Path,
    rel_prefix: &str,
    seen: &mut usize,
) -> (Vec<ArtifactEntry>, bool) {
    let mut out = Vec::new();
    let mut truncated = false;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (out, false);
    };
    for entry in entries.flatten() {
        if *seen >= MAX_ENTRIES {
            return (out, true);
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_ignored(&name) {
            continue;
        }
        let Some(md) = followed_metadata(canonical_base, &entry) else {
            continue;
        };
        *seen += 1;
        let rel = if rel_prefix.is_empty() {
            name.clone()
        } else {
            format!("{rel_prefix}/{name}")
        };
        if md.is_dir() {
            let (children, hit) = collect_tree(canonical_base, &entry.path(), &rel, seen);
            truncated |= hit;
            out.push(ArtifactEntry {
                name,
                path: rel,
                is_dir: true,
                size: 0,
                modified_at: mtime_ms(&md),
                presentation: None,
                children,
            });
        } else if md.is_file() {
            let presentation = presentation_for_path(&rel);
            out.push(ArtifactEntry {
                name,
                path: rel,
                is_dir: false,
                size: md.len(),
                modified_at: mtime_ms(&md),
                presentation: Some(presentation),
                children: Vec::new(),
            });
        }
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    (out, truncated)
}

/// Scan the artifacts dir (creating it if missing) into a plain file tree.
pub fn list(project: &LocalProject) -> Result<ArtifactsListing> {
    let dir = ensure_dir(project)?;
    let canonical = crate::paths::canonicalize(&dir)
        .map_err(|e| anyhow!("Could not resolve {}: {}", dir.display(), e))?;
    let mut seen = 0;
    let (entries, truncated) = collect_tree(&canonical, &canonical, "", &mut seen);
    Ok(ArtifactsListing {
        dir: dir.to_string_lossy().into_owned(),
        entries,
        truncated,
    })
}

/// A contained artifact path suitable for streaming without buffering.
pub fn file_path(project: &LocalProject, rel_path: &str) -> Result<PathBuf> {
    resolve_contained(&files_dir(project), rel_path)
}

/// Delete a file or folder in the artifacts dir.
///
/// The final component is deleted literally — a symlink is removed, never
/// followed — but every parent segment must resolve inside the artifacts dir, or
/// `a/b` with `a -> /elsewhere` would delete outside it.
pub fn delete_entry(project: &LocalProject, rel_path: &str) -> Result<()> {
    if !is_safe_rel_path(rel_path) {
        return Err(anyhow!("invalid file path: {rel_path}"));
    }
    let base = files_dir(project);
    let parent = match rel_path.rsplit_once('/') {
        Some((parent_rel, _)) => resolve_contained(&base, parent_rel)?,
        None => crate::paths::canonicalize(&base)
            .map_err(|e| anyhow!("Could not resolve {}: {}", base.display(), e))?,
    };
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let path = parent.join(name);
    let md = std::fs::symlink_metadata(&path)
        .map_err(|e| anyhow!("Could not stat {}: {}", path.display(), e))?;
    if md.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    }
    .map_err(|e| anyhow!("Could not delete {}: {}", path.display(), e))
}

/// Cheap change fingerprint (paths + sizes + mtimes) for the SSE diff loop.
/// A missing dir hashes to a stable value, so first creation is a change.
pub fn fingerprint(project: &LocalProject) -> u64 {
    let mut hasher = DefaultHasher::new();
    if let Ok(canonical) = crate::paths::canonicalize(files_dir(project)) {
        hash_dir(&canonical, &canonical, &mut hasher, &mut 0);
    }
    hasher.finish()
}

/// Hash the tree under `dir`, skipping (like `collect_tree`) symlinks that
/// resolve outside `base`, so the fingerprint tracks exactly what's listed.
fn hash_dir(base: &Path, dir: &Path, hasher: &mut DefaultHasher, seen: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *seen >= MAX_ENTRIES {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_ignored(&name) {
            continue;
        }
        let Some(md) = followed_metadata(base, &entry) else {
            continue;
        };
        *seen += 1;
        if let Ok(rel) = entry.path().strip_prefix(base) {
            rel.to_string_lossy().hash(hasher);
        }
        if md.is_dir() {
            hash_dir(base, &entry.path(), hasher, seen);
        } else {
            md.len().hash(hasher);
            mtime_ms(&md).hash(hasher);
        }
    }
}

#[cfg(test)]
mod project_cleanup_tests {
    use super::*;

    fn cleanup_artifacts_at<'a>(
        dir: &Path,
        delete_artifacts: bool,
        repositories: impl Iterator<Item = &'a Path>,
    ) -> Result<()> {
        plan_artifact_cleanup_at(dir, delete_artifacts, repositories)?.apply()
    }

    #[test]
    fn artifacts_require_opt_in_but_empty_folders_are_pruned() {
        let root =
            std::env::temp_dir().join(format!("orx-artifact-cleanup-{}", uuid::Uuid::new_v4()));
        let dir = root.join("artifacts");
        let repo = root.join("repository");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        let repositories = || std::iter::once(repo.as_path());
        std::fs::write(dir.join("report.md"), "keep my report").unwrap();

        cleanup_artifacts_at(&dir, false, repositories()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("report.md")).unwrap(),
            "keep my report"
        );
        cleanup_artifacts_at(&dir, true, repositories()).unwrap();
        assert!(!dir.exists());
        assert!(repo.is_dir());
        cleanup_artifacts_at(&dir, true, repositories()).unwrap();

        std::fs::create_dir(&dir).unwrap();
        cleanup_artifacts_at(&dir, false, repositories()).unwrap();
        assert!(!dir.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeping_artifacts_does_not_require_unrelated_repositories_to_resolve() {
        let root =
            std::env::temp_dir().join(format!("orx-artifact-unresolved-{}", uuid::Uuid::new_v4()));
        let dir = root.join("artifacts");
        std::fs::create_dir_all(&dir).unwrap();
        let repo = root.join("invalid\0repository");
        std::fs::write(dir.join("report.md"), "keep").unwrap();
        cleanup_artifacts_at(&dir, false, std::iter::once(repo.as_path())).unwrap();
        assert!(dir.join("report.md").is_file());
        assert!(cleanup_artifacts_at(&dir, true, std::iter::once(repo.as_path())).is_err());
        assert!(dir.join("report.md").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artifacts_never_remove_a_registered_source_folder() {
        let dir =
            std::env::temp_dir().join(format!("orx-artifact-source-{}", uuid::Uuid::new_v4()));
        let nested_repo = dir.join("another-project");
        std::fs::create_dir_all(&nested_repo).unwrap();
        for repo in [&dir, &nested_repo] {
            cleanup_artifacts_at(&dir, false, std::iter::once(repo.as_path())).unwrap();
            assert!(cleanup_artifacts_at(&dir, true, std::iter::once(repo.as_path())).is_err());
            assert!(repo.is_dir());
        }
        std::fs::remove_dir(&nested_repo).unwrap();
        cleanup_artifacts_at(&dir, false, std::iter::once(dir.as_path())).unwrap();
        assert!(
            dir.is_dir(),
            "even an empty registered source folder must be kept"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn artifact_cleanup_rejects_symlinks_and_keeps_their_targets() {
        let root = std::env::temp_dir().join(format!("orx-artifact-link-{}", uuid::Uuid::new_v4()));
        let target = root.join("target");
        let link = root.join("artifacts");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("report.md"), "safe").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        cleanup_artifacts_at(&link, false, std::iter::empty()).unwrap();
        assert!(cleanup_artifacts_at(&link, true, std::iter::empty()).is_err());
        assert!(target.join("report.md").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// Fresh scratch dir with a `base/` (the artifacts dir under test) and an
    /// `outside/` holding a file symlinks will try to escape to.
    fn scratch() -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("orx-files-{}", uuid::Uuid::new_v4()));
        let base = root.join("base");
        let outside = root.join("outside");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        (root, base, outside)
    }

    #[test]
    fn rejects_lexical_escapes() {
        let (root, base, _) = scratch();
        for bad in ["../x", "/etc/passwd", "a/../b", "a/./b", "", "a\\b"] {
            assert!(resolve_contained(&base, bad).is_err(), "accepted {bad:?}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn blocks_symlink_file_escape() {
        let (root, base, outside) = scratch();
        symlink(outside.join("secret.txt"), base.join("link.txt")).unwrap();
        assert!(resolve_contained(&base, "link.txt").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn blocks_symlink_dir_escape() {
        let (root, base, outside) = scratch();
        symlink(&outside, base.join("sub")).unwrap();
        assert!(resolve_contained(&base, "sub/secret.txt").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn allows_internal_symlink() {
        let (root, base, _) = scratch();
        std::fs::write(base.join("real.txt"), "data").unwrap();
        symlink("real.txt", base.join("alias.txt")).unwrap();
        let resolved = resolve_contained(&base, "alias.txt").unwrap();
        assert_eq!(std::fs::read_to_string(resolved).unwrap(), "data");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn allows_regular_files() {
        let (root, base, _) = scratch();
        std::fs::create_dir(base.join("exp")).unwrap();
        std::fs::write(base.join("exp/report.md"), "# T").unwrap();
        assert!(resolve_contained(&base, "exp/report.md").is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn listing_skips_escaping_symlinks_keeps_internal() {
        let (root, base, outside) = scratch();
        std::fs::write(base.join("real.txt"), "data").unwrap();
        symlink("real.txt", base.join("alias.txt")).unwrap();
        symlink(outside.join("secret.txt"), base.join("leak.txt")).unwrap();
        symlink(&outside, base.join("leakdir")).unwrap();
        let canonical = crate::paths::canonicalize(&base).unwrap();
        let (entries, truncated) = collect_tree(&canonical, &canonical, "", &mut 0);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["alias.txt", "real.txt"]);
        assert!(!truncated);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn listing_gives_project_and_experiment_names_no_special_order() {
        let (root, base, _) = scratch();
        for name in ["project", "baseline", "notes"] {
            std::fs::create_dir(base.join(name)).unwrap();
        }
        std::fs::write(base.join("summary.md"), "# Summary").unwrap();
        let canonical = crate::paths::canonicalize(&base).unwrap();
        let (entries, truncated) = collect_tree(&canonical, &canonical, "", &mut 0);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["baseline", "notes", "project", "summary.md"]);
        assert!(!truncated);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn report_md_is_an_ordinary_nested_file() {
        let (root, base, _) = scratch();
        std::fs::create_dir(base.join("exp")).unwrap();
        std::fs::write(base.join("exp/analysis.md"), "# Analysis").unwrap();
        std::fs::write(base.join("exp/report.md"), "# Report").unwrap();
        let canonical = crate::paths::canonicalize(&base).unwrap();
        let (entries, _) = collect_tree(&canonical, &canonical, "", &mut 0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "exp");
        let names: Vec<&str> = entries[0]
            .children
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["analysis.md", "report.md"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn content_types_cover_browser_media_families() {
        for (path, expected_type, expected_presentation) in [
            ("figure.AVIF", "image/avif", FilePresentation::Image),
            ("figure.jpeg", "image/jpeg", FilePresentation::Image),
            (
                "figure.ppm",
                "image/x-portable-pixmap",
                FilePresentation::Image,
            ),
            ("figure.svg", "image/svg+xml", FilePresentation::Image),
            ("sample.flac", "audio/flac", FilePresentation::Audio),
            ("sample.m4a", "audio/mp4", FilePresentation::Audio),
            ("sample.webm", "video/webm", FilePresentation::Video),
            ("sample.mov", "video/quicktime", FilePresentation::Video),
            ("paper.pdf", "application/pdf", FilePresentation::Pdf),
            (
                "source.rs",
                "text/plain; charset=utf-8",
                FilePresentation::Text,
            ),
            (
                "Makefile",
                "application/octet-stream",
                FilePresentation::Text,
            ),
            (
                ".gitignore",
                "text/plain; charset=utf-8",
                FilePresentation::Text,
            ),
            (
                "Cargo.lock",
                "text/plain; charset=utf-8",
                FilePresentation::Text,
            ),
            (
                ".env.local",
                "application/octet-stream",
                FilePresentation::Text,
            ),
            (
                "archive.bin",
                "application/octet-stream",
                FilePresentation::Unknown,
            ),
            (
                "drawing.eps",
                "application/postscript",
                FilePresentation::Download,
            ),
        ] {
            assert_eq!(
                content_type_for_path(path),
                expected_type,
                "wrong type for {path}"
            );
            assert_eq!(presentation_for_path(path), expected_presentation);
        }
        assert_eq!(content_disposition_for_path("page.html"), "attachment");
        assert_eq!(content_disposition_for_path("figure.svg"), "inline");
    }
}
