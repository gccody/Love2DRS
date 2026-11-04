//! Gets Android libraries (`android_dependencies`) into a game.
//!
//! A game's own code needs no Java, but some Android SDKs only come as Java and
//! resources in Maven packages. Resolving those, merging their manifests and
//! resources and making the dex files is what the Android Gradle Plugin is for,
//! so a game that lists any gets its base APK from a small Gradle project written
//! here. The native libraries and assets then go in as they do for every other
//! game, and the result is kept until something it was made from changes, so
//! Gradle only runs when it has to. Games without dependencies never touch it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::project::{Project, run, xml_escape};

/// The Android Gradle Plugin that builds the project. Gradle has to be recent
/// enough for it: <https://developer.android.com/build/releases/gradle-plugin>.
const PLUGIN_VERSION: &str = "9.1.0";
const GRADLE_VAR: &str = "GRADLE";

/// What goes into the Gradle project.
pub struct Input<'a> {
    pub project: &'a Project,
    pub sdk_root: &'a Path,
    /// Where the project goes.
    pub dir: PathBuf,
    /// The manifest, as written by `android_manifest` with Gradle in mind.
    pub manifest: String,
    pub debuggable: bool,
    /// A folder of resources to include: the launcher icon.
    pub icon_res: Option<PathBuf>,
}

/// The name of the string resource holding the `i`th `android_meta_data` value.
/// Manifests read a bare number as a number, and a Play Games project id has to
/// arrive as a string, so every value is a resource.
pub fn meta_data_resource(i: usize) -> String {
    format!("love2drs_meta_{i}")
}

/// A base APK with the dex files, resources and merged manifest, unsigned and
/// without native libraries or assets.
pub fn base_apk(input: &Input) -> Result<PathBuf, String> {
    let files = project_files(input);
    let stamp = stamp(input, &files);
    let stamp_file = input.dir.join("love2drs-stamp.txt");
    let built = find_apk(&input.dir);
    if let Some(apk) = &built
        && std::fs::read_to_string(&stamp_file).is_ok_and(|old| old == stamp)
    {
        return Ok(apk.clone());
    }
    for (name, text) in &files {
        let path = input.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let gradle = find_gradle()?;
    println!("Building the Android libraries with Gradle");
    run(
        Command::new(gradle)
            .arg("--console=plain")
            .arg("-p")
            .arg(&input.dir)
            .arg("assembleRelease"),
        "",
    )?;
    let apk = find_apk(&input.dir).ok_or("Gradle didn't make an APK".to_string())?;
    std::fs::write(&stamp_file, stamp).map_err(|e| format!("{}: {e}", stamp_file.display()))?;
    Ok(apk)
}

fn find_apk(dir: &Path) -> Option<PathBuf> {
    let outputs = dir.join("build/outputs/apk/release");
    std::fs::read_dir(outputs)
        .ok()?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .find(|path| path.extension().is_some_and(|e| e == "apk"))
}

/// Everything the APK is made from, in one comparable piece of text.
fn stamp(input: &Input, files: &[(&'static str, String)]) -> String {
    let mut stamp = format!("{PLUGIN_VERSION}\n");
    for (name, text) in files {
        stamp += &format!("{name}\n{text}\n");
    }
    if let Some(icon) = &input.project.icon
        && let Ok(meta) = std::fs::metadata(input.project.root.join(icon))
    {
        stamp += &format!("icon {} {:?}\n", meta.len(), meta.modified().ok());
    }
    stamp
}

/// Gradle reads paths in these files with forward slashes on every system.
fn forward_slashes(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn project_files(input: &Input) -> Vec<(&'static str, String)> {
    let project = input.project;
    let id = project.android_id();
    let mut res = vec!["\"res\"".to_string()];
    if let Some(icon) = &input.icon_res {
        res.push(format!("\"{}\"", forward_slashes(icon)));
    }
    let dependencies: String = project
        .android_dependencies
        .iter()
        .map(|d| format!("    implementation(\"{d}\")\n"))
        .collect();
    let strings: String = project
        .android_meta_data
        .iter()
        .enumerate()
        .map(|(i, (_, value))| {
            format!(
                "    <string name=\"{}\" translatable=\"false\">{}</string>\n",
                meta_data_resource(i),
                xml_escape(value)
            )
        })
        .collect();
    let build = format!(
        r#"plugins {{
    id("com.android.application") version "{PLUGIN_VERSION}"
}}

android {{
    namespace = "{id}"
    compileSdk = {target_sdk}
    defaultConfig {{
        applicationId = "{id}"
        minSdk = {min_sdk}
        targetSdk = {target_sdk}
        versionCode = {version_code}
        versionName = "{version}"
    }}
    buildTypes {{
        release {{
            isMinifyEnabled = false
            isDebuggable = {debuggable}
            signingConfig = null
        }}
    }}
    sourceSets {{
        getByName("main") {{
            manifest.srcFile("AndroidManifest.xml")
            res.srcDirs({res})
        }}
    }}
}}

dependencies {{
{dependencies}}}
"#,
        target_sdk = project.target_sdk,
        min_sdk = project.min_sdk,
        version_code = project.version_code,
        version = project.version,
        debuggable = input.debuggable,
        res = res.join(", "),
    );
    vec![
        (
            "settings.gradle.kts",
            r#"pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}
rootProject.name = "base"
"#
            .to_string(),
        ),
        ("build.gradle.kts", build),
        (
            "gradle.properties",
            "android.useAndroidX=true\norg.gradle.jvmargs=-Xmx2g\n".to_string(),
        ),
        (
            "local.properties",
            format!("sdk.dir={}\n", forward_slashes(input.sdk_root)),
        ),
        ("AndroidManifest.xml", input.manifest.clone()),
        (
            "res/values/love2drs.xml",
            format!("<resources>\n{strings}</resources>\n"),
        ),
    ]
}

fn find_gradle() -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "gradle.bat"
    } else {
        "gradle"
    };
    if let Some(path) = std::env::var_os(GRADLE_VAR) {
        return Ok(PathBuf::from(path));
    }
    let on_path = std::env::var_os("PATH").into_iter().find_map(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    });
    on_path.or_else(|| newest_wrapper(name)).ok_or(format!(
        "android_dependencies need Gradle: install it from https://gradle.org/install, or set \
         {GRADLE_VAR} to its launcher"
    ))
}

/// A copy of Gradle that some other project's wrapper already downloaded.
fn newest_wrapper(launcher: &str) -> Option<PathBuf> {
    let home = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            Some(Path::new(&home).join(".gradle"))
        })?;
    let mut found: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for dist in std::fs::read_dir(home.join("wrapper/dists")).ok()?.flatten() {
        for hash in std::fs::read_dir(dist.path()).into_iter().flatten().flatten() {
            for install in std::fs::read_dir(hash.path()).into_iter().flatten().flatten() {
                let name = install.file_name().to_string_lossy().into_owned();
                let launcher = install.path().join("bin").join(launcher);
                if let Some(version) = name.strip_prefix("gradle-")
                    && launcher.is_file()
                {
                    let version = version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
                    found.push((version, launcher));
                }
            }
        }
    }
    found.into_iter().max_by(|a, b| a.0.cmp(&b.0)).map(|(_, path)| path)
}
