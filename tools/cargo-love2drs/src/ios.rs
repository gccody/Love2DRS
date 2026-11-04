//! Packages a game as an iOS `.app` bundle with Xcode's command line tools, and
//! for the App Store as an `.ipa` with its debug symbols in a `.dSYM`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::project::{self, Orientation, Project, capture, run, xml_escape};
use crate::{Action, Options};

pub fn main(project: &Project, options: &Options, action: Action) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("iOS apps can only be built on macOS, because linking needs Xcode".into());
    }
    let installing = matches!(action, Action::Install | Action::Run);
    if installing && options.physical && options.identity.is_none() {
        return Err("installing on a real device needs --identity and --profile".into());
    }
    if options.identity.is_some() != options.profile.is_some() {
        return Err("--identity and --profile only work together".into());
    }
    let store = action == Action::Package;
    let app = build_app(project, options, store)?;
    println!("Built {}", app.display());
    if store {
        let ipa = write_ipa(project, &app)?;
        println!("Built {}", ipa.display());
        println!("Built {}", dsym_path(&app).display());
        return Ok(());
    }
    if action == Action::Build {
        return Ok(());
    }
    let id = project.ios_id();
    if options.physical {
        let device = options
            .device
            .as_deref()
            .ok_or("choose a device with --device; `xcrun devicectl list devices` lists them")?;
        install_physical(device, &app)?;
        println!("Installed {id}");
        if action == Action::Run {
            launch_physical(device, &id)?;
        }
    } else {
        let device = simulator(options)?;
        run(
            Command::new("xcrun")
                .args(["simctl", "install", &device])
                .arg(&app),
            "",
        )?;
        println!("Installed {id}");
        if action == Action::Run {
            run(
                Command::new("xcrun").args([
                    "simctl",
                    "launch",
                    "--console-pty",
                    "--terminate-running-process",
                    &device,
                    &id,
                ]),
                "",
            )?;
        }
    }
    Ok(())
}

fn rust_target(options: &Options) -> &'static str {
    if options.physical {
        "aarch64-apple-ios"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-apple-ios-sim"
    } else {
        "x86_64-apple-ios"
    }
}

/// `store` also keeps debug symbols aside and strips them from the app.
fn build_app(project: &Project, options: &Options, store: bool) -> Result<PathBuf, String> {
    let triple = rust_target(options);
    project::require_rust_target(triple)?;
    let exe = compile(project, options, triple, store)?;

    let platform = if options.physical {
        "iphoneos"
    } else {
        "iphonesimulator"
    };
    let app = project.out_dir(&format!("ios/{platform}/{}", options.profile_dir()))?;
    let app = app.join(format!("{}.app", project.name));
    if app.exists() {
        std::fs::remove_dir_all(&app).map_err(|e| format!("{}: {e}", app.display()))?;
    }
    std::fs::create_dir_all(&app).map_err(|e| format!("{}: {e}", app.display()))?;

    let app_exe = app.join(&project.name);
    copy(&exe, &app_exe)?;
    let build_info = options.physical.then(BuildInfo::find).transpose()?;
    if store {
        keep_symbols(&exe, &app)?;
        if let Some(info) = &build_info {
            warn_about_old_sdk(info);
        }
    }
    write_privacy_manifest(project, &app_exe, &app)?;
    if store {
        run(
            Command::new("xcrun")
                .args(["strip", "-S", "-x"])
                .arg(&app_exe),
            "",
        )?;
    }
    let plist = info_plist(project, options.physical, build_info.as_ref());
    std::fs::write(app.join("Info.plist"), plist).map_err(|e| format!("Info.plist: {e}"))?;
    for (file, app_path) in project.asset_files()? {
        let dest = app.join(app_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        copy(&file, &dest)?;
    }
    if let Some(icon) = &project.icon {
        compile_icon(project, options, &project.root.join(icon), &app)?;
    }
    sign(&app, options)?;
    Ok(app)
}

/// Xcode and SDK facts that the App Store expects in `Info.plist`. Xcode normally
/// writes them, and App Store Connect reads them to tell what built the app.
struct BuildInfo {
    sdk_version: String,
    sdk_build: String,
    xcode_version: String,
    xcode_build: String,
    machine_os_build: String,
}

impl BuildInfo {
    fn find() -> Result<BuildInfo, String> {
        let ask = |program: &str, args: &[&str]| {
            capture(Command::new(program).args(args)).ok_or(format!(
                "can't run `{program} {}`; is Xcode installed?",
                args.join(" ")
            ))
        };
        let sdk = ["--sdk", "iphoneos"];
        let sdk_args = |flag: &'static str| [sdk[0], sdk[1], flag];
        let xcodebuild = ask("xcodebuild", &["-version"])?;
        let (xcode_version, xcode_build) = parse_xcodebuild_version(&xcodebuild)
            .ok_or(format!("can't read Xcode's version from `{xcodebuild}`"))?;
        Ok(BuildInfo {
            sdk_version: ask("xcrun", &sdk_args("--show-sdk-version"))?,
            sdk_build: ask("xcrun", &sdk_args("--show-sdk-build-version"))?,
            xcode_version,
            xcode_build,
            machine_os_build: ask("sw_vers", &["-buildVersion"])?,
        })
    }

    fn plist_entries(&self) -> String {
        let entries = [
            ("BuildMachineOSBuild", self.machine_os_build.clone()),
            (
                "DTCompiler",
                "com.apple.compilers.llvm.clang.1_0".to_string(),
            ),
            ("DTPlatformBuild", self.sdk_build.clone()),
            ("DTPlatformName", "iphoneos".to_string()),
            ("DTPlatformVersion", self.sdk_version.clone()),
            ("DTSDKBuild", self.sdk_build.clone()),
            ("DTSDKName", format!("iphoneos{}", self.sdk_version)),
            ("DTXcode", dt_xcode(&self.xcode_version)),
            ("DTXcodeBuild", self.xcode_build.clone()),
        ];
        entries
            .iter()
            .map(|(key, value)| {
                format!(
                    "    <key>{key}</key>\n    <string>{}</string>\n",
                    xml_escape(value)
                )
            })
            .collect()
    }
}

/// Reads `Xcode 27.2\nBuild version 27B52`.
fn parse_xcodebuild_version(text: &str) -> Option<(String, String)> {
    let mut lines = text.lines();
    let version = lines.next()?.strip_prefix("Xcode ")?.trim();
    let build = lines.next()?.strip_prefix("Build version ")?.trim();
    Some((version.to_string(), build.to_string()))
}

/// `DTXcode` is the version without dots, padded: `26.0.1` is `2601`, `16.4` is `1640`.
fn dt_xcode(version: &str) -> String {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    format!("{major:02}{minor}{patch}")
}

/// What App Store Connect has required since April 28, 2026.
const APP_STORE_MIN_SDK: u32 = 26;

fn warn_about_old_sdk(info: &BuildInfo) {
    let major = info.sdk_version.split('.').next().unwrap_or("");
    if major.parse::<u32>().is_ok_and(|v| v < APP_STORE_MIN_SDK) {
        println!(
            "warning: the App Store requires the iOS {APP_STORE_MIN_SDK} SDK or newer, and this \
             Xcode has {}",
            info.sdk_version
        );
    }
}

fn dsym_path(app: &Path) -> PathBuf {
    let name = app.file_name().unwrap_or_default().to_string_lossy();
    app.with_file_name(format!("{name}.dSYM"))
}

/// Puts the debug symbols rustc wrote where they're easy to find. Upload them to
/// make sense of crash reports.
fn keep_symbols(exe: &Path, app: &Path) -> Result<(), String> {
    let mut symbols = exe.as_os_str().to_os_string();
    symbols.push(".dSYM");
    let symbols = PathBuf::from(symbols);
    if !symbols.exists() {
        return Err(format!(
            "the build left no symbols at {}",
            symbols.display()
        ));
    }
    let dsym = dsym_path(app);
    if dsym.exists() {
        std::fs::remove_dir_all(&dsym).map_err(|e| format!("{}: {e}", dsym.display()))?;
    }
    run(Command::new("ditto").arg(&symbols).arg(&dsym), "")
}

/// Zips the app the way the App Store takes it.
fn write_ipa(project: &Project, app: &Path) -> Result<PathBuf, String> {
    let dir = app.parent().ok_or("the app has no directory")?;
    let ipa = dir.join(format!("{}.ipa", project.name));
    if ipa.exists() {
        std::fs::remove_file(&ipa).map_err(|e| format!("{}: {e}", ipa.display()))?;
    }
    let work = dir.join("ipa");
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    let name = app.file_name().ok_or("the app has no name")?;
    let payload = work.join("Payload");
    std::fs::create_dir_all(&payload).map_err(|e| format!("{}: {e}", payload.display()))?;
    // `ditto` keeps what the code signature covers exactly as it is.
    run(Command::new("ditto").arg(app).arg(payload.join(name)), "")?;
    run(
        Command::new("zip")
            .current_dir(&work)
            .args(["-qry"])
            .arg(&ipa)
            .arg("Payload"),
        "",
    )?;
    std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    Ok(ipa)
}

/// A system API that Apple requires a reason for, how to spot it in an executable,
/// and the reason that fits a game using it the ordinary way.
struct RequiredReasonApi {
    category: &'static str,
    reason: &'static str,
    symbols: &'static [&'static str],
}

/// Rust's standard library and common crates use some of these without a game
/// knowing. The reasons are for use within the app itself.
const REQUIRED_REASON_APIS: [RequiredReasonApi; 4] = [
    RequiredReasonApi {
        category: "NSPrivacyAccessedAPICategoryFileTimestamp",
        // Files inside the app's own container.
        reason: "C617.1",
        symbols: &[
            "_stat",
            "_fstat",
            "_lstat",
            "_fstatat",
            "_getattrlist",
            "_fgetattrlist",
            "_getattrlistat",
            "_getattrlistbulk",
        ],
    },
    RequiredReasonApi {
        category: "NSPrivacyAccessedAPICategorySystemBootTime",
        // Measuring time between events within the app.
        reason: "35F9.1",
        symbols: &["_mach_absolute_time"],
    },
    RequiredReasonApi {
        category: "NSPrivacyAccessedAPICategoryDiskSpace",
        // Checking there's room to write files.
        reason: "E174.1",
        symbols: &["_statfs", "_statvfs", "_fstatfs", "_fstatvfs"],
    },
    RequiredReasonApi {
        category: "NSPrivacyAccessedAPICategoryUserDefaults",
        // Data only the app itself reads and writes.
        reason: "CA92.1",
        symbols: &["_OBJC_CLASS_$_NSUserDefaults"],
    },
];

/// Which required-reason APIs appear in the output of `nm -u`.
fn apis_in_use(undefined_symbols: &str) -> Vec<&'static RequiredReasonApi> {
    let symbols: Vec<&str> = undefined_symbols
        .lines()
        // Versions of a function have a suffix, like `_stat$INODE64`.
        .map(|line| line.trim().split('$').next().unwrap_or(""))
        .collect();
    let objc: Vec<&str> = undefined_symbols.lines().map(str::trim).collect();
    REQUIRED_REASON_APIS
        .iter()
        .filter(|api| {
            api.symbols
                .iter()
                .any(|symbol| symbols.contains(symbol) || objc.contains(symbol))
        })
        .collect()
}

/// Writes the privacy manifest the App Store asks for.
fn privacy_manifest_xml(apis: &[&RequiredReasonApi]) -> String {
    let accessed: String = apis
        .iter()
        .map(|api| {
            format!(
                r#"        <dict>
            <key>NSPrivacyAccessedAPIType</key>
            <string>{}</string>
            <key>NSPrivacyAccessedAPITypeReasons</key>
            <array>
                <string>{}</string>
            </array>
        </dict>
"#,
                api.category, api.reason
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSPrivacyTracking</key>
    <false/>
    <key>NSPrivacyTrackingDomains</key>
    <array/>
    <key>NSPrivacyCollectedDataTypes</key>
    <array/>
    <key>NSPrivacyAccessedAPITypes</key>
    <array>
{accessed}    </array>
</dict>
</plist>
"#
    )
}

/// Copies the game's own privacy manifest, or makes one from what the executable
/// uses. A made one can't know what the game does with the player's data, so it
/// says so.
fn write_privacy_manifest(project: &Project, exe: &Path, app: &Path) -> Result<(), String> {
    let dest = app.join("PrivacyInfo.xcprivacy");
    if let Some(manifest) = &project.privacy_manifest {
        return copy(&project.root.join(manifest), &dest);
    }
    let symbols = capture(Command::new("xcrun").args(["nm", "-u"]).arg(exe))
        .ok_or("can't list the system APIs the game uses")?;
    let apis = apis_in_use(&symbols);
    let categories: Vec<&str> = apis.iter().map(|api| api.category).collect();
    println!(
        "note: made a privacy manifest declaring {} and no tracking or data collection. If the \
         game collects data, set `privacy_manifest` in Cargo.toml.",
        if categories.is_empty() {
            "no required-reason APIs".to_string()
        } else {
            categories.join(", ")
        }
    );
    std::fs::write(&dest, privacy_manifest_xml(&apis))
        .map_err(|e| format!("{}: {e}", dest.display()))
}

/// Compiles the icon with Xcode's asset catalog compiler, which writes it into
/// the bundle, and adds the entries it asks for to `Info.plist`.
fn compile_icon(
    project: &Project,
    options: &Options,
    icon: &Path,
    app: &Path,
) -> Result<(), String> {
    let work = app.with_extension("icon");
    let set = work.join("Assets.xcassets/AppIcon.appiconset");
    if work.exists() {
        std::fs::remove_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    }
    std::fs::create_dir_all(&set).map_err(|e| format!("{}: {e}", set.display()))?;
    let write_err = |e: &dyn std::fmt::Display| format!("can't write the icon: {e}");
    crate::icon::app_store_icon(&crate::icon::load(icon)?)
        .save(set.join("icon.png"))
        .map_err(|e| write_err(&e))?;
    std::fs::write(
        set.join("Contents.json"),
        r#"{"images":[{"filename":"icon.png","idiom":"universal","platform":"ios","size":"1024x1024"}],"info":{"author":"xcode","version":1}}"#,
    )
    .map_err(|e| write_err(&e))?;
    let catalog = work.join("Assets.xcassets");
    std::fs::write(
        catalog.join("Contents.json"),
        r#"{"info":{"author":"xcode","version":1}}"#,
    )
    .map_err(|e| write_err(&e))?;

    let partial = work.join("partial.plist");
    let platform = if options.physical {
        "iphoneos"
    } else {
        "iphonesimulator"
    };
    run(
        Command::new("xcrun")
            .arg("actool")
            .arg("--compile")
            .arg(app)
            .args(["--platform", platform])
            .args([
                "--minimum-deployment-target",
                &project.ios_deployment_target,
            ])
            .args(["--app-icon", "AppIcon"])
            .arg("--output-partial-info-plist")
            .arg(&partial)
            .args(["--target-device", "iphone", "--target-device", "ipad"])
            .args(["--output-format", "human-readable-text"])
            .arg(&catalog)
            .stdout(Stdio::null()),
        "",
    )?;
    run(
        Command::new("/usr/libexec/PlistBuddy")
            .arg("-c")
            .arg(format!("Merge {}", partial.display()))
            .arg(app.join("Info.plist")),
        "",
    )
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", from.display()))
}

/// Compiles the game and returns the executable Cargo reports. `store` also makes
/// rustc leave a `.dSYM` next to it, however the game's release profile is set up.
fn compile(
    project: &Project,
    options: &Options,
    triple: &str,
    store: bool,
) -> Result<PathBuf, String> {
    let mut cargo = Command::new(project::cargo());
    cargo
        .current_dir(&project.root)
        .arg("build")
        .args(options.cargo_args(project))
        .args(["--target", triple])
        .arg("--message-format=json-render-diagnostics")
        .env("IPHONEOS_DEPLOYMENT_TARGET", &project.ios_deployment_target);
    if store {
        cargo
            .env("CARGO_PROFILE_RELEASE_DEBUG", "line-tables-only")
            .env("CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO", "packed")
            .env("CARGO_PROFILE_RELEASE_STRIP", "none");
    }
    let output = cargo
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("can't run cargo: {e}"))?;
    if !output.status.success() {
        return Err("the build failed".into());
    }
    project::built_executable(&output.stdout, &project.name)
}

fn info_plist(project: &Project, physical: bool, build_info: Option<&BuildInfo>) -> String {
    let landscape = [
        "UIInterfaceOrientationLandscapeLeft",
        "UIInterfaceOrientationLandscapeRight",
    ];
    let portrait = ["UIInterfaceOrientationPortrait"];
    let orientations: Vec<&str> = match project.orientation {
        Orientation::Any => portrait.iter().chain(&landscape).copied().collect(),
        Orientation::Landscape => landscape.to_vec(),
        Orientation::Portrait => portrait.to_vec(),
    };
    let orientations: String = orientations
        .iter()
        .map(|o| format!("        <string>{o}</string>\n"))
        .collect();
    let platform = if physical {
        "iPhoneOS"
    } else {
        "iPhoneSimulator"
    };
    let id = xml_escape(&project.ios_id());
    let exe = xml_escape(&project.name);
    let name = xml_escape(&project.display_name);
    let version = xml_escape(&project.version);
    let code = project.version_code;
    let min_os = xml_escape(&project.ios_deployment_target);
    let build_info = build_info.map(BuildInfo::plist_entries).unwrap_or_default();
    // The App Store refuses a 64-bit binary that doesn't list `arm64` here (error 90502).
    // Simulators can run on an Intel Mac, so only a real device's build says so.
    let arm64 = if physical {
        "        <string>arm64</string>\n"
    } else {
        ""
    };
    // Eligibility, not a switch: the OS decides when Game Mode runs. Apple's sessions name
    // both keys and don't say how they differ on iOS, so a game gets both.
    let game_mode = if project.app_category.as_deref() == Some("game") {
        "    <key>GCSupportsGameMode</key>
    <true/>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.games</string>
    <key>LSSupportsGameMode</key>
    <true/>
"
    } else {
        ""
    };
    // An empty `UILaunchScreen` is what asks for the whole screen. Without one,
    // iOS runs the app letterboxed at an old iPhone's resolution.
    //
    // `UIApplicationSceneManifest` adopts the UIScene lifecycle, which iOS 27 demands
    // of apps built with its SDK, stopping them at launch otherwise. The manifest only
    // counts if it lists a configuration: iOS 27 still stops an app whose manifest has
    // nothing but `UIApplicationSupportsMultipleScenes`. The configuration names no
    // delegate because winit attaches its window to the scene itself.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
{build_info}    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>{name}</string>
    <key>CFBundleExecutable</key>
    <string>{exe}</string>
    <key>CFBundleIdentifier</key>
    <string>{id}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>{platform}</string>
    </array>
    <key>CFBundleVersion</key>
    <string>{code}</string>
    <key>GCSupportedGameControllers</key>
    <array>
        <dict>
            <key>ProfileName</key>
            <string>ExtendedGamepad</string>
        </dict>
    </array>
    <key>GCSupportsControllerUserInteraction</key>
    <true/>
{game_mode}    <key>LSRequiresIPhoneOS</key>
    <true/>
    <key>MinimumOSVersion</key>
    <string>{min_os}</string>
    <key>UIApplicationSceneManifest</key>
    <dict>
        <key>UIApplicationSupportsMultipleScenes</key>
        <false/>
        <key>UISceneConfigurations</key>
        <dict>
            <key>UIWindowSceneSessionRoleApplication</key>
            <array>
                <dict>
                    <key>UISceneConfigurationName</key>
                    <string>Default Configuration</string>
                </dict>
            </array>
        </dict>
    </dict>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
{arm64}        <string>metal</string>
    </array>
    <key>UIRequiresFullScreen</key>
    <true/>
    <key>UIStatusBarHidden</key>
    <true/>
    <key>UISupportedInterfaceOrientations</key>
    <array>
{orientations}    </array>
</dict>
</plist>
"#
    )
}

/// Simulators accept an ad-hoc signature. Real devices need a certificate and a
/// provisioning profile from an Apple developer account, so without them the
/// bundle is left for the user to sign.
fn sign(app: &Path, options: &Options) -> Result<(), String> {
    let mut codesign = Command::new("codesign");
    codesign.args(["--force", "--timestamp=none"]);
    match (&options.identity, &options.profile) {
        (Some(identity), Some(profile)) => {
            copy(Path::new(profile), &app.join("embedded.mobileprovision"))?;
            let entitlements = entitlements_from_profile(profile, app)?;
            codesign
                .args(["--sign", identity])
                .arg("--entitlements")
                .arg(entitlements);
        }
        _ if options.physical => {
            println!(
                "warning: the app isn't signed. A real device needs --identity and --profile."
            );
            return Ok(());
        }
        _ => {
            codesign.args(["--sign", "-"]);
        }
    }
    run(codesign.arg(app), "")
}

/// Provisioning profiles list the entitlements an app may be signed with.
fn entitlements_from_profile(profile: &str, app: &Path) -> Result<PathBuf, String> {
    let decoded = capture(Command::new("security").args(["cms", "-D", "-i", profile]))
        .ok_or(format!("can't read the provisioning profile {profile}"))?;
    let dir = app.parent().ok_or("the app has no directory")?;
    let decoded_path = dir.join("profile.plist");
    std::fs::write(&decoded_path, decoded).map_err(|e| format!("profile.plist: {e}"))?;
    let entitlements = dir.join("entitlements.plist");
    run(
        Command::new("plutil")
            .args(["-extract", "Entitlements", "xml1", "-o"])
            .arg(&entitlements)
            .arg(&decoded_path),
        "",
    )?;
    Ok(entitlements)
}

/// The simulator to use: the one asked for, the booted one, or else the first
/// available iPhone, which gets booted.
fn simulator(options: &Options) -> Result<String, String> {
    if let Some(device) = &options.device {
        return Ok(device.clone());
    }
    let list =
        capture(Command::new("xcrun").args(["simctl", "list", "devices", "available", "--json"]))
            .ok_or("can't list simulators: install Xcode and an iOS runtime")?;
    let list: serde_json::Value =
        serde_json::from_str(&list).map_err(|e| format!("can't read the simulator list: {e}"))?;
    let (booted, iphone) = pick_simulator(&list);
    if let Some(booted) = booted {
        return Ok(booted);
    }
    let iphone = iphone.ok_or("no iPhone simulators are available; add one in Xcode")?;
    run(
        Command::new("xcrun").args(["simctl", "bootstatus", &iphone, "-b"]),
        "",
    )?;
    // Shows the simulator's window. The app runs either way.
    let _ = Command::new("open").args(["-a", "Simulator"]).status();
    Ok(iphone)
}

/// Finds `(a booted device, an iPhone)` in the output of `simctl list`.
fn pick_simulator(list: &serde_json::Value) -> (Option<String>, Option<String>) {
    let devices: Vec<&serde_json::Value> = list["devices"]
        .as_object()
        .into_iter()
        .flat_map(|runtimes| runtimes.iter())
        .filter(|(runtime, _)| runtime.contains("iOS"))
        .filter_map(|(_, devices)| devices.as_array())
        .flatten()
        .collect();
    let udid = |d: &&serde_json::Value| d["udid"].as_str().map(str::to_string);
    let booted = devices
        .iter()
        .find(|d| d["state"] == "Booted")
        .and_then(udid);
    let iphone = devices
        .iter()
        .find(|d| d["name"].as_str().is_some_and(|n| n.starts_with("iPhone")))
        .and_then(udid);
    (booted, iphone)
}

fn install_physical(device: &str, app: &Path) -> Result<(), String> {
    run(
        Command::new("xcrun")
            .args(["devicectl", "device", "install", "app", "--device", device])
            .arg(app),
        "",
    )
}

fn launch_physical(device: &str, id: &str) -> Result<(), String> {
    run(
        Command::new("xcrun").args([
            "devicectl",
            "device",
            "process",
            "launch",
            "--console",
            "--device",
            device,
            id,
        ]),
        "",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Target;

    fn project(orientation: Orientation) -> Project {
        Project {
            package: "game".into(),
            root: ".".into(),
            target_dir: "target".into(),
            target: Target::Bin("my_game".into()),
            name: "my_game".into(),
            display_name: "Tom & Jerry".into(),
            app_id: "com.example.my_game".into(),
            version: "1.2.3".into(),
            version_code: 10203,
            orientation,
            assets: vec![],
            min_sdk: 26,
            target_sdk: 36,
            permissions: vec![],
            icon: None,
            app_category: Some("game".into()),
            ios_deployment_target: "15.0".into(),
            privacy_manifest: None,
            android_dependencies: vec![],
            android_meta_data: vec![],
        }
    }

    #[test]
    fn plist_fills_in_the_project() {
        let plist = info_plist(&project(Orientation::Any), false, None);
        assert!(plist.contains("<string>com.example.my-game</string>"));
        assert!(plist.contains("<string>Tom &amp; Jerry</string>"));
        assert!(plist.contains("<string>iPhoneSimulator</string>"));
        assert!(plist.contains("UIInterfaceOrientationPortrait"));
        assert!(plist.contains("UIInterfaceOrientationLandscapeLeft"));
    }

    #[test]
    fn plist_limits_orientation() {
        let plist = info_plist(&project(Orientation::Landscape), true, None);
        assert!(plist.contains("<string>iPhoneOS</string>"));
        assert!(!plist.contains("UIInterfaceOrientationPortrait"));
    }

    #[test]
    fn plist_adopts_the_scene_lifecycle() {
        for physical in [false, true] {
            let plist = info_plist(&project(Orientation::Any), physical, None);
            for expected in [
                "<key>UIApplicationSceneManifest</key>\n    <dict>",
                "<key>UIApplicationSupportsMultipleScenes</key>\n        <false/>",
                // iOS 27 doesn't count a manifest without a configuration as adopting.
                "<key>UIWindowSceneSessionRoleApplication</key>\n            <array>\n                \
                 <dict>\n                    <key>UISceneConfigurationName</key>",
            ] {
                assert!(plist.contains(expected), "missing {expected}");
            }
            // winit attaches its own window, and there's no delegate class to name.
            assert!(!plist.contains("UISceneDelegateClassName"));
        }
    }

    #[test]
    fn plist_records_what_built_the_app() {
        let info = BuildInfo {
            sdk_version: "26.2".into(),
            sdk_build: "23C53".into(),
            xcode_version: "26.2".into(),
            xcode_build: "17C52".into(),
            machine_os_build: "25C56".into(),
        };
        let plist = info_plist(&project(Orientation::Any), true, Some(&info));
        for expected in [
            "<key>DTSDKName</key>\n    <string>iphoneos26.2</string>",
            "<key>DTPlatformName</key>\n    <string>iphoneos</string>",
            "<key>DTPlatformVersion</key>\n    <string>26.2</string>",
            "<key>DTPlatformBuild</key>\n    <string>23C53</string>",
            "<key>DTSDKBuild</key>\n    <string>23C53</string>",
            "<key>DTXcode</key>\n    <string>2620</string>",
            "<key>DTXcodeBuild</key>\n    <string>17C52</string>",
            "<key>BuildMachineOSBuild</key>\n    <string>25C56</string>",
            "<key>DTCompiler</key>",
        ] {
            assert!(plist.contains(expected), "missing {expected}");
        }
        assert!(!info_plist(&project(Orientation::Any), true, None).contains("DTSDKName"));
    }

    #[test]
    fn games_ask_for_game_mode() {
        let plist = info_plist(&project(Orientation::Any), true, None);
        for key in ["GCSupportsGameMode", "LSSupportsGameMode"] {
            assert!(plist.contains(&format!("<key>{key}</key>\n    <true/>")), "{key}");
        }
        assert!(plist.contains("<string>public.app-category.games</string>"));

        let mut other = project(Orientation::Any);
        other.app_category = None;
        let plist = info_plist(&other, true, None);
        assert!(!plist.contains("GameMode"));
        assert!(!plist.contains("LSApplicationCategoryType"));
    }

    #[test]
    fn real_devices_require_arm64() {
        assert!(info_plist(&project(Orientation::Any), true, None).contains("<string>arm64</string>"));
        assert!(!info_plist(&project(Orientation::Any), false, None).contains("<string>arm64</string>"));
    }

    #[test]
    fn xcode_versions_read_the_way_xcode_writes_them() {
        assert_eq!(dt_xcode("26.0.1"), "2601");
        assert_eq!(dt_xcode("16.4"), "1640");
        assert_eq!(dt_xcode("27"), "2700");
        assert_eq!(dt_xcode("9.4"), "0940");
        assert_eq!(
            parse_xcodebuild_version("Xcode 27.2\nBuild version 27B52"),
            Some(("27.2".to_string(), "27B52".to_string()))
        );
        assert_eq!(parse_xcodebuild_version("nonsense"), None);
    }

    #[test]
    fn privacy_manifest_lists_only_the_apis_found() {
        let apis = apis_in_use("_malloc\n_stat$INODE64\n_mach_absolute_time\n");
        let found: Vec<&str> = apis.iter().map(|a| a.category).collect();
        assert_eq!(
            found,
            [
                "NSPrivacyAccessedAPICategoryFileTimestamp",
                "NSPrivacyAccessedAPICategorySystemBootTime",
            ]
        );
        assert!(apis_in_use("_malloc\n_statistics\n").is_empty());
        assert_eq!(apis_in_use("_OBJC_CLASS_$_NSUserDefaults\n").len(), 1);

        let xml = privacy_manifest_xml(&apis);
        assert!(xml.contains("<key>NSPrivacyTracking</key>\n    <false/>"));
        assert!(xml.contains("<string>NSPrivacyAccessedAPICategoryFileTimestamp</string>"));
        assert!(xml.contains("<string>C617.1</string>"));
        assert!(!xml.contains("DiskSpace"));
    }

    #[test]
    fn dsym_sits_next_to_the_app() {
        assert_eq!(
            dsym_path(Path::new("out/game.app")),
            Path::new("out/game.app.dSYM")
        );
    }

    #[test]
    fn simulators_prefer_the_booted_device() {
        let list = serde_json::json!({ "devices": {
            "com.apple.CoreSimulator.SimRuntime.tvOS-26": [
                { "name": "Apple TV", "udid": "tv", "state": "Booted" },
            ],
            "com.apple.CoreSimulator.SimRuntime.iOS-26": [
                { "name": "iPad", "udid": "ipad", "state": "Booted" },
                { "name": "iPhone 17", "udid": "phone", "state": "Shutdown" },
            ],
        }});
        assert_eq!(
            pick_simulator(&list),
            (Some("ipad".to_string()), Some("phone".to_string()))
        );
    }
}
