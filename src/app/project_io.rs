//! Native recipe-file transactions. The authoring session is owned by the caller.

use super::project_file::{
    decode_project, encode_project, ProjectFile, ProjectFileError, MAX_PROJECT_FILE_BYTES,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Hash of raw disk bytes, independent of the canonical recipe identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DiskStamp([u8; 32]);
/// Validated recipe and the exact disk version it was loaded from.
// T5 connects these native entry points; remove the local allowances on integration.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
pub(super) struct LoadedProject {
    pub file: ProjectFile,
    pub disk_stamp: DiskStamp,
}
/// A save is allowed only for an absent file or this explicitly captured disk version.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub(super) enum SaveExpectation {
    NewFile,
    Unchanged(DiskStamp),
    ConfirmedOverwrite(DiskStamp),
}
/// Returned only after the saved bytes and any old-recipe backup were verified.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
pub(super) struct SaveReceipt {
    pub path: PathBuf,
    pub disk_stamp: DiskStamp,
    pub backup: Option<PathBuf>,
}
/// Operation failures retain the original cause and paths checked as readable on return.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, thiserror::Error)]
pub(super) enum ProjectIoError {
    #[error("disk conflict")]
    Conflict { recovery_paths: Vec<PathBuf> },
    #[error("read: {source}")]
    Read {
        source: io::Error,
        recovery_paths: Vec<PathBuf>,
    },
    #[error("write: {source}")]
    Write {
        source: io::Error,
        recovery_paths: Vec<PathBuf>,
    },
    #[error("sync: {source}")]
    Sync {
        source: io::Error,
        recovery_paths: Vec<PathBuf>,
    },
    #[error("replace: {source}")]
    Replace {
        source: io::Error,
        recovery_paths: Vec<PathBuf>,
    },
    #[error("format: {source}")]
    Format {
        source: ProjectFileError,
        recovery_paths: Vec<PathBuf>,
    },
}
impl ProjectIoError {
    /// Files readable when the error was constructed; third-party changes can occur later.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn recovery_paths(&self) -> &[PathBuf] {
        match self {
            Self::Conflict { recovery_paths }
            | Self::Read { recovery_paths, .. }
            | Self::Write { recovery_paths, .. }
            | Self::Sync { recovery_paths, .. }
            | Self::Replace { recovery_paths, .. }
            | Self::Format { recovery_paths, .. } => recovery_paths,
        }
    }
}
fn stamp(bytes: &[u8]) -> DiskStamp {
    DiskStamp(*blake3::hash(bytes).as_bytes())
}

fn readable_paths(paths: &[&Path]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|path| read_bytes(path).is_ok())
        .map(|path| path.to_path_buf())
        .collect()
}

// Limit allocation before the codec's own authoritative size check. The extra byte
// lets it classify an oversized file as Format(Limit), preserving the codec cause.
fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take((MAX_PROJECT_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Reads and validates `path` without changing any caller-owned session.
/// Returns Read for filesystem failures or Format with the unchanged codec cause.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn read_project(path: &Path) -> Result<LoadedProject, ProjectIoError> {
    let bytes = read_bytes(path).map_err(|source| ProjectIoError::Read {
        source,
        recovery_paths: readable_paths(&[path]),
    })?;
    let file = decode_project(&bytes).map_err(|source| ProjectIoError::Format {
        source,
        recovery_paths: readable_paths(&[path]),
    })?;
    Ok(LoadedProject {
        file,
        disk_stamp: stamp(&bytes),
    })
}

fn check_expected(
    path: &Path,
    expected: SaveExpectation,
    candidates: &[&Path],
) -> Result<Option<Vec<u8>>, ProjectIoError> {
    if matches!(expected, SaveExpectation::NewFile) {
        return match fs::symlink_metadata(path) {
            Ok(_) => Err(ProjectIoError::Conflict {
                recovery_paths: readable_paths(candidates),
            }),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ProjectIoError::Read {
                source,
                recovery_paths: readable_paths(candidates),
            }),
        };
    }
    let bytes = match read_bytes(path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(ProjectIoError::Conflict {
                recovery_paths: readable_paths(candidates),
            })
        }
        Err(source) => {
            return Err(ProjectIoError::Read {
                source,
                recovery_paths: readable_paths(candidates),
            })
        }
    };
    let (SaveExpectation::Unchanged(expected_stamp)
    | SaveExpectation::ConfirmedOverwrite(expected_stamp)) = expected
    else {
        unreachable!("NewFile handled above")
    };
    if bytes.len() > MAX_PROJECT_FILE_BYTES || stamp(&bytes) != expected_stamp {
        return Err(ProjectIoError::Conflict {
            recovery_paths: readable_paths(candidates),
        });
    }
    Ok(Some(bytes))
}

fn create_sibling(path: &Path, role: &str) -> io::Result<(PathBuf, File)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    loop {
        let name = format!(
            ".sekai-{}-{}-{role}.json",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let candidate = parent.join(name);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(source),
        }
    }
}

fn verify_bytes(path: &Path, expected: &[u8]) -> io::Result<()> {
    if read_bytes(path)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved file checksum mismatch",
        ));
    }
    Ok(())
}

/// Saves a captured recipe under `expected`, retaining a same-directory old-file backup.
/// Failures report Conflict, Format, Read/Write/Sync/Replace and actual readable paths.
/// The final stamp check protects its check window, not arbitrary concurrent writers
/// through cross-process CAS; synchronization does not promise absolute power-loss safety.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn save_project(
    path: &Path,
    file: &ProjectFile,
    expected: SaveExpectation,
) -> Result<SaveReceipt, ProjectIoError> {
    let bytes = encode_project(file).map_err(|source| ProjectIoError::Format {
        source,
        recovery_paths: readable_paths(&[path]),
    })?;
    let old = check_expected(path, expected, &[path])?;
    let (temporary, mut writer) =
        create_sibling(path, "pending").map_err(|source| ProjectIoError::Write {
            source,
            recovery_paths: readable_paths(&[path]),
        })?;
    let candidates = [path, temporary.as_path()];
    #[cfg(test)]
    if tests::take_failure(|point| matches!(point, tests::FailPoint::Write)).is_some() {
        writer
            .write_all(&bytes[..bytes.len() / 2])
            .expect("test partial write");
        return Err(ProjectIoError::Write {
            source: io::Error::other("injected partial write failure"),
            recovery_paths: readable_paths(&candidates),
        });
    }
    writer
        .write_all(&bytes)
        .map_err(|source| ProjectIoError::Write {
            source,
            recovery_paths: readable_paths(&candidates),
        })?;
    #[cfg(test)]
    if tests::take_failure(|point| matches!(point, tests::FailPoint::Sync)).is_some() {
        return Err(ProjectIoError::Sync {
            source: io::Error::other("injected sync failure"),
            recovery_paths: readable_paths(&candidates),
        });
    }
    writer.sync_all().map_err(|source| ProjectIoError::Sync {
        source,
        recovery_paths: readable_paths(&candidates),
    })?;
    drop(writer); // ReplaceFileW requires the replacement handle to be closed.
    #[cfg(test)]
    if tests::take_failure(|point| matches!(point, tests::FailPoint::TempChecksum)).is_some() {
        fs::write(&temporary, b"damaged temporary").expect("test temporary mutation");
    }
    verify_bytes(&temporary, &bytes).map_err(|source| ProjectIoError::Read {
        source,
        recovery_paths: readable_paths(&candidates),
    })?;
    // Pre-reserve and sync a backup with old bytes. ReplaceFileW also writes the old
    // target here; even rare partial-replacement errors have a recoverable old copy.
    let backup = if let Some(old) = &old {
        let (backup, mut writer) =
            create_sibling(path, "backup").map_err(|source| ProjectIoError::Write {
                source,
                recovery_paths: readable_paths(&candidates),
            })?;
        let all = [path, temporary.as_path(), backup.as_path()];
        writer
            .write_all(old)
            .map_err(|source| ProjectIoError::Write {
                source,
                recovery_paths: readable_paths(&all),
            })?;
        writer.sync_all().map_err(|source| ProjectIoError::Sync {
            source,
            recovery_paths: readable_paths(&all),
        })?;
        drop(writer);
        verify_bytes(&backup, old).map_err(|source| ProjectIoError::Read {
            source,
            recovery_paths: readable_paths(&all),
        })?;
        Some(backup)
    } else {
        None
    };
    let mut all = vec![path, temporary.as_path()];
    if let Some(backup) = &backup {
        all.push(backup);
    }
    #[cfg(test)]
    if let Some(tests::FailPoint::LateExternal(external)) =
        tests::take_failure(|point| matches!(point, tests::FailPoint::LateExternal(_)))
    {
        fs::write(path, external).expect("test external edit");
    }
    // No production file mutation occurs between this second check and replacement.
    check_expected(path, expected, &all)?;
    #[cfg(test)]
    if let Some(tests::FailPoint::FirstRace(external)) =
        tests::take_failure(|point| matches!(point, tests::FailPoint::FirstRace(_)))
    {
        fs::write(path, external).expect("test external first-file race");
    }
    if let Err(source) = replace_file(path, &temporary, backup.as_deref()) {
        if old.is_none() && source.kind() == io::ErrorKind::AlreadyExists {
            return Err(ProjectIoError::Conflict {
                recovery_paths: readable_paths(&all),
            });
        }
        return Err(ProjectIoError::Replace {
            source,
            recovery_paths: readable_paths(&all),
        });
    }
    verify_bytes(path, &bytes).map_err(|source| ProjectIoError::Replace {
        source,
        recovery_paths: readable_paths(&all),
    })?;
    if let (Some(old), Some(backup)) = (&old, &backup) {
        verify_bytes(backup, old).map_err(|source| ProjectIoError::Replace {
            source,
            recovery_paths: readable_paths(&all),
        })?;
    }
    Ok(SaveReceipt {
        path: path.to_path_buf(),
        disk_stamp: stamp(&bytes),
        backup,
    })
}

fn replace_file(path: &Path, temporary: &Path, backup: Option<&Path>) -> io::Result<()> {
    #[cfg(test)]
    if tests::take_failure(|point| matches!(point, tests::FailPoint::AfterBackupMove)).is_some() {
        let backup = backup.expect("replacement test has a backup");
        fs::remove_file(backup).expect("remove test backup reservation");
        fs::rename(path, backup).expect("move old file to test backup");
        return Err(io::Error::other("injected failure after old-file move"));
    }
    platform_replace_file(path, temporary, backup)
}

#[cfg(windows)]
fn platform_replace_file(path: &Path, temporary: &Path, backup: Option<&Path>) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, ReplaceFileW};
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut encoded: Vec<_> = path.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "NUL in file path",
            ));
        }
        encoded.push(0);
        Ok(encoded)
    }
    let path = wide(path)?;
    let temporary = wide(temporary)?;
    let backup = backup.map(wide).transpose()?;
    // Partial failures can move the old file to backup; the caller inspects all paths.
    // https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew
    // SAFETY: owned UTF-16 buffers are NUL-terminated, have no interior NUL, and
    // remain live for the synchronous calls. Reserved pointers are null. Neither
    // API retains the pointers. Flags zero preserve ACL errors and prevent first-save overwrite.
    let result = unsafe {
        if let Some(backup) = &backup {
            ReplaceFileW(
                path.as_ptr(),
                temporary.as_ptr(),
                backup.as_ptr(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(temporary.as_ptr(), path.as_ptr(), 0)
        }
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn platform_replace_file(path: &Path, temporary: &Path, backup: Option<&Path>) -> io::Result<()> {
    if backup.is_some() {
        fs::rename(temporary, path)
    } else {
        // rename on Unix can overwrite. A same-directory hard link is no-clobber.
        fs::hard_link(temporary, path)?;
        fs::remove_file(temporary)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{authoring::AuthorInputs, project_file::encode_project};
    use super::*;
    use std::{
        cell::RefCell,
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    pub(super) enum FailPoint {
        Write,
        Sync,
        TempChecksum,
        LateExternal(Vec<u8>),
        FirstRace(Vec<u8>),
        AfterBackupMove,
    }
    thread_local! { static FAILURE: RefCell<Option<FailPoint>> = const { RefCell::new(None) }; }
    struct InjectFailure;
    impl InjectFailure {
        fn new(point: FailPoint) -> Self {
            FAILURE.with(|slot| *slot.borrow_mut() = Some(point));
            Self
        }
    }
    impl Drop for InjectFailure {
        fn drop(&mut self) {
            FAILURE.with(|slot| *slot.borrow_mut() = None);
        }
    }
    pub(super) fn take_failure(predicate: impl FnOnce(&FailPoint) -> bool) -> Option<FailPoint> {
        FAILURE.with(|slot| slot.borrow_mut().take_if(|point| predicate(point)))
    }

    // Only real small files can prove sharing, replacement, and recovery-path behavior.
    struct TestDir {
        root: PathBuf,
        parent: PathBuf,
    }
    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let parent = std::env::temp_dir().canonicalize().unwrap();
            let root = parent.join(format!(
                "sekai-project-io-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self {
                root: root.canonicalize().unwrap(),
                parent,
            }
        }
        fn path(&self) -> PathBuf {
            self.root.join("中文 项目.sekai.json")
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            // Canonical direct-child ownership is checked before recursive test cleanup.
            assert_eq!(self.root.parent(), Some(self.parent.as_path()));
            assert!(self
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("sekai-project-io-"));
            assert_eq!(fs::canonicalize(&self.root).unwrap(), self.root);
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn project(label: &str) -> ProjectFile {
        let mut file = ProjectFile::new(AuthorInputs::default()).unwrap();
        file.extensions.insert("name".into(), label.into());
        file
    }
    fn assert_recovery(error: &ProjectIoError, expected: &[u8]) {
        // Read every reported path, including paths after the first content match.
        let recovered: Vec<_> = error
            .recovery_paths()
            .iter()
            .map(|path| fs::read(path).unwrap())
            .collect();
        assert!(
            recovered.iter().any(|bytes| bytes == expected),
            "missing readable recovery: {error:?}"
        );
    }
    #[test]
    fn first_save_reads_back_exact_bytes_on_unicode_path() {
        let dir = TestDir::new();
        let path = dir.path();
        let file = project("first");
        let receipt = save_project(&path, &file, SaveExpectation::NewFile).unwrap();
        assert_eq!(receipt.path, path);
        assert!(receipt.backup.is_none());
        assert_eq!(fs::read(&path).unwrap(), encode_project(&file).unwrap());
        let loaded = read_project(&path).unwrap();
        assert_eq!(loaded.file, file);
        assert_eq!(loaded.disk_stamp, receipt.disk_stamp);
    }
    #[test]
    fn new_file_never_overwrites_existing_file() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = encode_project(&project("old")).unwrap();
        fs::write(&path, &old).unwrap();
        let error = save_project(&path, &project("new"), SaveExpectation::NewFile).unwrap_err();
        assert!(matches!(error, ProjectIoError::Conflict { .. }));
        assert_eq!(fs::read(&path).unwrap(), old);
        assert_recovery(&error, &old);
    }
    #[test]
    fn changed_raw_bytes_conflict_even_when_recipe_is_identical() {
        let dir = TestDir::new();
        let path = dir.path();
        let receipt = save_project(&path, &project("old"), SaveExpectation::NewFile).unwrap();
        let mut external = fs::read(&path).unwrap();
        external.push(b'\n');
        fs::write(&path, &external).unwrap();
        let error = save_project(
            &path,
            &project("new"),
            SaveExpectation::Unchanged(receipt.disk_stamp),
        )
        .unwrap_err();
        assert!(matches!(error, ProjectIoError::Conflict { .. }));
        assert_eq!(fs::read(&path).unwrap(), external);
        assert_recovery(&error, &external);
    }
    #[test]
    fn successful_replacement_retains_old_recipe_backup() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let new = project("new");
        let initial = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let receipt =
            save_project(&path, &new, SaveExpectation::Unchanged(initial.disk_stamp)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), encode_project(&new).unwrap());
        assert_eq!(
            fs::read(receipt.backup.unwrap()).unwrap(),
            encode_project(&old).unwrap()
        );
        assert_eq!(read_project(&path).unwrap().disk_stamp, receipt.disk_stamp);
    }
    #[test]
    fn confirmed_overwrite_is_bound_to_captured_disk_bytes() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let new = project("new");
        let first = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let receipt = save_project(
            &path,
            &new,
            SaveExpectation::ConfirmedOverwrite(first.disk_stamp),
        )
        .unwrap();
        let error = save_project(
            &path,
            &old,
            SaveExpectation::ConfirmedOverwrite(first.disk_stamp),
        )
        .unwrap_err();
        assert!(matches!(error, ProjectIoError::Conflict { .. }));
        assert_eq!(read_project(&path).unwrap().disk_stamp, receipt.disk_stamp);
    }
    #[test]
    fn damaged_read_preserves_file_and_format_reason() {
        let dir = TestDir::new();
        let path = dir.path();
        fs::write(&path, b"{").unwrap();
        let error = read_project(&path).unwrap_err();
        assert!(matches!(
            &error,
            ProjectIoError::Format {
                source: ProjectFileError::Format(_),
                ..
            }
        ));
        assert_eq!(fs::read(&path).unwrap(), b"{");
        assert_recovery(&error, b"{");
    }
    #[test]
    fn missing_parent_write_failure_preserves_original() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = encode_project(&project("old")).unwrap();
        fs::write(&path, &old).unwrap();
        let error = save_project(
            &dir.root.join("missing").join("new.json"),
            &project("new"),
            SaveExpectation::NewFile,
        )
        .unwrap_err();
        assert!(matches!(error, ProjectIoError::Write { .. }));
        assert_eq!(fs::read(&path).unwrap(), old);
    }
    #[test]
    fn partial_write_failure_preserves_original_and_reports_actual_files() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let first = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let _injection = InjectFailure::new(FailPoint::Write);
        let error = save_project(
            &path,
            &project("new"),
            SaveExpectation::Unchanged(first.disk_stamp),
        )
        .unwrap_err();
        assert!(matches!(error, ProjectIoError::Write { .. }));
        assert_recovery(&error, &encode_project(&old).unwrap());
        assert_eq!(fs::read(&path).unwrap(), encode_project(&old).unwrap());
    }
    #[test]
    fn sync_failure_preserves_old_and_complete_new_recipe() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let new = project("new");
        let first = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let _injection = InjectFailure::new(FailPoint::Sync);
        let error =
            save_project(&path, &new, SaveExpectation::Unchanged(first.disk_stamp)).unwrap_err();
        assert!(matches!(error, ProjectIoError::Sync { .. }));
        assert_recovery(&error, &encode_project(&old).unwrap());
        assert_recovery(&error, &encode_project(&new).unwrap());
    }
    #[test]
    fn temporary_checksum_failure_never_replaces_original() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let first = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let _injection = InjectFailure::new(FailPoint::TempChecksum);
        let error = save_project(
            &path,
            &project("new"),
            SaveExpectation::Unchanged(first.disk_stamp),
        )
        .unwrap_err();
        assert!(matches!(error, ProjectIoError::Read { .. }));
        assert_eq!(fs::read(&path).unwrap(), encode_project(&old).unwrap());
        assert_recovery(&error, &encode_project(&old).unwrap());
    }
    #[test]
    fn late_external_edit_is_checked_immediately_before_replacement() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let external = encode_project(&project("external")).unwrap();
        let first = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let _injection = InjectFailure::new(FailPoint::LateExternal(external.clone()));
        let error = save_project(
            &path,
            &project("new"),
            SaveExpectation::ConfirmedOverwrite(first.disk_stamp),
        )
        .unwrap_err();
        assert!(matches!(error, ProjectIoError::Conflict { .. }));
        assert_eq!(fs::read(&path).unwrap(), external);
        assert_recovery(&error, &external);
    }
    #[test]
    fn first_save_race_uses_platform_no_overwrite_operation() {
        let dir = TestDir::new();
        let path = dir.path();
        let external = encode_project(&project("external")).unwrap();
        let _injection = InjectFailure::new(FailPoint::FirstRace(external.clone()));
        let error = save_project(&path, &project("new"), SaveExpectation::NewFile).unwrap_err();
        assert!(matches!(error, ProjectIoError::Conflict { .. }));
        assert_eq!(fs::read(&path).unwrap(), external);
        assert_recovery(&error, &external);
    }
    #[test]
    fn replacement_error_after_old_file_moves_reports_real_backup_and_pending() {
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let new = project("new");
        let first = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let _injection = InjectFailure::new(FailPoint::AfterBackupMove);
        let error =
            save_project(&path, &new, SaveExpectation::Unchanged(first.disk_stamp)).unwrap_err();
        assert!(matches!(error, ProjectIoError::Replace { .. }));
        assert!(!path.exists());
        assert!(!error.recovery_paths().contains(&path));
        assert_recovery(&error, &encode_project(&old).unwrap());
        assert_recovery(&error, &encode_project(&new).unwrap());
    }
    #[test]
    fn oversized_read_is_bounded_and_retains_codec_limit_reason() {
        let dir = TestDir::new();
        let path = dir.path();
        fs::write(&path, vec![b' '; MAX_PROJECT_FILE_BYTES + 2]).unwrap();
        let error = read_project(&path).unwrap_err();
        assert!(matches!(
            error,
            ProjectIoError::Format {
                source: ProjectFileError::Limit(_),
                ..
            }
        ));
    }
    #[test]
    fn missing_file_read_is_a_read_error() {
        let dir = TestDir::new();
        let error = read_project(&dir.path()).unwrap_err();
        assert!(
            matches!(&error, ProjectIoError::Read { source, .. } if source.kind() == io::ErrorKind::NotFound)
        );
        assert!(error.recovery_paths().is_empty());
    }
    #[cfg(windows)]
    #[test]
    fn windows_real_sharing_violation_reports_readable_old_and_new_recipes() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        let dir = TestDir::new();
        let path = dir.path();
        let old = project("old");
        let new = project("new");
        let first = save_project(&path, &old, SaveExpectation::NewFile).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        let error =
            save_project(&path, &new, SaveExpectation::Unchanged(first.disk_stamp)).unwrap_err();
        assert!(
            matches!(&error, ProjectIoError::Replace { source, .. } if source.raw_os_error() == Some(32))
        );
        assert_recovery(&error, &encode_project(&old).unwrap());
        assert_recovery(&error, &encode_project(&new).unwrap());
        assert_eq!(fs::read(&path).unwrap(), encode_project(&old).unwrap());
        drop(lock);
    }
}
