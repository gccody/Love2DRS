//! Which game controllers are plugged in on Windows, found without waking
//! anything up.
//!
//! gilrs reads controllers through Windows.Gaming.Input by asking for every
//! controller's state 125 times a second on a thread of its own, and Windows
//! runs another thread to have the answers ready. A game nobody holds a
//! controller for shouldn't pay for that, so love2drs only starts gilrs once
//! Windows lists a controller that love2drs doesn't read itself (PlayStation
//! pads, see `sony.rs`), and stops it when the last one is unplugged.
//!
//! The list is the one raw input keeps. Asking for it is a single cheap call,
//! and every HID game controller is in it, Xbox pads and Steam's virtual ones
//! included (it's the list SDL finds them in).

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows_sys::Win32::UI::Input::{
    GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RID_DEVICE_INFO,
    RIDI_DEVICEINFO, RIM_TYPEHID,
};

use super::gamepad::HID_GRACE;
use super::sony;

/// How often to look at the list.
const CENSUS_TIME: Duration = Duration::from_millis(500);
/// How long gilrs keeps running after the last controller it was for has gone,
/// in case that one is only reconnecting.
const LINGER: Duration = Duration::from_secs(5);

/// HID usages (page, then the ids on it) that mean a game controller.
const GENERIC_DESKTOP: u16 = 1;
const JOYSTICK: u16 = 4;
const GAMEPAD: u16 = 5;
const MULTI_AXIS_CONTROLLER: u16 = 8;

/// How many game controllers are plugged in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Census {
    /// PlayStation pads that `sony.rs` reads.
    pub(crate) playstation: usize,
    /// Every other game controller, which only gilrs can read.
    pub(crate) others: usize,
}

impl Census {
    /// Counts one HID device, if it's a controller.
    fn count(&mut self, (vid, pid): (u16, u16), usage_page: u16, usage: u16) {
        if usage_page != GENERIC_DESKTOP {
            return;
        }
        if sony::is_supported(vid, pid) {
            // Counted whatever its usage, as `sony.rs` opens it whatever its usage.
            self.playstation += 1;
        } else if matches!(usage, JOYSTICK | GAMEPAD | MULTI_AXIS_CONTROLLER) {
            self.others += 1;
        }
    }
}

/// Whether a PlayStation pad that's plugged in is still to be opened over HID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unread {
    /// Every one of them is open.
    No,
    /// One isn't, as of then: worth scanning for.
    Since(Instant),
    /// One still isn't after [`HID_GRACE`] (another program may hold it), so
    /// gilrs gets to show it. No more scans until the device list changes.
    GivenUp,
}

/// Keeps count of the controllers, and decides when gilrs is needed.
pub(crate) struct Watch {
    /// The list as last seen (each device's handle and type), and its census.
    /// `None` until the first look.
    listed: Option<Vec<(usize, u32)>>,
    census: Census,
    /// Where Windows writes the list.
    buffer: Vec<RAWINPUTDEVICELIST>,
    /// When to look next. `None` means now.
    next: Option<Instant>,
    unread: Unread,
    /// Since when gilrs has been running with nothing to read.
    unwanted_since: Option<Instant>,
    /// gilrs wouldn't start, so there's no use trying again.
    pub(crate) gilrs_failed: bool,
}

impl Watch {
    pub(crate) fn new() -> Self {
        Watch {
            listed: None,
            census: Census::default(),
            buffer: vec![RAWINPUTDEVICELIST::default(); 64],
            next: None,
            unread: Unread::No,
            unwanted_since: None,
            gilrs_failed: false,
        }
    }

    /// Whether it's time for another look. Says yes once per [`CENSUS_TIME`].
    pub(crate) fn due(&mut self, now: Instant) -> bool {
        if self.next.is_some_and(|at| now < at) {
            return false;
        }
        self.next = Some(now + CENSUS_TIME);
        true
    }

    /// Counts the controllers Windows lists. `None` if it wouldn't say.
    pub(crate) fn census(&mut self) -> Option<Census> {
        let listed = self.list()?;
        if self.listed.as_ref() == Some(&listed) {
            return Some(self.census);
        }
        let mut census = Census::default();
        for &(device, kind) in &listed {
            if kind == RIM_TYPEHID
                && let Some((ids, usage_page, usage)) = describe(device)
            {
                census.count(ids, usage_page, usage);
            }
        }
        log::debug!("game controllers plugged in: {census:?}");
        self.listed = Some(listed);
        self.census = census;
        // Something was plugged in or pulled out, so a pad that wouldn't open
        // before is worth another try.
        self.unread = Unread::No;
        Some(census)
    }

    /// Every raw input device's handle and type. Usually one call.
    fn list(&mut self) -> Option<Vec<(usize, u32)>> {
        let size = size_of::<RAWINPUTDEVICELIST>() as u32;
        for _ in 0..4 {
            let mut count = self.buffer.len() as u32;
            // SAFETY: `buffer` has room for `count` entries of `size` bytes.
            let got = unsafe { GetRawInputDeviceList(self.buffer.as_mut_ptr(), &mut count, size) };
            if got != u32::MAX {
                let devices = &self.buffer[..(got as usize).min(self.buffer.len())];
                return Some(
                    devices
                        .iter()
                        .map(|d| (d.hDevice as usize, d.dwType))
                        .collect(),
                );
            }
            // It fails when there are more devices than room, and says how many.
            if count as usize <= self.buffer.len() {
                return None;
            }
            self.buffer
                .resize(count as usize + 16, RAWINPUTDEVICELIST::default());
        }
        None
    }

    /// Follows the PlayStation pads: `plugged` are listed and `open` are being
    /// read. Call again after a scan, which may have opened the missing one.
    pub(crate) fn playstation(&mut self, plugged: usize, open: usize, now: Instant) -> Unread {
        self.unread = if plugged <= open {
            Unread::No
        } else {
            match self.unread {
                Unread::No => Unread::Since(now),
                Unread::Since(since) if now.duration_since(since) >= HID_GRACE => Unread::GivenUp,
                waiting => waiting,
            }
        };
        self.unread
    }

    /// Whether gilrs should be running, given whether there's anything for it
    /// to read right now.
    pub(crate) fn keep_gilrs(&mut self, wanted: bool, now: Instant) -> bool {
        if wanted {
            self.unwanted_since = None;
            return true;
        }
        let since = *self.unwanted_since.get_or_insert(now);
        now.duration_since(since) < LINGER
    }
}

/// A HID device's vendor and product ids, and its usage page and usage.
fn describe(device: usize) -> Option<((u16, u16), u16, u16)> {
    let mut info = RID_DEVICE_INFO {
        cbSize: size_of::<RID_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    let mut size = info.cbSize;
    // SAFETY: `info` is `size` bytes, and `cbSize` says so, as the call requires.
    let got = unsafe {
        GetRawInputDeviceInfoW(
            device as *mut c_void,
            RIDI_DEVICEINFO,
            (&raw mut info).cast(),
            &mut size,
        )
    };
    // It fails for a device that went away since it was listed.
    if got == 0 || got == u32::MAX || info.dwType != RIM_TYPEHID {
        return None;
    }
    // SAFETY: the type says which member of the union was filled in.
    let hid = unsafe { info.Anonymous.hid };
    Some((
        (hid.dwVendorId as u16, hid.dwProductId as u16),
        hid.usUsagePage,
        hid.usUsage,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XBOX: (u16, u16) = (0x045e, 0x0b12);
    const DUALSENSE: (u16, u16) = (0x054c, 0x0ce6);

    #[test]
    fn counts_controllers_and_nothing_else() {
        let mut census = Census::default();
        census.count(XBOX, GENERIC_DESKTOP, GAMEPAD);
        census.count((0x046d, 0xc215), GENERIC_DESKTOP, JOYSTICK);
        census.count(DUALSENSE, GENERIC_DESKTOP, GAMEPAD);
        // A mouse, a keyboard, and a headset's buttons.
        census.count((0x046d, 0xc08b), GENERIC_DESKTOP, 2);
        census.count((0x046d, 0xc33f), GENERIC_DESKTOP, 6);
        census.count(DUALSENSE, 0x0c, 1);
        assert_eq!(
            census,
            Census {
                playstation: 1,
                others: 2
            }
        );
    }

    /// The list can be read on any Windows machine, and reading it again
    /// without a change is answered from memory.
    #[test]
    fn the_device_list_can_be_read() {
        let mut watch = Watch::new();
        let first = watch.census().expect("Windows lists its raw input devices");
        assert_eq!(watch.census(), Some(first));
    }

    #[test]
    fn looks_twice_a_second() {
        let mut watch = Watch::new();
        let now = Instant::now();
        assert!(watch.due(now));
        assert!(!watch.due(now + Duration::from_millis(100)));
        assert!(watch.due(now + CENSUS_TIME));
    }

    #[test]
    fn a_playstation_pad_that_wont_open_goes_to_gilrs() {
        let mut watch = Watch::new();
        let now = Instant::now();
        assert_eq!(watch.playstation(0, 0, now), Unread::No);
        // Plugged in: scan for it. The scan opens it.
        assert_eq!(watch.playstation(1, 0, now), Unread::Since(now));
        assert_eq!(watch.playstation(1, 1, now), Unread::No);
        // A second one that never opens is waited for, then given up on.
        assert_eq!(watch.playstation(2, 1, now), Unread::Since(now));
        let later = now + Duration::from_secs(1);
        assert_eq!(watch.playstation(2, 1, later), Unread::Since(now));
        assert_eq!(watch.playstation(2, 1, now + HID_GRACE), Unread::GivenUp);
        assert_eq!(
            watch.playstation(2, 1, now + HID_GRACE * 2),
            Unread::GivenUp
        );
        // Unplugged again.
        assert_eq!(watch.playstation(1, 1, now + HID_GRACE * 2), Unread::No);
    }

    #[test]
    fn gilrs_outlasts_a_reconnect_but_not_an_unplugging() {
        let mut watch = Watch::new();
        let now = Instant::now();
        assert!(watch.keep_gilrs(true, now));
        assert!(watch.keep_gilrs(false, now + Duration::from_secs(1)));
        assert!(watch.keep_gilrs(true, now + Duration::from_secs(2)));
        assert!(watch.keep_gilrs(false, now + Duration::from_secs(3)));
        assert!(!watch.keep_gilrs(false, now + Duration::from_secs(3) + LINGER));
    }
}
