//! Packages a game as an APK with the Android SDK's own tools.
//!
//! The game runs under `NativeActivity`, so there's no Java to compile and no
//! need for Gradle: Cargo builds a shared library, `aapt2` makes the APK around
//! the manifest, and the library and assets are added to it before `zipalign`
//! and `apksigner` finish it.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::project::{self, Orientation, Project, capture, run, xml_escape};
use crate::{Action, Options};

/// `zipalign -P` value and linker page size. Android 15+ devices may use 16 KB
/// pages, and Google Play requires apps to support them.
const PAGE_SIZE_KB: u32 = 16;
const KEYSTORE_PASS_VAR: &str = "LOVE2DRS_KEYSTORE_PASS";
/// Names the crate to turn into a library when running as a rustc wrapper.
const WRAPPED_CRATE_VAR: &str = "LOVE2DRS_WRAPPED_CRATE";

pub fn main(project: &Project, options: &Options, action: Action) -> Result<(), String> {
    let sdk = Sdk::find()?;
    let device = (action != Action::Build)
        .then(|| Device::connect(&sdk, options))
        .transpose()?;
    let abis = choose_abis(options, device.as_ref())?;
    let apk = build_apk(project, options, &sdk, &abis)?;
    println!("Built {}", apk.display());

    let Some(device) = device else {
        return Ok(());
    };
    device.install(&apk)?;
    println!("Installed {}", project.android_id());
    if action == Action::Run {
        device.launch(&project.android_id())?;
        device.show_log()?;
    }
    Ok(())
}

/// An Android ABI and the Rust target that builds for it.
#[derive(Clone, Copy)]
struct Abi {
    name: &'static str,
    triple: &'static str,
}

const ABIS: [Abi; 2] = [
    Abi {
        name: "arm64-v8a",
        triple: "aarch64-linux-android",
    },
    Abi {
        name: "x86_64",
        triple: "x86_64-linux-android",
    },
];

fn choose_abis(options: &Options, device: Option<&Device>) -> Result<Vec<Abi>, String> {
    let find = |name: &str| {
        ABIS.iter()
            .copied()
            .find(|abi| abi.name == name)
            .ok_or(format!(
                "ABI `{name}` isn't supported; use arm64-v8a or x86_64"
            ))
    };
    if !options.abis.is_empty() {
        return options.abis.iter().map(|abi| find(abi)).collect();
    }
    match device {
        Some(device) => Ok(vec![find(&device.abi()?)?]),
        None => Ok(ABIS.to_vec()),
    }
}

// ----------------------------------------------------------------------
// Finding the SDK
// ----------------------------------------------------------------------

struct Sdk {
    root: PathBuf,
    ndk: PathBuf,
    build_tools: PathBuf,
    android_jar: PathBuf,
}

impl Sdk {
    fn find() -> Result<Sdk, String> {
        let root = sdk_root().ok_or(
            "can't find the Android SDK: install Android Studio or set ANDROID_HOME".to_string(),
        )?;
        let env_ndk = ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]
            .iter()
            .find_map(|v| std::env::var_os(v).map(PathBuf::from))
            .filter(|p| p.is_dir());
        let ndk = env_ndk
            .or_else(|| newest_child(&root.join("ndk"), ""))
            .ok_or(
                "can't find the Android NDK: install it from Android Studio's SDK Manager, or set \
             ANDROID_NDK_HOME",
            )?;
        let build_tools = newest_child(&root.join("build-tools"), "").ok_or(
            "can't find the Android build-tools: install them from Android Studio's SDK Manager",
        )?;
        let platform = newest_child(&root.join("platforms"), "android-").ok_or(
            "can't find an Android platform: install one from Android Studio's SDK Manager",
        )?;
        Ok(Sdk {
            root,
            ndk,
            build_tools,
            android_jar: platform.join("android.jar"),
        })
    }

    fn tool(&self, name: &str) -> PathBuf {
        self.build_tools.join(exe(name))
    }

    fn adb(&self) -> Command {
        Command::new(self.root.join("platform-tools").join(exe("adb")))
    }

    fn apksigner(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "apksigner.bat"
        } else {
            "apksigner"
        };
        self.build_tools.join(name)
    }

    /// The NDK's LLVM tools for this kind of computer.
    fn llvm_bin(&self) -> Result<PathBuf, String> {
        let host = match std::env::consts::OS {
            "windows" => "windows-x86_64",
            "macos" => "darwin-x86_64",
            _ => "linux-x86_64",
        };
        let bin = self
            .ndk
            .join("toolchains/llvm/prebuilt")
            .join(host)
            .join("bin");
        if bin.is_dir() {
            Ok(bin)
        } else {
            Err(format!("the NDK has no toolchain at {}", bin.display()))
        }
    }
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn sdk_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let home = Path::new(&home);
    let local = std::env::var_os("LOCALAPPDATA").map(|d| Path::new(&d).join("Android/Sdk"));
    ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .chain(local)
        .chain([home.join("Library/Android/sdk"), home.join("Android/Sdk")])
        .find(|p| p.is_dir())
}

/// The subdirectory with the highest version number, like `build-tools/37.0.0`
/// or `platforms/android-37.0`.
fn newest_child(dir: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            let version = parse_version(name.strip_prefix(prefix)?)?;
            entry.path().is_dir().then(|| (version, entry.path()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path)| path)
}

fn parse_version(text: &str) -> Option<Vec<u32>> {
    text.split('.').map(|part| part.parse().ok()).collect()
}

// ----------------------------------------------------------------------
// Building
// ----------------------------------------------------------------------

fn build_apk(
    project: &Project,
    options: &Options,
    sdk: &Sdk,
    abis: &[Abi],
) -> Result<PathBuf, String> {
    let lib_name = project.name.replace('-', "_");
    let out = project.out_dir("android")?;
    let work = out.join(format!("{}-{}", project.name, options.profile_dir()));
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;

    let mut libs = Vec::new();
    for abi in abis {
        let built = compile(project, options, sdk, *abi)?;
        let stripped = work.join(format!("lib{lib_name}-{}.so", abi.name));
        let strip_level = if options.release {
            "--strip-all"
        } else {
            "--strip-debug"
        };
        run(
            Command::new(sdk.llvm_bin()?.join(exe("llvm-strip")))
                .arg(strip_level)
                .arg(&built)
                .arg("-o")
                .arg(&stripped),
            "",
        )?;
        libs.push((stripped, format!("lib/{}/lib{lib_name}.so", abi.name)));
    }

    let manifest = work.join("AndroidManifest.xml");
    let manifest_text =
        android_manifest(project, &lib_name, !options.release, project.icon.is_some());
    std::fs::write(&manifest, manifest_text).map_err(|e| format!("{}: {e}", manifest.display()))?;

    let base = work.join("base.apk");
    let mut aapt2 = Command::new(sdk.tool("aapt2"));
    aapt2
        .arg("link")
        .arg("-o")
        .arg(&base)
        .arg("-I")
        .arg(&sdk.android_jar)
        .arg("--manifest")
        .arg(&manifest)
        .args(["--min-sdk-version", &project.min_sdk.to_string()])
        .args(["--target-sdk-version", &project.target_sdk.to_string()])
        .args(["--version-code", &project.version_code.to_string()])
        .args(["--version-name", &project.version]);
    if !options.release {
        aapt2.arg("--debug-mode");
    }
    if let Some(icon) = &project.icon {
        aapt2.arg(compile_icon(sdk, &project.root.join(icon), &work)?);
    }
    run(&mut aapt2, "")?;

    let unaligned = work.join("unaligned.apk");
    add_files(&base, &unaligned, &libs, &project.asset_files()?)?;

    let aligned = work.join("aligned.apk");
    run(
        Command::new(sdk.tool("zipalign"))
            .args(["-P", &PAGE_SIZE_KB.to_string(), "-f", "4"])
            .arg(&unaligned)
            .arg(&aligned),
        "",
    )?;

    let apk = out.join(format!("{}-{}.apk", project.name, options.profile_dir()));
    sign(sdk, options, &aligned, &apk)?;
    Ok(apk)
}

/// Compiles the game into a shared library and returns its path.
///
/// Android loads games as libraries, but Cargo won't build a binary or example
/// as one. So that the game's manifest can stay as it is, [`rustc_wrapper`]
/// changes the crate type behind Cargo's back. With `--no-undefined`, a missing
/// `android_main!` fails here with a link error, not as a crash on the device.
fn compile(project: &Project, options: &Options, sdk: &Sdk, abi: Abi) -> Result<PathBuf, String> {
    project::require_rust_target(abi.triple)?;
    let llvm = sdk.llvm_bin()?;
    let env_triple = abi.triple.replace('-', "_");
    let wrapper_suffix = if cfg!(windows) { ".cmd" } else { "" };
    let clang_wrapper = |tool: &str| {
        llvm.join(format!(
            "{}{}-{tool}{wrapper_suffix}",
            abi.triple, project.min_sdk
        ))
    };
    let this_tool = std::env::current_exe().map_err(|e| format!("can't find this tool: {e}"))?;
    let output = Command::new(project::cargo())
        .current_dir(&project.root)
        .arg("rustc")
        .args(options.cargo_args(project))
        .args(["--target", abi.triple])
        .arg("--message-format=json-render-diagnostics")
        .env("RUSTC_WORKSPACE_WRAPPER", this_tool)
        .env(WRAPPED_CRATE_VAR, project.name.replace('-', "_"))
        .env(
            format!("CARGO_TARGET_{}_LINKER", env_triple.to_uppercase()),
            llvm.join(exe("clang")),
        )
        // For dependencies that compile C or C++.
        .env(format!("CC_{env_triple}"), clang_wrapper("clang"))
        .env(format!("CXX_{env_triple}"), clang_wrapper("clang++"))
        .env(format!("AR_{env_triple}"), llvm.join(exe("llvm-ar")))
        .env("ANDROID_NDK_HOME", &sdk.ndk)
        .env("ANDROID_NDK_ROOT", &sdk.ndk)
        .arg("--")
        .arg(format!(
            "-Clink-arg=--target={}{}",
            abi.triple, project.min_sdk
        ))
        .arg(format!(
            "-Clink-arg=-Wl,-z,max-page-size={}",
            PAGE_SIZE_KB * 1024
        ))
        .arg("-Clink-arg=-Wl,--no-undefined")
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("can't run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "the build failed. If the error is an undefined symbol `android_main`, add \
             `love2drs::android_main!(main);` next to `fn main` in {}",
            project.name
        ));
    }
    project::built_executable(&output.stdout, &project.name)
}

/// Runs as `rustc` during an Android build (see [`compile`]), turning the game's
/// crate into a library. Cargo still expects the executable it asked for, so the
/// library is also copied to that name.
///
/// Returns `None` if this process isn't acting as a wrapper.
pub fn rustc_wrapper() -> Option<i32> {
    let game = std::env::var(WRAPPED_CRATE_VAR).ok()?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let rustc = args.remove(0);
    let output = retarget_game_crate(&mut args, &game);
    let code = Command::new(rustc)
        .args(&args)
        .status()
        .map_or(101, |status| status.code().unwrap_or(101));
    if code == 0
        && let Some((library, executable)) = output
        && let Err(e) = std::fs::copy(&library, &executable)
    {
        eprintln!("error: {}: {e}", library.display());
        return Some(1);
    }
    Some(code)
}

/// Changes `--crate-type bin` to `cdylib` if these are rustc's arguments for the
/// game, and returns where the library will appear and where Cargo wants it.
fn retarget_game_crate(args: &mut [String], game: &str) -> Option<(PathBuf, PathBuf)> {
    let value_of = |args: &[String], flag: &str| {
        let i = args.iter().position(|a| a == flag)?;
        args.get(i + 1).cloned()
    };
    if value_of(args, "--crate-name")? != game {
        return None;
    }
    let crate_type = args.iter().position(|a| a == "--crate-type")? + 1;
    if args.get(crate_type)? != "bin" {
        return None;
    }
    args[crate_type] = "cdylib".into();
    let out_dir = PathBuf::from(value_of(args, "--out-dir")?);
    let hash = args
        .iter()
        .find_map(|a| a.strip_prefix("extra-filename="))
        .unwrap_or("");
    Some((
        out_dir.join(format!("lib{game}{hash}.so")),
        out_dir.join(format!("{game}{hash}")),
    ))
}

fn android_manifest(project: &Project, lib_name: &str, debuggable: bool, has_icon: bool) -> String {
    let orientation = match project.orientation {
        // The `user` variants respect the rotation lock.
        Orientation::Any => "fullUser",
        Orientation::Landscape => "userLandscape",
        Orientation::Portrait => "userPortrait",
    };
    // Controller rumble needs this. Android grants it without asking the user.
    let permissions: String = std::iter::once("android.permission.VIBRATE")
        .chain(project.permissions.iter().map(String::as_str))
        .map(|p| {
            format!(
                "    <uses-permission android:name=\"{}\" />\n",
                xml_escape(p)
            )
        })
        .collect();
    let package = xml_escape(&project.android_id());
    let label = xml_escape(&project.display_name);
    let icon = if has_icon {
        "\n        android:icon=\"@mipmap/ic_launcher\""
    } else {
        ""
    };
    // `configChanges`: the game handles rotation itself through `resize`. Android
    // would otherwise destroy the activity, and its game state with it.
    //
    // `enableOnBackInvokedCallback`: predictive back would keep the back gesture
    // from reaching the game as `Key::Escape`.
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{package}">
    <uses-feature android:glEsVersion="0x00030000" android:required="true" />
{permissions}    <application
        android:label="{label}"{icon}
        android:hasCode="false"
        android:debuggable="{debuggable}"
        android:extractNativeLibs="false"
        android:enableOnBackInvokedCallback="false"
        android:theme="@android:style/Theme.DeviceDefault.NoActionBar.Fullscreen">
        <activity
            android:name="android.app.NativeActivity"
            android:exported="true"
            android:launchMode="singleTask"
            android:screenOrientation="{orientation}"
            android:configChanges="orientation|screenSize|smallestScreenSize|screenLayout|keyboard|keyboardHidden|navigation|density|fontScale|uiMode|locale|layoutDirection">
            <meta-data android:name="android.app.lib_name" android:value="{lib_name}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#
    )
}

/// Writes the launcher icon at every density as Android resources and compiles
/// them. Returns the file `aapt2 link` takes.
fn compile_icon(sdk: &Sdk, icon: &Path, work: &Path) -> Result<PathBuf, String> {
    let res = work.join("res");
    if res.exists() {
        std::fs::remove_dir_all(&res).map_err(|e| format!("{}: {e}", res.display()))?;
    }
    let source = crate::icon::load(icon)?;
    let write = |dir: &str, name: &str, image: &image::RgbaImage| {
        let dir = res.join(dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        image
            .save(dir.join(name))
            .map_err(|e| format!("can't write the icon: {e}"))
    };
    // The launcher icon for each screen density, used before Android 8.
    for (density, size) in [
        ("mdpi", 48),
        ("hdpi", 72),
        ("xhdpi", 96),
        ("xxhdpi", 144),
        ("xxxhdpi", 192),
    ] {
        let image = crate::icon::resized(&source, size);
        write(&format!("mipmap-{density}"), "ic_launcher.png", &image)?;
    }
    // Since Android 8, launchers cut icons into shapes of their own from two 108 dp
    // layers, of which only the middle 72 dp is sure to show. Fitting the icon
    // there and filling the rest with its corner color keeps all of it visible.
    let foreground = crate::icon::centered(&source, 432, 288);
    write("mipmap-xxxhdpi", "ic_launcher_foreground.png", &foreground)?;
    let values = res.join("values");
    std::fs::create_dir_all(&values).map_err(|e| format!("{}: {e}", values.display()))?;
    let [r, g, b, _] = crate::icon::corner_color(&source);
    std::fs::write(
        values.join("colors.xml"),
        format!(
            "<resources><color name=\"ic_launcher_background\">#{r:02x}{g:02x}{b:02x}</color></resources>\n"
        ),
    )
    .map_err(|e| format!("can't write the icon: {e}"))?;
    let adaptive = res.join("mipmap-anydpi-v26");
    std::fs::create_dir_all(&adaptive).map_err(|e| format!("{}: {e}", adaptive.display()))?;
    std::fs::write(
        adaptive.join("ic_launcher.xml"),
        "<adaptive-icon xmlns:android=\"http://schemas.android.com/apk/res/android\">\n\
         <background android:drawable=\"@color/ic_launcher_background\" />\n\
         <foreground android:drawable=\"@mipmap/ic_launcher_foreground\" />\n\
         </adaptive-icon>\n",
    )
    .map_err(|e| format!("can't write the icon: {e}"))?;

    let compiled = work.join("res.zip");
    run(
        Command::new(sdk.tool("aapt2"))
            .arg("compile")
            .arg("--dir")
            .arg(&res)
            .arg("-o")
            .arg(&compiled),
        "",
    )?;
    Ok(compiled)
}

/// Copies the APK from `aapt2` and adds native libraries and assets to it.
/// Libraries are stored uncompressed so Android can map them straight from the
/// APK (`extractNativeLibs="false"`).
fn add_files(
    base: &Path,
    out: &Path,
    libs: &[(PathBuf, String)],
    assets: &[(PathBuf, String)],
) -> Result<(), String> {
    let describe = |e: &dyn std::fmt::Display| format!("{}: {e}", out.display());
    let base_file = File::open(base).map_err(|e| describe(&e))?;
    let mut input = ZipArchive::new(base_file).map_err(|e| describe(&e))?;
    let out_file = File::create(out).map_err(|e| describe(&e))?;
    let mut zip = ZipWriter::new(out_file);
    for i in 0..input.len() {
        let entry = input.by_index_raw(i).map_err(|e| describe(&e))?;
        zip.raw_copy_file(entry).map_err(|e| describe(&e))?;
    }
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let entries = libs
        .iter()
        .map(|(path, name)| (path, name.clone(), stored))
        .chain(
            assets
                .iter()
                .map(|(path, name)| (path, format!("assets/{name}"), deflated)),
        );
    for (path, name, options) in entries {
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        zip.start_file(name, options).map_err(|e| describe(&e))?;
        std::io::copy(&mut file, &mut zip).map_err(|e| describe(&e))?;
    }
    zip.finish().map_err(|e| describe(&e))?;
    Ok(())
}

// ----------------------------------------------------------------------
// Signing
// ----------------------------------------------------------------------

fn sign(sdk: &Sdk, options: &Options, input: &Path, output: &Path) -> Result<(), String> {
    let mut signer = Command::new(sdk.apksigner());
    signer.arg("sign");
    if let Some(keystore) = &options.keystore {
        if std::env::var_os(KEYSTORE_PASS_VAR).is_none() {
            return Err(format!(
                "set {KEYSTORE_PASS_VAR} to the password of the keystore"
            ));
        }
        signer
            .args(["--ks", keystore])
            .args(["--ks-pass", &format!("env:{KEYSTORE_PASS_VAR}")]);
        if let Some(alias) = &options.key_alias {
            signer.args(["--ks-key-alias", alias]);
        }
    } else {
        if options.release {
            println!(
                "warning: signing with the debug key; stores need a real one (--keystore <PATH>)"
            );
        }
        let keystore = debug_keystore()?;
        signer
            .arg("--ks")
            .arg(keystore)
            .args(["--ks-pass", "pass:android"])
            .args(["--key-pass", "pass:android"])
            .args(["--ks-key-alias", "androiddebugkey"]);
    }
    signer.arg("--out").arg(output).arg(input);
    run(&mut signer, "apksigner needs Java: install a JDK")
}

/// The debug key Android Studio uses, created the same way if it's missing.
fn debug_keystore() -> Result<PathBuf, String> {
    let home = std::env::var_os("ANDROID_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| Path::new(&home).join(".android"))
        })
        .ok_or("can't find the home directory")?;
    let path = home.join("debug.keystore");
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let keytool = std::env::var_os("JAVA_HOME")
        .map(|java| Path::new(&java).join("bin").join(exe("keytool")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("keytool"));
    println!("Creating the debug key at {}", path.display());
    run(
        Command::new(keytool)
            .args(["-genkeypair", "-keystore"])
            .arg(&path)
            .args(["-storepass", "android", "-keypass", "android"])
            .args(["-alias", "androiddebugkey"])
            .args(["-keyalg", "RSA", "-keysize", "2048", "-validity", "10000"])
            .args(["-dname", "CN=Android Debug,O=Android,C=US"]),
        "keytool comes with a JDK: install one or set JAVA_HOME",
    )?;
    Ok(path)
}

// ----------------------------------------------------------------------
// Devices
// ----------------------------------------------------------------------

struct Device<'a> {
    sdk: &'a Sdk,
    serial: String,
}

impl<'a> Device<'a> {
    fn connect(sdk: &'a Sdk, options: &Options) -> Result<Device<'a>, String> {
        let list = capture(sdk.adb().arg("devices")).ok_or("can't run adb")?;
        let online: Vec<&str> = list
            .lines()
            .skip(1)
            .filter_map(|line| line.strip_suffix("\tdevice"))
            .collect();
        let serial = match (&options.device, &online[..]) {
            (Some(wanted), _) if online.contains(&wanted.as_str()) => wanted.clone(),
            (Some(wanted), _) => return Err(format!("device `{wanted}` isn't connected")),
            (None, []) => {
                return Err("no Android device: start an emulator or plug in a phone".into());
            }
            (None, [only]) => only.to_string(),
            (None, many) => {
                return Err(format!(
                    "several devices are connected; choose one with --device: {}",
                    many.join(", ")
                ));
            }
        };
        let device = Device { sdk, serial };
        device.wait_for_boot()?;
        Ok(device)
    }

    /// `adb devices` lists an emulator long before it can install anything.
    fn wait_for_boot(&self) -> Result<(), String> {
        let booted = || {
            capture(self.adb().args(["shell", "getprop", "sys.boot_completed"]))
                .is_some_and(|value| value == "1")
        };
        if !booted() {
            println!("Waiting for {} to finish booting", self.serial);
            run(
                self.adb().args([
                    "shell",
                    "while [ \"$(getprop sys.boot_completed)\" != 1 ]; do sleep 1; done",
                ]),
                "",
            )?;
        }
        Ok(())
    }

    fn adb(&self) -> Command {
        let mut adb = self.sdk.adb();
        adb.args(["-s", &self.serial]);
        adb
    }

    fn abi(&self) -> Result<String, String> {
        capture(self.adb().args(["shell", "getprop", "ro.product.cpu.abi"]))
            .ok_or("can't ask the device for its ABI".to_string())
    }

    fn install(&self, apk: &Path) -> Result<(), String> {
        run(self.adb().args(["install", "-r", "-t"]).arg(apk), "")
    }

    fn launch(&self, package: &str) -> Result<(), String> {
        let component = format!("{package}/android.app.NativeActivity");
        run(
            self.adb()
                .args(["shell", "am", "start", "-W", "-S", "-n", &component]),
            "",
        )
    }

    /// Follows the game's log until the user stops watching.
    fn show_log(&self) -> Result<(), String> {
        run(
            self.adb()
                .args(["logcat", "-T", "1", "-s", "love2drs:V", "RustPanic:V"]),
            "",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Target;

    fn project() -> Project {
        Project {
            package: "game".into(),
            root: ".".into(),
            target_dir: "target".into(),
            target: Target::Bin("my-game".into()),
            name: "my-game".into(),
            display_name: "Tom & Jerry".into(),
            app_id: "com.example.my-game".into(),
            version: "1.2.3".into(),
            version_code: 10203,
            orientation: Orientation::Landscape,
            assets: vec![],
            min_sdk: 26,
            target_sdk: 36,
            permissions: vec!["android.permission.INTERNET".into()],
            icon: None,
            ios_deployment_target: "15.0".into(),
        }
    }

    #[test]
    fn manifest_fills_in_the_project() {
        let xml = android_manifest(&project(), "my_game", true, true);
        assert!(xml.contains(r#"android:icon="@mipmap/ic_launcher""#));
        assert!(xml.contains("android.permission.VIBRATE"));
        assert!(xml.contains(r#"package="com.example.my_game""#));
        assert!(xml.contains(r#"android:label="Tom &amp; Jerry""#));
        assert!(xml.contains(r#"android:debuggable="true""#));
        assert!(xml.contains(r#"android:value="my_game""#));
        assert!(xml.contains(r#"android:screenOrientation="userLandscape""#));
        assert!(xml.contains(r#"<uses-permission android:name="android.permission.INTERNET" />"#));
    }

    #[test]
    fn only_the_game_crate_becomes_a_library() {
        let args = |name: &str, kind: &str| -> Vec<String> {
            [
                "--crate-name",
                name,
                "--crate-type",
                kind,
                "--out-dir",
                "out",
                "-C",
                "extra-filename=-abc",
            ]
            .map(String::from)
            .to_vec()
        };
        let mut game = args("my_game", "bin");
        let (library, executable) = retarget_game_crate(&mut game, "my_game").unwrap();
        assert_eq!(game[3], "cdylib");
        assert_eq!(library, Path::new("out/libmy_game-abc.so"));
        assert_eq!(executable, Path::new("out/my_game-abc"));

        let mut dependency = args("wgpu", "lib");
        assert_eq!(retarget_game_crate(&mut dependency, "my_game"), None);
        assert_eq!(dependency[3], "lib");
        let mut game_lib = args("my_game", "lib");
        assert_eq!(retarget_game_crate(&mut game_lib, "my_game"), None);
        assert_eq!(game_lib[3], "lib");
    }

    #[test]
    fn versions_sort_as_numbers() {
        assert!(parse_version("37.0.0") > parse_version("9.0.0"));
        assert!(parse_version("37.0") > parse_version("36"));
        assert_eq!(parse_version("37-ext"), None);
    }
}
