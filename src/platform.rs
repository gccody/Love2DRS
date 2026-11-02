//! What the operating system needs from a process on Android and iOS.

/// Android and iOS, where the OS owns the window and the app's lifetime.
pub(crate) const MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

/// Moves the working directory to the save directory, since the one an app
/// starts in isn't writable on mobile.
pub(crate) fn init() {
    if MOBILE {
        let dir = crate::filesystem::save_dir();
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::env::set_current_dir(&dir))
        {
            log::warn!("can't use {} as the working directory: {e}", dir.display());
        }
    }
}

/// Makes `fn main` the entry point on Android, where the OS loads the game as
/// a library instead of running an executable. Put it next to `main`. It
/// expands to nothing on other platforms, and iOS runs `main` directly.
///
/// ```no_run
/// use love2drs::prelude::*;
///
/// struct Hello;
/// impl Game for Hello {}
///
/// fn main() -> love2drs::Result {
///     love2drs::run(Conf::new("Hello"), |_ctx| Ok(Hello))
/// }
///
/// love2drs::android_main!(main);
/// ```
#[macro_export]
macro_rules! android_main {
    ($main:path) => {
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        pub fn android_main(app: $crate::winit::platform::android::activity::AndroidApp) {
            $crate::__android_entry(app, $main)
        }
    };
}

#[cfg(target_os = "android")]
pub use android::{__android_entry, MainResult};
#[cfg(target_os = "android")]
pub(crate) use android::{android_app, window_insets};
#[cfg(all(target_os = "android", feature = "gamepad"))]
pub(crate) use android::{sdk_version, with_jni};

#[cfg(target_os = "android")]
mod android {
    use std::fmt::Debug;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::OnceLock;
    use std::time::Duration;

    use jni::objects::{JObject, JValue};
    use jni::{JNIEnv, JavaVM};
    use winit::platform::android::activity::AndroidApp;

    static APP: OnceLock<AndroidApp> = OnceLock::new();

    /// How long `Activity.finish()` gets to reach the OS before the process
    /// exits. Android restarts a foreground app whose process just disappears.
    const FINISH_GRACE: Duration = Duration::from_millis(500);

    pub(crate) fn android_app() -> &'static AndroidApp {
        APP.get().expect("love2drs::android_main! sets the app")
    }

    /// Calls Android's Java APIs, which have no native equivalent. A Java exception
    /// or JNI failure gives `None`. The closure also gets the activity.
    pub(crate) fn with_jni<T>(
        f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>,
    ) -> Option<T> {
        let app = android_app();
        // SAFETY: android-activity keeps both pointers valid while the activity
        // lives, and the game only runs during that time.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        // Stays attached, because `safe_area` can get here several times per frame.
        let mut env = vm.attach_current_thread_permanently().ok()?;
        let result = env.with_local_frame(32, |env| f(env, &activity));
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        result.ok()
    }

    pub(crate) fn sdk_version(env: &mut JNIEnv) -> jni::errors::Result<i32> {
        env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
            .i()
    }

    /// How far the system bars, display cutouts and on-screen keyboard reach into
    /// the window on each side, as `(left, top, right, bottom)` in physical pixels.
    /// `None` before Android 11, which lacks this API, or while the window isn't
    /// attached.
    pub(crate) fn window_insets() -> Option<(i32, i32, i32, i32)> {
        with_jni(query_insets).flatten()
    }

    /// `activity.getWindow().getDecorView().getRootWindowInsets().getInsets(
    ///     Type.systemBars() | Type.displayCutout() | Type.ime())`
    fn query_insets(
        env: &mut JNIEnv,
        activity: &JObject,
    ) -> jni::errors::Result<Option<(i32, i32, i32, i32)>> {
        const ANDROID_11: i32 = 30;
        if sdk_version(env)? < ANDROID_11 {
            return Ok(None);
        }
        let window = env
            .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
            .l()?;
        let view = env
            .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
            .l()?;
        let root = env
            .call_method(
                &view,
                "getRootWindowInsets",
                "()Landroid/view/WindowInsets;",
                &[],
            )?
            .l()?;
        if root.is_null() {
            return Ok(None);
        }
        let inset_type = |env: &mut JNIEnv, name: &str| {
            env.call_static_method("android/view/WindowInsets$Type", name, "()I", &[])?
                .i()
        };
        let mask = inset_type(env, "systemBars")?
            | inset_type(env, "displayCutout")?
            | inset_type(env, "ime")?;
        let insets = env
            .call_method(
                &root,
                "getInsets",
                "(I)Landroid/graphics/Insets;",
                &[JValue::Int(mask)],
            )?
            .l()?;
        let mut side = |name: &str| env.get_field(&insets, name, "I")?.i();
        Ok(Some((
            side("left")?,
            side("top")?,
            side("right")?,
            side("bottom")?,
        )))
    }

    /// What `fn main` may return: nothing or a `Result`.
    #[doc(hidden)]
    pub trait MainResult {
        #[doc(hidden)]
        fn log_error(self);
    }

    impl MainResult for () {
        fn log_error(self) {}
    }

    impl<E: Debug> MainResult for Result<(), E> {
        fn log_error(self) {
            if let Err(e) = self {
                log::error!("the game stopped with an error: {e:?}");
            }
        }
    }

    #[doc(hidden)]
    pub fn __android_entry<R: MainResult>(app: AndroidApp, main: fn() -> R) {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("love2drs"),
        );
        std::panic::set_hook(Box::new(|info| log::error!("{info}")));

        // winit allows one event loop per process, and Android can start a new
        // activity in the old process. Exiting makes the OS start a clean one.
        if APP.set(app).is_err() {
            log::warn!("restarting the process for a new activity");
            std::process::exit(0);
        }

        crate::platform::init();
        if let Ok(result) = catch_unwind(AssertUnwindSafe(main)) {
            result.log_error();
        }

        // Returning makes android-activity finish the activity. The process
        // has to go too, or the next launch would need a second event loop.
        std::thread::spawn(|| {
            std::thread::sleep(FINISH_GRACE);
            std::process::exit(0);
        });
    }
}
