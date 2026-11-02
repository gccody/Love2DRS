//! Packages a game as an iOS `.app` bundle with Xcode's command line tools.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::project::{self, Orientation, Project, capture, run, xml_escape};
use crate::{Action, Options};

pub fn main(project: &Project, options: &Options, action: Action) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("iOS apps can only be built on macOS, because linking needs Xcode".into());
    }
    if action != Action::Build && options.physical && options.identity.is_none() {
        return Err("installing on a real device needs --identity and --profile".into());
    }
    if options.identity.is_some() != options.profile.is_some() {
        return Err("--identity and --profile only work together".into());
    }
    let app = build_app(project, options)?;
    println!("Built {}", app.display());
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

fn build_app(project: &Project, options: &Options) -> Result<PathBuf, String> {
    let triple = rust_target(options);
    project::require_rust_target(triple)?;
    let exe = compile(project, options, triple)?;

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

    copy(&exe, &app.join(&project.name))?;
    let plist = info_plist(project, options.physical);
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

/// Compiles the game and returns the executable Cargo reports.
fn compile(project: &Project, options: &Options, triple: &str) -> Result<PathBuf, String> {
    let output = Command::new(project::cargo())
        .current_dir(&project.root)
        .arg("build")
        .args(options.cargo_args(project))
        .args(["--target", triple])
        .arg("--message-format=json-render-diagnostics")
        .env("IPHONEOS_DEPLOYMENT_TARGET", &project.ios_deployment_target)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("can't run cargo: {e}"))?;
    if !output.status.success() {
        return Err("the build failed".into());
    }
    project::built_executable(&output.stdout, &project.name)
}

fn info_plist(project: &Project, physical: bool) -> String {
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
    // An empty `UILaunchScreen` is what asks for the whole screen. Without one,
    // iOS runs the app letterboxed at an old iPhone's resolution.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
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
    <key>LSRequiresIPhoneOS</key>
    <true/>
    <key>MinimumOSVersion</key>
    <string>{min_os}</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>metal</string>
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
            ios_deployment_target: "15.0".into(),
        }
    }

    #[test]
    fn plist_fills_in_the_project() {
        let plist = info_plist(&project(Orientation::Any), false);
        assert!(plist.contains("<string>com.example.my-game</string>"));
        assert!(plist.contains("<string>Tom &amp; Jerry</string>"));
        assert!(plist.contains("<string>iPhoneSimulator</string>"));
        assert!(plist.contains("UIInterfaceOrientationPortrait"));
        assert!(plist.contains("UIInterfaceOrientationLandscapeLeft"));
    }

    #[test]
    fn plist_limits_orientation() {
        let plist = info_plist(&project(Orientation::Landscape), true);
        assert!(plist.contains("<string>iPhoneOS</string>"));
        assert!(!plist.contains("UIInterfaceOrientationPortrait"));
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
