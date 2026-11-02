//! Game files (a small part of `love.filesystem`).
//!
//! Relative paths work the same everywhere, so `"assets/player.png"` loads on
//! every platform:
//!
//! - On desktop they are relative to the current directory, which is the
//!   package root under `cargo run`.
//! - On Android and iOS the working directory is [`save_dir`], and files that
//!   aren't there are looked up in the app package. List the directories to
//!   package under `assets` in `[package.metadata.love2drs]`.
//!
//! Every loader in love2drs (`new_image`, `Font::load`, `new_source`, ...) goes
//! through [`read`]. `std::fs` can't see packaged files on mobile, so use
//! [`read`] for your own data files too. Writing with `std::fs` is fine:
//! relative paths end up in [`save_dir`] on mobile.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Reads a whole file: from disk if it exists, otherwise from the app package
/// on Android and iOS. A saved file therefore overrides a packaged one with
/// the same path, as in LÖVE.
///
/// ```no_run
/// let level = love2drs::filesystem::read("assets/level1.json")?;
/// # Ok::<(), love2drs::Error>(())
/// ```
pub fn read(path: impl AsRef<Path>) -> Result<Vec<u8>> {
    let path = path.as_ref();
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(e) if path.is_absolute() || e.kind() != std::io::ErrorKind::NotFound => {
            Err(Error::io(path, e))
        }
        Err(e) => read_packaged(path).ok_or_else(|| Error::io(path, e)),
    }
}

/// The directory for save files and other data the game writes.
///
/// - Android: the app's internal storage, removed with the app.
/// - iOS: the app's `Documents` directory, which the OS backs up.
/// - Desktop: the current directory.
pub fn save_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    if let Some(dir) = crate::platform::android_app().internal_data_path() {
        return dir;
    }
    #[cfg(target_os = "ios")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Documents");
    }
    PathBuf::from(".")
}

#[cfg(target_os = "android")]
fn read_packaged(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;

    let name = packaged_name(path)?;
    let mut asset = crate::platform::android_app()
        .asset_manager()
        .open(&std::ffi::CString::new(name).ok()?)?;
    let mut bytes = Vec::new();
    asset.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The executable sits at the root of an iOS app bundle, next to the assets.
#[cfg(target_os = "ios")]
fn read_packaged(path: &Path) -> Option<Vec<u8>> {
    let exe = std::env::current_exe().ok()?;
    std::fs::read(exe.parent()?.join(packaged_name(path)?)).ok()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn read_packaged(_path: &Path) -> Option<Vec<u8>> {
    None
}

/// `./assets\a.png` becomes `assets/a.png`. Paths that climb out of the package
/// with `..` have no packaged equivalent, so they give `None`.
#[cfg(any(target_os = "android", target_os = "ios", test))]
fn packaged_name(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for part in path.to_str()?.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_names_are_normalized() {
        let name = |p: &str| packaged_name(Path::new(p));
        assert_eq!(name("assets/a.png").as_deref(), Some("assets/a.png"));
        assert_eq!(
            name("./assets\\b//a.png").as_deref(),
            Some("assets/b/a.png")
        );
        assert_eq!(name("../secret"), None);
        assert_eq!(name("."), None);
    }

    #[test]
    fn missing_files_report_the_path() {
        let err = read("does/not/exist.png").unwrap_err();
        assert!(err.to_string().contains("exist.png"), "{err}");
    }
}
