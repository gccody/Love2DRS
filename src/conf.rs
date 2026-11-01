/// Startup configuration, the equivalent of LÖVE's `conf.lua`.
///
/// ```no_run
/// # use love2drs::Conf;
/// let conf = Conf::new("My Game").size(1280, 720).resizable(true).msaa(4);
/// ```
#[derive(Debug, Clone)]
pub struct Conf {
    /// Window title.
    pub title: String,
    /// Window width in logical pixels.
    pub width: u32,
    /// Window height in logical pixels.
    pub height: u32,
    /// Whether the user can resize the window.
    pub resizable: bool,
    /// Start in borderless fullscreen.
    pub fullscreen: bool,
    /// Synchronise presentation with the display refresh rate.
    pub vsync: bool,
    /// Multisample anti-aliasing for the screen (1 = off, 4 = typical).
    pub msaa: u32,
    /// Smallest size the window may be resized to.
    pub min_size: Option<(u32, u32)>,
    /// Upper bound for the `dt` passed to `update`, in seconds. Prevents huge
    /// simulation steps after the window was dragged or the game was paused by the OS.
    pub max_dt: f32,
}

impl Default for Conf {
    fn default() -> Self {
        Conf {
            title: "love2drs".into(),
            width: 800,
            height: 600,
            resizable: false,
            fullscreen: false,
            vsync: true,
            msaa: 1,
            min_size: None,
            max_dt: 0.25,
        }
    }
}

impl Conf {
    /// A default 800x600 configuration with the given title.
    pub fn new(title: impl Into<String>) -> Self {
        Conf {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    pub fn vsync(mut self, vsync: bool) -> Self {
        self.vsync = vsync;
        self
    }

    /// Sample count for screen anti-aliasing. Values other than 1 and 4 are
    /// rounded to the closest of the two, as those are universally supported.
    pub fn msaa(mut self, samples: u32) -> Self {
        self.msaa = if samples >= 2 { 4 } else { 1 };
        self
    }

    pub fn min_size(mut self, width: u32, height: u32) -> Self {
        self.min_size = Some((width, height));
        self
    }

    pub fn max_dt(mut self, max_dt: f32) -> Self {
        self.max_dt = max_dt;
        self
    }
}
