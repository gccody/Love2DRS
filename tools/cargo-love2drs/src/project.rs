//! What's being packaged: the Cargo package, its target and its app settings.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::Options;

/// Which Cargo target holds the game's `fn main`.
#[derive(Clone, Default, PartialEq, Debug)]
pub enum Target {
    /// The package's only binary.
    #[default]
    Default,
    Bin(String),
    Example(String),
}

impl Target {
    pub fn cargo_args(&self) -> Vec<String> {
        match self {
            Target::Default => vec![],
            Target::Bin(name) => vec!["--bin".into(), name.clone()],
            Target::Example(name) => vec!["--example".into(), name.clone()],
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Target::Default => "",
            Target::Bin(name) | Target::Example(name) => name,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Orientation {
    Any,
    Landscape,
    Portrait,
}

pub struct Project {
    pub package: String,
    pub root: PathBuf,
    pub target_dir: PathBuf,
    pub target: Target,
    /// Name of the compiled target.
    pub name: String,
    pub display_name: String,
    /// Reverse-DNS app id. Android and iOS allow different characters, so see
    /// [`Project::android_id`] and [`Project::ios_id`].
    pub app_id: String,
    pub version: String,
    pub version_code: u64,
    pub orientation: Orientation,
    pub assets: Vec<PathBuf>,
    /// A square PNG, relative to the package root.
    pub icon: Option<PathBuf>,
    pub min_sdk: u64,
    pub target_sdk: u64,
    pub permissions: Vec<String>,
    /// What Android files the app under, or `None` to leave it out of the manifest.
    pub app_category: Option<String>,
    pub ios_deployment_target: String,
    /// The game's own `PrivacyInfo.xcprivacy`, relative to the package root.
    pub privacy_manifest: Option<PathBuf>,
}

impl Project {
    pub fn load(options: &Options) -> Result<Project, String> {
        let output = Command::new(cargo())
            .args(["metadata", "--format-version=1", "--no-deps"])
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| format!("can't run cargo: {e}"))?;
        if !output.status.success() {
            return Err("cargo metadata failed; run this inside a Cargo project".into());
        }
        let metadata: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("can't read cargo metadata: {e}"))?;
        let package = find_package(&metadata, options.package.as_deref())?;
        Self::from_metadata(&metadata, package, options)
    }

    fn from_metadata(
        metadata: &Value,
        package: &Value,
        options: &Options,
    ) -> Result<Project, String> {
        let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let name = text(&package["name"]);
        let target = resolve_target(package, &options.target)?;
        let settings = &package["metadata"]["love2drs"];
        let target_name = target.name().to_string();

        let root = PathBuf::from(text(&package["manifest_path"]))
            .parent()
            .ok_or("the package has no directory")?
            .to_path_buf();
        let assets: Vec<PathBuf> = settings["assets"]
            .as_array()
            .map(|list| list.iter().map(|a| PathBuf::from(text(a))).collect())
            .unwrap_or_default();
        check_assets(&root, &assets)?;

        let icon = settings["icon"].as_str().map(PathBuf::from);
        if let Some(icon) = &icon {
            check_assets(&root, std::slice::from_ref(icon))?;
        }

        let privacy_manifest = settings["privacy_manifest"].as_str().map(PathBuf::from);
        if let Some(manifest) = &privacy_manifest {
            check_assets(&root, std::slice::from_ref(manifest))?;
        }

        let version = text(&package["version"]);
        let app_id = match settings["app_id"].as_str() {
            // Examples share the package's settings but are separate apps.
            Some(id) if matches!(target, Target::Example(_)) => format!("{id}.{target_name}"),
            Some(id) => id.to_string(),
            None => format!("com.love2drs.{target_name}"),
        };
        Ok(Project {
            package: name,
            root,
            target_dir: PathBuf::from(text(&metadata["target_directory"])),
            display_name: match settings["name"].as_str() {
                Some(name) if !matches!(target, Target::Example(_)) => name.to_string(),
                _ => target_name.clone(),
            },
            target,
            name: target_name,
            app_id,
            version_code: settings["version_code"]
                .as_u64()
                .unwrap_or_else(|| version_code(&version)),
            version,
            orientation: parse_orientation(settings["orientation"].as_str())?,
            assets,
            icon,
            min_sdk: settings["min_sdk"].as_u64().unwrap_or(26),
            target_sdk: settings["target_sdk"].as_u64().unwrap_or(36),
            permissions: settings["permissions"]
                .as_array()
                .map(|list| list.iter().map(text).collect())
                .unwrap_or_default(),
            app_category: parse_app_category(settings["app_category"].as_str())?,
            ios_deployment_target: settings["ios_deployment_target"]
                .as_str()
                .unwrap_or("15.0")
                .to_string(),
            privacy_manifest,
        })
    }

    /// Android ids allow `_` but not `-`.
    pub fn android_id(&self) -> String {
        self.app_id.replace('-', "_")
    }

    /// iOS bundle ids allow `-` but not `_`.
    pub fn ios_id(&self) -> String {
        self.app_id.replace('_', "-")
    }

    /// Where packaged apps go.
    pub fn out_dir(&self, platform: &str) -> Result<PathBuf, String> {
        let dir = self.target_dir.join("love2drs").join(platform);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(dir)
    }

    /// Every packaged file as `(path on disk, path inside the app)`, with `/`
    /// separators inside the app.
    pub fn asset_files(&self) -> Result<Vec<(PathBuf, String)>, String> {
        let mut files = Vec::new();
        for asset in &self.assets {
            collect_files(&self.root.join(asset), &to_app_path(asset), &mut files)?;
        }
        Ok(files)
    }
}

/// The cargo that started this tool, if any.
pub fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Runs a command, failing with `hint` when it can't be started.
pub fn run(cmd: &mut Command, hint: &str) -> Result<(), String> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd.status().map_err(|e| {
        let hint = if hint.is_empty() {
            String::new()
        } else {
            format!(" {hint}")
        };
        format!("can't run {name}: {e}.{hint}")
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed ({status})"))
    }
}

/// Runs a command and returns what it printed, or `None` if it failed.
pub fn capture(cmd: &mut Command) -> Option<String> {
    let output = cmd.stderr(Stdio::null()).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Checks that a Rust target's standard library is installed, when rustup can tell.
pub fn require_rust_target(triple: &str) -> Result<(), String> {
    let installed = capture(Command::new("rustup").args(["target", "list", "--installed"]));
    match installed {
        Some(list) if !list.lines().any(|t| t == triple) => {
            Err(format!("run `rustup target add {triple}` first"))
        }
        _ => Ok(()),
    }
}

/// Finds the executable of target `name` in Cargo's `--message-format=json` output.
pub fn built_executable(cargo_output: &[u8], name: &str) -> Result<PathBuf, String> {
    String::from_utf8_lossy(cargo_output)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == name)
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .ok_or(format!("the build didn't produce `{name}`"))
}

/// Escapes text for XML files (manifests and plists).
pub fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn find_package<'a>(metadata: &'a Value, wanted: Option<&str>) -> Result<&'a Value, String> {
    let packages = metadata["packages"].as_array().ok_or("no packages found")?;
    if let Some(wanted) = wanted {
        return packages
            .iter()
            .find(|p| p["name"] == wanted)
            .ok_or(format!("no package named `{wanted}` in this workspace"));
    }
    let defaults = metadata["workspace_default_members"].as_array();
    let is_default = |p: &&Value| defaults.is_some_and(|ids| ids.contains(&p["id"]));
    match packages.iter().filter(is_default).collect::<Vec<_>>()[..] {
        [package] => Ok(package),
        _ => Err("this workspace has several packages; choose one with -p".into()),
    }
}

/// Finds the target to build and checks that it exists.
fn resolve_target(package: &Value, wanted: &Target) -> Result<Target, String> {
    let targets_of_kind = |kind: &str| -> Vec<String> {
        package["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| {
                t["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|k| k == kind))
            })
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect()
    };
    match wanted {
        Target::Default => match targets_of_kind("bin")[..] {
            [] => Err("this package has no binary; choose one with --example".into()),
            [ref only] => Ok(Target::Bin(only.clone())),
            ref many => Err(format!(
                "this package has several binaries; choose one with --bin: {}",
                many.join(", ")
            )),
        },
        Target::Bin(name) if !targets_of_kind("bin").contains(name) => {
            Err(format!("no binary named `{name}` in this package"))
        }
        Target::Example(name) if !targets_of_kind("example").contains(name) => {
            Err(format!("no example named `{name}` in this package"))
        }
        known => Ok(known.clone()),
    }
}

fn parse_orientation(value: Option<&str>) -> Result<Orientation, String> {
    match value {
        None | Some("any") => Ok(Orientation::Any),
        Some("landscape") => Ok(Orientation::Landscape),
        Some("portrait") => Ok(Orientation::Portrait),
        Some(other) => Err(format!(
            "orientation `{other}` should be landscape, portrait or any"
        )),
    }
}

/// `android:appCategory`. Games are the default here because Android 16 ignores
/// a locked orientation on tablets unless the app says it's a game.
fn parse_app_category(value: Option<&str>) -> Result<Option<String>, String> {
    const CATEGORIES: [&str; 9] = [
        "accessibility",
        "audio",
        "game",
        "image",
        "maps",
        "news",
        "productivity",
        "social",
        "video",
    ];
    match value {
        None => Ok(Some("game".to_string())),
        Some("none") => Ok(None),
        Some(category) if CATEGORIES.contains(&category) => Ok(Some(category.to_string())),
        Some(other) => Err(format!(
            "app_category `{other}` should be none or one of {}",
            CATEGORIES.join(", ")
        )),
    }
}

/// `1.2.3` becomes `10203`. Stores require a number that grows with each release.
fn version_code(version: &str) -> u64 {
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u64>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0);
    (next() * 10_000 + next() * 100 + next()).max(1)
}

/// Assets keep their path in the app, so it has to stay inside the package.
fn check_assets(root: &Path, assets: &[PathBuf]) -> Result<(), String> {
    for asset in assets {
        let inside = asset
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !inside {
            return Err(format!(
                "asset `{}` must be a relative path inside the package",
                asset.display()
            ));
        }
        if !root.join(asset).exists() {
            return Err(format!(
                "asset `{}` doesn't exist in {}",
                asset.display(),
                root.display()
            ));
        }
    }
    Ok(())
}

fn to_app_path(path: &Path) -> String {
    let parts: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect();
    parts.join("/")
}

fn collect_files(
    path: &Path,
    app_path: &str,
    files: &mut Vec<(PathBuf, String)>,
) -> Result<(), String> {
    if !path.is_dir() {
        files.push((path.to_path_buf(), app_path.to_string()));
        return Ok(());
    }
    let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut entries: Vec<_> = entries
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with('.') {
            collect_files(&entry.path(), &format!("{app_path}/{name}"), files)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_codes_grow_with_the_version() {
        assert_eq!(version_code("0.1.0"), 100);
        assert_eq!(version_code("1.2.3"), 10203);
        assert_eq!(version_code("1.2.3-beta.1"), 10203);
        assert_eq!(version_code("0.0.0"), 1);
        assert!(version_code("1.10.0") > version_code("1.9.9"));
    }

    #[test]
    fn assets_must_stay_in_the_package() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(check_assets(root, &[PathBuf::from("src")]).is_ok());
        assert!(check_assets(root, &[PathBuf::from("../secret")]).is_err());
        assert!(check_assets(root, &[PathBuf::from("no-such-dir")]).is_err());
    }

    #[test]
    fn asset_paths_use_forward_slashes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        collect_files(&root.join("src"), "game/src", &mut files).unwrap();
        assert!(files.iter().any(|(_, p)| p == "game/src/main.rs"));
        assert_eq!(to_app_path(Path::new("./a/b.png")), "a/b.png");
    }

    #[test]
    fn the_only_binary_is_the_default_target() {
        let package = serde_json::json!({ "targets": [
            { "name": "game", "kind": ["bin"] },
            { "name": "demo", "kind": ["example"] },
        ]});
        assert_eq!(
            resolve_target(&package, &Target::Default),
            Ok(Target::Bin("game".into()))
        );
        let demo = Target::Example("demo".into());
        assert_eq!(resolve_target(&package, &demo), Ok(demo));
        assert!(resolve_target(&package, &Target::Example("x".into())).is_err());
    }

    #[test]
    fn app_category_defaults_to_game() {
        assert_eq!(parse_app_category(None), Ok(Some("game".to_string())));
        assert_eq!(parse_app_category(Some("none")), Ok(None));
        assert_eq!(
            parse_app_category(Some("social")),
            Ok(Some("social".to_string()))
        );
        assert!(parse_app_category(Some("games")).is_err());
    }

    #[test]
    fn xml_is_escaped() {
        assert_eq!(xml_escape("a<b> & \"c\""), "a&lt;b&gt; &amp; &quot;c&quot;");
    }
}
