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

#[cfg(target_os = "ios")]
pub(crate) use ios::{FrameLink, SceneWindow};

#[cfg(target_os = "ios")]
mod ios {
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2::encode::{Encode, Encoding};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{ClassType, class, define_class, msg_send, sel};
    use objc2_foundation::NSString;

    /// Set by the display link when the display is about to refresh.
    static TICKED: AtomicBool = AtomicBool::new(false);

    define_class!(
        // SAFETY: `NSObject` asks nothing of its subclasses, and this one has no `Drop`.
        #[unsafe(super(NSObject))]
        #[name = "Love2drsFrameLinkTarget"]
        struct LinkTarget;

        impl LinkTarget {
            #[unsafe(method(tick:))]
            fn tick(&self, _link: &AnyObject) {
                TICKED.store(true, Ordering::Relaxed);
            }
        }
    );

    /// `CAFrameRateRange`.
    #[repr(C)]
    struct FrameRateRange {
        minimum: f32,
        maximum: f32,
        preferred: f32,
    }

    // SAFETY: Three floats, under the name Core Animation gives the struct.
    unsafe impl Encode for FrameRateRange {
        const ENCODING: Encoding =
            Encoding::Struct("CAFrameRateRange", &[f32::ENCODING, f32::ENCODING, f32::ENCODING]);
    }

    /// A `CADisplayLink`: wakes the event loop once for every refresh of the
    /// display, and tells iOS how fast the game wants it refreshed.
    ///
    /// That second part is the only way to be shown more than 60 frames a second.
    /// A display that can do 120 runs at 60 or less until a display link asks for
    /// more, however fast the app presents. (An iPhone also needs
    /// `CADisableMinimumFrameDurationOnPhone` in its Info.plist, which `cargo
    /// love2drs` writes.)
    pub(crate) struct FrameLink {
        link: Retained<AnyObject>,
        running: bool,
        rate: f32,
    }

    impl FrameLink {
        /// Starts out paused. Call on the main thread.
        pub(crate) fn new() -> Self {
            unsafe extern "C" {
                static NSRunLoopCommonModes: &'static NSString;
            }
            // SAFETY: The event loop calls this on the main thread, and these are
            // QuartzCore's and Foundation's documented signatures. The link keeps
            // its target alive.
            let link = unsafe {
                let target: Retained<LinkTarget> = msg_send![LinkTarget::class(), new];
                let link: Retained<AnyObject> =
                    msg_send![class!(CADisplayLink), displayLinkWithTarget: &*target, selector: sel!(tick:)];
                let _: () = msg_send![&*link, setPaused: true];
                let run_loop: *mut AnyObject = msg_send![class!(NSRunLoop), mainRunLoop];
                let _: () = msg_send![&*link, addToRunLoop: run_loop, forMode: NSRunLoopCommonModes];
                link
            };
            FrameLink { link, running: false, rate: 0.0 }
        }

        /// Ticks at the display's rate of `rate` frames a second from now on.
        pub(crate) fn run(&mut self, rate: f32) {
            if self.rate != rate {
                self.rate = rate;
                // What Apple suggests for a game: the full rate, and no less than
                // two thirds of it when the system has to hold back (80 of 120).
                let range = FrameRateRange { minimum: rate * 2.0 / 3.0, maximum: rate, preferred: rate };
                // SAFETY: iOS 15's signature, which is the oldest iOS this builds for.
                let _: () = unsafe { msg_send![&*self.link, setPreferredFrameRateRange: range] };
            }
            if !std::mem::replace(&mut self.running, true) {
                TICKED.store(false, Ordering::Relaxed);
                // SAFETY: A `BOOL` property.
                let _: () = unsafe { msg_send![&*self.link, setPaused: false] };
            }
        }

        /// Stops the ticks, and with them the request for a fast display.
        pub(crate) fn pause(&mut self) {
            if std::mem::take(&mut self.running) {
                // SAFETY: A `BOOL` property.
                let _: () = unsafe { msg_send![&*self.link, setPaused: true] };
            }
        }

        /// Whether the display has refreshed since this was last asked, while running.
        pub(crate) fn take_tick(&self) -> bool {
            TICKED.swap(false, Ordering::Relaxed) && self.running
        }
    }

    impl Drop for FrameLink {
        fn drop(&mut self) {
            // SAFETY: Takes the link off the run loop, which lets go of the target.
            let _: () = unsafe { msg_send![&*self.link, invalidate] };
        }
    }

    /// A window that's key only while winit makes the real one.
    ///
    /// winit makes its window with `initWithFrame:` and then attaches it to the
    /// scene. Under the UIScene lifecycle, UIKit then leaves that window's safe area
    /// as it was in the first orientation, so landscape gets portrait's insets. That
    /// doesn't happen if a window made with `initWithWindowScene:` was key first.
    /// This can go once winit makes its window from the scene.
    pub(crate) struct SceneWindow(Retained<AnyObject>);

    impl SceneWindow {
        /// `None` if no window scene is connected. Call on the main thread.
        pub(crate) fn show() -> Option<Self> {
            // SAFETY: The event loop calls this on the main thread, and these are
            // UIKit's documented signatures for iOS 13 and up.
            unsafe {
                let app: *mut AnyObject = msg_send![class!(UIApplication), sharedApplication];
                let scenes: *mut AnyObject = msg_send![app, connectedScenes];
                let scenes: *mut AnyObject = msg_send![scenes, allObjects];
                let count: usize = msg_send![scenes, count];
                let scene = (0..count)
                    .map(|i| -> *mut AnyObject { msg_send![scenes, objectAtIndex: i] })
                    .find(|&scene| {
                        let is_window_scene: bool =
                            msg_send![scene, isKindOfClass: class!(UIWindowScene)];
                        is_window_scene
                    })?;
                let window: Allocated<AnyObject> = msg_send![class!(UIWindow), alloc];
                let window: Retained<AnyObject> = msg_send![window, initWithWindowScene: scene];
                let controller: Retained<AnyObject> = msg_send![class!(UIViewController), new];
                let _: () = msg_send![&*window, setRootViewController: &*controller];
                let _: () = msg_send![&*window, makeKeyAndVisible];
                Some(SceneWindow(window))
            }
        }
    }

    impl Drop for SceneWindow {
        fn drop(&mut self) {
            // SAFETY: Made on the main thread and dropped in the same event.
            let _: () = unsafe { msg_send![&*self.0, setHidden: true] };
        }
    }
}

#[cfg(target_os = "android")]
pub use android::{__android_entry, MainResult, android_app};
#[cfg(target_os = "android")]
pub(crate) use android::window_insets;
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

    /// The running app, for reaching Android's APIs the framework doesn't wrap.
    /// `activity_as_ptr()` is the real `Activity`, and `vm_as_ptr()` the Java VM.
    /// There's only ever one activity, since a second one restarts the process.
    ///
    /// # Panics
    ///
    /// Without `love2drs::android_main!`, which sets it before `main` runs.
    pub fn android_app() -> &'static AndroidApp {
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
