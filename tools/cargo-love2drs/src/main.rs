//! `cargo love2drs`: builds, installs and runs love2drs games on Android and iOS.

mod android;
mod ios;
mod project;

use project::Target;

const USAGE: &str = "\
Builds, installs and runs love2drs games on Android and iOS.

USAGE:
    cargo love2drs <android|ios> <build|install|run> [OPTIONS]

    build     package the game: an .apk on Android, an .app on iOS
    install   build, then put it on the device
    run       install, then launch it and show its log (Ctrl-C to stop watching)

WHAT TO BUILD:
    --bin <NAME>            a binary of the package (needed if it has several)
    --example <NAME>        an example of the package
    -p, --package <NAME>    a package of the workspace
    --release               optimize; also used for store builds
    --features <LIST>       Cargo features to enable
    --no-default-features   turn off the default features

ANDROID:
    --device <SERIAL>       which device, as listed by `adb devices`
    --abi <ABI>             arm64-v8a or x86_64; repeat for both. Default: the
                            device's for install/run, both for build
    --keystore <PATH>       release keystore; its password is read from the
                            LOVE2DRS_KEYSTORE_PASS environment variable
    --key-alias <ALIAS>     key to sign with (default: the keystore's first)

IOS (needs macOS with Xcode):
    --device <UDID>         which device (default: the booted simulator)
    --physical              build for a real iPhone or iPad, not the simulator
    --identity <NAME>       code signing identity, for --physical
    --profile <PATH>        provisioning profile, for --physical

The game's `fn main` needs `love2drs::android_main!(main);` next to it. Options
for the app itself live in the game's Cargo.toml:

    [package.metadata.love2drs]
    app_id = \"com.example.mygame\"    # default: com.love2drs.<name>
    name = \"My Game\"                 # default: the Cargo name
    assets = [\"assets\"]              # files to package, from the package root
    orientation = \"landscape\"        # landscape, portrait or any (default)
    version_code = 3                 # default: derived from the version
    min_sdk = 26                     # Android
    target_sdk = 36                  # Android
    permissions = [\"android.permission.INTERNET\"]  # Android
    ios_deployment_target = \"15.0\"   # iOS
";

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Build,
    Install,
    Run,
}

/// The parsed command line.
#[derive(Default)]
pub struct Options {
    pub target: Target,
    pub package: Option<String>,
    pub release: bool,
    pub features: Option<String>,
    pub no_default_features: bool,
    pub device: Option<String>,
    pub abis: Vec<String>,
    pub keystore: Option<String>,
    pub key_alias: Option<String>,
    pub physical: bool,
    pub identity: Option<String>,
    pub profile: Option<String>,
}

impl Options {
    /// The flags that select what Cargo builds.
    pub fn cargo_args(&self, project: &project::Project) -> Vec<String> {
        let mut args = vec!["-p".to_string(), project.package.clone()];
        args.extend(project.target.cargo_args());
        if self.release {
            args.push("--release".into());
        }
        if let Some(features) = &self.features {
            args.extend(["--features".into(), features.clone()]);
        }
        if self.no_default_features {
            args.push("--no-default-features".into());
        }
        args
    }

    pub fn profile_dir(&self) -> &'static str {
        if self.release { "release" } else { "debug" }
    }
}

fn parse_options(args: &mut impl Iterator<Item = String>) -> Result<Options, String> {
    let mut o = Options::default();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--bin" => o.target = Target::Bin(value()?),
            "--example" => o.target = Target::Example(value()?),
            "-p" | "--package" => o.package = Some(value()?),
            "--release" => o.release = true,
            "--features" => o.features = Some(value()?),
            "--no-default-features" => o.no_default_features = true,
            "--device" => o.device = Some(value()?),
            "--abi" => o.abis.push(value()?),
            "--keystore" => o.keystore = Some(value()?),
            "--key-alias" => o.key_alias = Some(value()?),
            "--physical" => o.physical = true,
            "--identity" => o.identity = Some(value()?),
            "--profile" => o.profile = Some(value()?),
            other => return Err(format!("unknown option `{other}`\n\n{USAGE}")),
        }
    }
    Ok(o)
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1).peekable();
    // Cargo passes the subcommand's name when it runs `cargo-love2drs`.
    if args.peek().is_some_and(|a| a == "love2drs") {
        args.next();
    }
    let platform = args.next().unwrap_or_default();
    if matches!(platform.as_str(), "" | "help" | "-h" | "--help") {
        print!("{USAGE}");
        return Ok(());
    }
    let action = match args.next().as_deref() {
        Some("build") => Action::Build,
        Some("install") => Action::Install,
        Some("run") => Action::Run,
        other => {
            return Err(format!(
                "expected build, install or run, got `{}`\n\n{USAGE}",
                other.unwrap_or("")
            ));
        }
    };
    let options = parse_options(&mut args)?;
    let project = project::Project::load(&options)?;
    match platform.as_str() {
        "android" => android::main(&project, &options, action),
        "ios" => ios::main(&project, &options, action),
        other => Err(format!(
            "unknown platform `{other}`, expected android or ios"
        )),
    }
}

fn main() {
    if let Some(code) = android::rustc_wrapper() {
        std::process::exit(code);
    }
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
