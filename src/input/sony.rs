//! A HID driver for PlayStation controllers (DualShock 4, DualSense, DualSense
//! Edge): input and rumble, over USB and Bluetooth.
//!
//! gilrs can read these pads everywhere, but can't rumble them on Windows
//! (Windows.Gaming.Input only drives Xbox-style pads) or macOS. So on those
//! platforms love2drs opens them with hidapi and speaks their protocol itself,
//! the way SDL does, and hides gilrs's copy of the pad. Reading input here too
//! is what makes Bluetooth rumble possible: rumble over Bluetooth switches the
//! controller to "enhanced" reports, which the OS gamepad APIs can't parse. It
//! also means each pad's rumble goes to exactly that pad.
//!
//! On Linux the kernel's `hid-playstation` driver already gives gilrs rumble,
//! so this module is a stub there.
//!
//! Report layouts follow the Linux `hid-playstation` driver.

// The protocol helpers are only called by the Windows/macOS backend (and tests).
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

use std::time::{Duration, Instant};

use super::gamepad::{AXES, BUTTONS, GamepadAxis, GamepadButton};

#[cfg(any(windows, target_os = "macos"))]
pub(crate) use imp::{Hid, Pad};
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) use stub::{Hid, Pad};

const SONY_VID: u16 = 0x054c;
const DS4_PIDS: [u16; 3] = [
    0x05c4, // DualShock 4 (first revision)
    0x09cc, // DualShock 4 (second revision)
    0x0ba0, // DualShock 4 USB wireless adapter
];
const DUALSENSE_PID: u16 = 0x0ce6;
const DUALSENSE_EDGE_PID: u16 = 0x0df2;

/// Same radial dead zone gilrs applies to sticks, so pads feel alike.
const STICK_DEAD_ZONE: f32 = 0.1;

/// Whether `vid`/`pid` is a controller this module drives.
pub(crate) fn is_supported(vid: u16, pid: u16) -> bool {
    vid == SONY_VID
        && (DS4_PIDS.contains(&pid) || pid == DUALSENSE_PID || pid == DUALSENSE_EDGE_PID)
}

/// Which report formats a controller speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    DualShock4,
    /// `v2` selects the newer rumble emulation, which feels like classic
    /// rumble. Older DualSense firmware only has v1.
    DualSense {
        v2: bool,
    },
}

/// A message from a pad's worker thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Msg {
    /// The controller's state changed.
    Input(Input),
    /// The controller was unplugged or stopped responding.
    Gone,
}

/// A snapshot of a controller's buttons and axes, already dead-zoned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Input {
    pub(crate) down: [bool; BUTTONS],
    pub(crate) axes: [f32; AXES],
}

// ---------------------------------------------------------------------------
// Input reports

/// Decodes an input report (`data[0]` is the report id). `bluetooth` tells the
/// USB report 0x01 apart from the short Bluetooth one with the same id.
/// Returns `None` for reports that carry no gamepad state or fail their CRC.
fn parse_input(kind: Kind, bluetooth: bool, data: &[u8]) -> Option<Input> {
    let (&id, _) = data.split_first()?;
    match (kind, id) {
        // Full Bluetooth reports end in a CRC; drop corrupted ones.
        (Kind::DualSense { .. }, 0x31) => {
            check_input_crc(data)?;
            ds5_common(data.get(2..)?)
        }
        (Kind::DualShock4, 0x11) => {
            check_input_crc(data)?;
            ds4_layout(data.get(3..)?)
        }
        // A DualSense on Bluetooth starts out sending short DS4-style reports
        // until it's switched to full ones (by the first rumble).
        (Kind::DualSense { .. }, 0x01) if bluetooth => ds4_layout(data.get(1..)?),
        (Kind::DualSense { .. }, 0x01) => ds5_common(data.get(1..)?),
        (Kind::DualShock4, 0x01) => ds4_layout(data.get(1..)?),
        _ => None,
    }
}

/// DualShock 4 reports, and the DualSense's short Bluetooth one: sticks,
/// three button bytes, then the triggers.
fn ds4_layout(d: &[u8]) -> Option<Input> {
    let d = d.get(..9)?;
    Some(decode(
        [d[0], d[1], d[2], d[3]],
        d[7],
        d[8],
        [d[4], d[5], d[6]],
    ))
}

/// The DualSense's full report: sticks, triggers, a counter, then buttons.
fn ds5_common(d: &[u8]) -> Option<Input> {
    let d = d.get(..10)?;
    Some(decode(
        [d[0], d[1], d[2], d[3]],
        d[4],
        d[5],
        [d[7], d[8], d[9]],
    ))
}

fn decode(sticks: [u8; 4], l2: u8, r2: u8, b: [u8; 3]) -> Input {
    use GamepadButton as B;

    let mut down = [false; BUTTONS];
    let mut set = |button: B, on: bool| down[button as usize] = on;

    // The low nibble of the first button byte is a hat: 0 = up, clockwise in
    // eighths; 8 or more = released.
    let hat = b[0] & 0x0f;
    set(B::DPadUp, matches!(hat, 7 | 0 | 1));
    set(B::DPadRight, matches!(hat, 1..=3));
    set(B::DPadDown, matches!(hat, 3..=5));
    set(B::DPadLeft, matches!(hat, 5..=7));
    set(B::X, b[0] & 0x10 != 0); // Square
    set(B::A, b[0] & 0x20 != 0); // Cross
    set(B::B, b[0] & 0x40 != 0); // Circle
    set(B::Y, b[0] & 0x80 != 0); // Triangle
    set(B::LeftShoulder, b[1] & 0x01 != 0);
    set(B::RightShoulder, b[1] & 0x02 != 0);
    set(B::LeftTrigger, b[1] & 0x04 != 0);
    set(B::RightTrigger, b[1] & 0x08 != 0);
    set(B::Back, b[1] & 0x10 != 0); // Share / Create
    set(B::Start, b[1] & 0x20 != 0); // Options
    set(B::LeftStick, b[1] & 0x40 != 0);
    set(B::RightStick, b[1] & 0x80 != 0);
    set(B::Guide, b[2] & 0x01 != 0); // PS

    let stick = |v: u8| ((v as f32 - 128.0) / 127.0).clamp(-1.0, 1.0);
    let (lx, ly) = dead_zone(stick(sticks[0]), stick(sticks[1]));
    let (rx, ry) = dead_zone(stick(sticks[2]), stick(sticks[3]));
    let mut axes = [0.0; AXES];
    // HID sticks already have y pointing down, like the screen.
    axes[GamepadAxis::LeftX as usize] = lx;
    axes[GamepadAxis::LeftY as usize] = ly;
    axes[GamepadAxis::RightX as usize] = rx;
    axes[GamepadAxis::RightY as usize] = ry;
    axes[GamepadAxis::LeftTrigger as usize] = l2 as f32 / 255.0;
    axes[GamepadAxis::RightTrigger as usize] = r2 as f32 / 255.0;
    Input { down, axes }
}

/// Radial dead zone, rescaled so values still reach 1 (as gilrs does).
pub(crate) fn dead_zone(x: f32, y: f32) -> (f32, f32) {
    let magnitude = (x * x + y * y).sqrt().min(1.0);
    if magnitude <= STICK_DEAD_ZONE {
        (0.0, 0.0)
    } else {
        let scale = (magnitude - STICK_DEAD_ZONE) / (1.0 - STICK_DEAD_ZONE) / magnitude;
        (x * scale, y * scale)
    }
}

// ---------------------------------------------------------------------------
// Output reports

/// Bluetooth reports are 78 bytes ending in a CRC-32 of a seed byte plus the
/// rest of the report: 0xA1 for input, 0xA2 for output.
const BT_REPORT_LEN: usize = 78;

fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = !0u32;
    for &byte in parts.iter().flat_map(|p| p.iter()) {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn check_input_crc(data: &[u8]) -> Option<()> {
    let data = data.get(..BT_REPORT_LEN)?;
    let (body, tail) = data.split_at(BT_REPORT_LEN - 4);
    let want = u32::from_le_bytes(tail.try_into().ok()?);
    (crc32(&[&[0xa1], body]) == want).then_some(())
}

fn seal_bt_report(r: &mut [u8; BT_REPORT_LEN]) {
    let crc = crc32(&[&[0xa2], &r[..BT_REPORT_LEN - 4]]);
    r[BT_REPORT_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
}

/// The DualSense's 47-byte output block shared by USB and Bluetooth reports:
/// valid_flag0, valid_flag1, motor_right (weak), motor_left (strong), ...,
/// valid_flag2 at offset 38. Only the rumble fields are flagged valid, so
/// lights and everything else are left alone.
fn ds5_output_common(v2: bool, strong: u8, weak: u8) -> [u8; 47] {
    let mut c = [0u8; 47];
    // HAPTICS_SELECT: drive the actuators from the motor values instead of
    // the audio haptics stream.
    c[0] = 0x02;
    if v2 {
        c[38] = 0x04; // valid_flag2: COMPATIBLE_VIBRATION2
    } else {
        c[0] |= 0x01; // valid_flag0: COMPATIBLE_VIBRATION
    }
    c[2] = weak;
    c[3] = strong;
    c
}

/// Builds the output report that sets the motors to `strong`/`weak`. `seq` is
/// the DualSense's 4-bit Bluetooth report counter.
fn rumble_report(kind: Kind, bluetooth: bool, seq: u8, strong: u8, weak: u8) -> Vec<u8> {
    match (kind, bluetooth) {
        (Kind::DualShock4, false) => {
            // Report 0x05: id, valid_flag0, valid_flag1, reserved, weak, strong.
            let mut r = vec![0u8; 32];
            r[0] = 0x05;
            r[1] = 0x01; // valid_flag0: motors
            r[4] = weak;
            r[5] = strong;
            r
        }
        (Kind::DualShock4, true) => {
            // Report 0x11: id, hw_control, audio_control, then the USB layout.
            let mut r = [0u8; BT_REPORT_LEN];
            r[0] = 0x11;
            r[1] = 0xc0; // hw_control: HID data + CRC present
            r[3] = 0x01; // valid_flag0: motors
            r[6] = weak;
            r[7] = strong;
            seal_bt_report(&mut r);
            r.to_vec()
        }
        (Kind::DualSense { v2 }, false) => {
            let mut r = vec![0u8; 48];
            r[0] = 0x02;
            r[1..].copy_from_slice(&ds5_output_common(v2, strong, weak));
            r
        }
        (Kind::DualSense { v2 }, true) => {
            // Report 0x31: id, sequence tag, 0x10 tag, the common block, CRC.
            let mut r = [0u8; BT_REPORT_LEN];
            r[0] = 0x31;
            r[1] = (seq & 0x0f) << 4;
            r[2] = 0x10;
            r[3..50].copy_from_slice(&ds5_output_common(v2, strong, weak));
            seal_bt_report(&mut r);
            r.to_vec()
        }
    }
}

/// Converts a 0..1 motor strength to the byte the controller wants.
fn motor_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// What the motors should be doing: current strengths and when to stop.
#[derive(Debug, Default)]
struct Rumble {
    motors: (u8, u8),
    until: Option<Instant>,
}

impl Rumble {
    /// Applies a `set_vibration` call; returns the motor bytes to send.
    fn set(&mut self, left: f32, right: f32, duration: f32, now: Instant) -> (u8, u8) {
        let motors = (motor_byte(left), motor_byte(right));
        if motors == (0, 0) || duration <= 0.0 || duration.is_nan() {
            *self = Rumble::default();
        } else {
            self.motors = motors;
            // Infinite (or absurdly long) durations never time out.
            self.until = Duration::try_from_secs_f32(duration)
                .ok()
                .and_then(|d| now.checked_add(d));
        }
        self.motors
    }

    /// Whether a timed rumble ran out at `now`. Clears it if so.
    fn expired(&mut self, now: Instant) -> bool {
        if self.until.is_some_and(|t| now >= t) {
            *self = Rumble::default();
            true
        } else {
            false
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use std::ffi::{CStr, CString};
    use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
    use std::thread::JoinHandle;
    use std::time::Instant;

    use hidapi::{BusType, HidApi, HidDevice};

    use super::{
        DUALSENSE_EDGE_PID, DUALSENSE_PID, Input, Kind, Msg, Rumble, SONY_VID, is_supported,
        parse_input, rumble_report,
    };

    /// How long the worker waits for an input report before checking for
    /// rumble commands; bounds rumble latency and timing error.
    const POLL_MS: i32 = 4;

    /// A scan's request: the paths already open, and where to send what it finds.
    type Scan = (Vec<CString>, Sender<Vec<Pad>>);

    /// The hidapi context, on a thread of its own. Listing devices spins the
    /// calling thread's run loop on macOS, which runs whatever else is queued
    /// there. On the main thread that re-enters winit's event handler, which panics.
    pub(crate) struct Hid {
        scans: Option<Sender<Scan>>,
    }

    impl Hid {
        pub(crate) fn new() -> Self {
            let (scans, requests) = mpsc::channel::<Scan>();
            let (ready, started) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("hidapi".to_owned())
                .spawn(move || {
                    let mut api = match HidApi::new() {
                        Ok(api) => api,
                        Err(e) => {
                            log::warn!(
                                "hidapi unavailable, PlayStation pads fall back to gilrs: {e}"
                            );
                            let _ = ready.send(false);
                            return;
                        }
                    };
                    let _ = ready.send(true);
                    for (open, reply) in requests {
                        let _ = reply.send(open_new_pads(&mut api, &open));
                    }
                });
            let available = thread.is_ok() && started.recv().unwrap_or(false);
            Hid {
                scans: available.then_some(scans),
            }
        }

        /// A backend that finds nothing, for tests.
        #[cfg(test)]
        pub(crate) fn disabled() -> Self {
            Hid { scans: None }
        }

        /// Whether PlayStation pads are read here rather than through gilrs.
        pub(crate) fn available(&self) -> bool {
            self.scans.is_some()
        }

        /// Opens every supported controller that isn't in `open` yet.
        pub(crate) fn scan(&mut self, open: &[&CStr]) -> Vec<Pad> {
            let Some(scans) = &self.scans else {
                return Vec::new();
            };
            let open = open.iter().map(|path| (*path).to_owned()).collect();
            let (reply, found) = mpsc::channel();
            if scans.send((open, reply)).is_err() {
                return Vec::new();
            }
            found.recv().unwrap_or_default()
        }
    }

    /// What a scan does on the hidapi thread.
    fn open_new_pads(api: &mut HidApi, open: &[CString]) -> Vec<Pad> {
        if let Err(e) = api
            .reset_devices()
            .and_then(|()| api.add_devices(SONY_VID, 0))
        {
            log::warn!("couldn't list HID devices: {e}");
            return Vec::new();
        }
        let mut found: Vec<Pad> = Vec::new();
        for info in api.device_list() {
            let (vid, pid) = (info.vendor_id(), info.product_id());
            // Skip non-gamepad collections (usage page 1 is Generic
            // Desktop; 0 means the backend didn't say).
            if !is_supported(vid, pid) || !matches!(info.usage_page(), 0 | 1) {
                continue;
            }
            let path = info.path();
            if open.iter().any(|o| o.as_c_str() == path)
                || found.iter().any(|p| p.path.as_c_str() == path)
            {
                continue;
            }
            let dev = match api.open_path(path) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("couldn't open PlayStation controller: {e}");
                    continue;
                }
            };
            let bluetooth = matches!(info.bus_type(), BusType::Bluetooth);
            let (kind, name) = match pid {
                DUALSENSE_PID => (
                    Kind::DualSense {
                        v2: dualsense_has_vibration_v2(&dev),
                    },
                    "DualSense",
                ),
                DUALSENSE_EDGE_PID => (Kind::DualSense { v2: true }, "DualSense Edge"),
                _ => (Kind::DualShock4, "DualShock 4"),
            };
            log::debug!(
                "{name} via HID ({})",
                if bluetooth { "Bluetooth" } else { "USB" }
            );
            found.push(Pad::spawn(
                dev,
                path.to_owned(),
                name,
                (vid, pid),
                kind,
                bluetooth,
            ));
        }
        found
    }

    /// Reads the DualSense firmware version: v2 rumble arrived in update 2.21.
    fn dualsense_has_vibration_v2(dev: &HidDevice) -> bool {
        let mut buf = [0u8; 64];
        buf[0] = 0x20; // firmware info feature report
        match dev.get_feature_report(&mut buf) {
            Ok(n) if n >= 46 => u16::from_le_bytes([buf[44], buf[45]]) >= 0x0215,
            _ => false,
        }
    }

    enum Cmd {
        Rumble {
            left: f32,
            right: f32,
            duration: f32,
        },
    }

    /// An opened PlayStation controller, served by its own worker thread.
    /// Dropping it stops the motors and ends the thread.
    pub(crate) struct Pad {
        path: CString,
        name: &'static str,
        ids: (u16, u16),
        cmds: Option<Sender<Cmd>>,
        msgs: Receiver<Msg>,
        thread: Option<JoinHandle<()>>,
    }

    impl Pad {
        fn spawn(
            dev: HidDevice,
            path: CString,
            name: &'static str,
            ids: (u16, u16),
            kind: Kind,
            bluetooth: bool,
        ) -> Pad {
            let (cmd_tx, cmd_rx) = mpsc::channel();
            let (msg_tx, msg_rx) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name(format!("love2drs {name}"))
                .spawn(move || worker(dev, kind, bluetooth, msg_tx, cmd_rx))
                .map_err(|e| log::warn!("couldn't start {name} thread: {e}"))
                .ok();
            Pad {
                path,
                name,
                ids,
                cmds: Some(cmd_tx),
                msgs: msg_rx,
                thread,
            }
        }

        pub(crate) fn name(&self) -> &'static str {
            self.name
        }

        pub(crate) fn path(&self) -> &CStr {
            &self.path
        }

        /// The USB vendor and product id.
        pub(crate) fn ids(&self) -> (u16, u16) {
            self.ids
        }

        /// Starts (or with zeros, stops) rumble. Returns `false` if the
        /// controller is gone.
        pub(crate) fn set(&mut self, left: f32, right: f32, duration: f32) -> bool {
            self.cmds.as_ref().is_some_and(|tx| {
                tx.send(Cmd::Rumble {
                    left,
                    right,
                    duration,
                })
                .is_ok()
            })
        }

        /// Messages from the worker since the last call.
        pub(crate) fn messages(&self) -> impl Iterator<Item = Msg> + '_ {
            self.msgs.try_iter()
        }
    }

    impl Drop for Pad {
        fn drop(&mut self) {
            // Closing the command channel tells the worker to stop the motors
            // and exit; wait so that happens before the program does.
            self.cmds = None;
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn worker(
        dev: HidDevice,
        kind: Kind,
        mut bluetooth: bool,
        msgs: Sender<Msg>,
        cmds: Receiver<Cmd>,
    ) {
        let mut buf = [0u8; 128];
        let mut last: Option<Input> = None;
        let mut rumble = Rumble::default();
        let mut seq = 0u8;
        let mut write = |bluetooth: bool, (strong, weak): (u8, u8)| {
            let report = rumble_report(kind, bluetooth, seq, strong, weak);
            seq = (seq + 1) & 0x0f;
            if let Err(e) = dev.write(&report) {
                log::warn!("PlayStation rumble failed: {e}");
            }
        };
        loop {
            match dev.read_timeout(&mut buf, POLL_MS) {
                Ok(0) => {}
                Ok(n) => {
                    let data = &buf[..n];
                    // Full-length reports only exist on Bluetooth: trust them
                    // over a bus type the OS might not have reported.
                    if matches!(
                        (kind, data[0]),
                        (Kind::DualSense { .. }, 0x31) | (Kind::DualShock4, 0x11)
                    ) {
                        bluetooth = true;
                    }
                    if let Some(input) = parse_input(kind, bluetooth, data)
                        && last != Some(input)
                    {
                        last = Some(input);
                        if msgs.send(Msg::Input(input)).is_err() {
                            break; // the pad was dropped
                        }
                    }
                }
                Err(e) => {
                    log::debug!("PlayStation controller gone: {e}");
                    let _ = msgs.send(Msg::Gone);
                    return;
                }
            }
            loop {
                match cmds.try_recv() {
                    Ok(Cmd::Rumble {
                        left,
                        right,
                        duration,
                    }) => write(bluetooth, rumble.set(left, right, duration, Instant::now())),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if rumble.motors != (0, 0) {
                            write(bluetooth, (0, 0));
                        }
                        return;
                    }
                }
            }
            if rumble.expired(Instant::now()) {
                write(bluetooth, (0, 0));
            }
        }
        if rumble.motors != (0, 0) {
            write(bluetooth, (0, 0));
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod stub {
    use std::ffi::CStr;

    use super::Msg;

    pub(crate) struct Hid;

    impl Hid {
        pub(crate) fn new() -> Self {
            Hid
        }
        #[cfg(test)]
        pub(crate) fn disabled() -> Self {
            Hid
        }
        pub(crate) fn available(&self) -> bool {
            false
        }
        pub(crate) fn scan(&mut self, _open: &[&CStr]) -> Vec<Pad> {
            Vec::new()
        }
    }

    /// Never constructed on this platform.
    pub(crate) enum Pad {}

    impl Pad {
        pub(crate) fn name(&self) -> &'static str {
            match *self {}
        }
        pub(crate) fn path(&self) -> &CStr {
            match *self {}
        }
        pub(crate) fn ids(&self) -> (u16, u16) {
            match *self {}
        }
        pub(crate) fn set(&mut self, _: f32, _: f32, _: f32) -> bool {
            match *self {}
        }
        pub(crate) fn messages(&self) -> std::iter::Empty<Msg> {
            match *self {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use GamepadButton as B;

    /// Hid's thread answers scans whether or not hidapi could start, and stops with it.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn scans_are_answered_off_the_calling_thread() {
        let mut hid = Hid::new();
        let _ = hid.scan(&[]);
        let _ = hid.scan(&[]);
        assert!(!Hid::disabled().available());
    }

    fn pressed(input: &Input) -> Vec<GamepadButton> {
        GamepadButton::ALL
            .into_iter()
            .filter(|&b| input.down[b as usize])
            .collect()
    }

    fn bt_input(id: u8, fill: impl FnOnce(&mut [u8])) -> [u8; BT_REPORT_LEN] {
        let mut r = [0u8; BT_REPORT_LEN];
        r[0] = id;
        fill(&mut r);
        let crc = crc32(&[&[0xa1], &r[..BT_REPORT_LEN - 4]]);
        r[BT_REPORT_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        r
    }

    const DS5: Kind = Kind::DualSense { v2: true };

    #[test]
    fn recognizes_playstation_pads() {
        assert!(is_supported(SONY_VID, DUALSENSE_EDGE_PID));
        assert!(is_supported(SONY_VID, 0x09cc));
        assert!(!is_supported(SONY_VID, 0x0001));
        assert!(!is_supported(0x045e, DUALSENSE_PID)); // Microsoft vid
    }

    #[test]
    fn crc32_matches_the_standard() {
        assert_eq!(crc32(&[b"1234", b"56789"]), 0xcbf4_3926);
    }

    #[test]
    fn dualsense_usb_input() {
        let mut r = [0u8; 64];
        r[0] = 0x01;
        r[1..5].copy_from_slice(&[0, 128, 255, 128]); // LX left, RX right
        r[5] = 255; // L2
        r[8] = 0x20 | 0x02; // Cross, hat right
        r[9] = 0x01 | 0x20; // L1, Options
        r[10] = 0x01; // PS
        let input = parse_input(DS5, false, &r).unwrap();
        assert_eq!(
            pressed(&input),
            [B::A, B::Guide, B::Start, B::LeftShoulder, B::DPadRight]
        );
        assert_eq!(input.axes[GamepadAxis::LeftX as usize], -1.0);
        assert_eq!(input.axes[GamepadAxis::LeftY as usize], 0.0);
        assert_eq!(input.axes[GamepadAxis::RightX as usize], 1.0);
        assert_eq!(input.axes[GamepadAxis::LeftTrigger as usize], 1.0);
    }

    #[test]
    fn dualsense_bluetooth_inputs() {
        // Short report, DS4 layout: buttons before triggers.
        let mut short = [0u8; 10];
        short[0] = 0x01;
        short[1..5].copy_from_slice(&[128; 4]);
        short[5] = 0x80 | 0x08; // Triangle, hat released
        short[9] = 255; // R2
        let input = parse_input(DS5, true, &short).unwrap();
        assert_eq!(pressed(&input), [B::Y]);
        assert_eq!(input.axes[GamepadAxis::RightTrigger as usize], 1.0);

        // Full report: same as USB, shifted by one, with a CRC.
        let full = bt_input(0x31, |r| {
            r[2..6].copy_from_slice(&[128, 0, 128, 128]); // LY up
            r[9] = 0x10 | 0x06; // Square, hat left
        });
        let input = parse_input(DS5, true, &full).unwrap();
        assert_eq!(pressed(&input), [B::X, B::DPadLeft]);
        assert_eq!(input.axes[GamepadAxis::LeftY as usize], -1.0);

        let mut corrupt = full;
        corrupt[9] ^= 0x40;
        assert_eq!(parse_input(DS5, true, &corrupt), None);
    }

    #[test]
    fn dualshock4_inputs() {
        let mut usb = [0u8; 64];
        usb[0] = 0x01;
        usb[1..5].copy_from_slice(&[128; 4]);
        usb[5] = 0x40 | 0x07; // Circle, hat up-left
        usb[6] = 0x80; // R3
        let input = parse_input(Kind::DualShock4, false, &usb).unwrap();
        assert_eq!(
            pressed(&input),
            [B::B, B::RightStick, B::DPadUp, B::DPadLeft]
        );

        let bt = bt_input(0x11, |r| {
            r[3..7].copy_from_slice(&[128; 4]);
            r[7] = 0x08; // hat released
            r[8] = 0x10; // Share
        });
        let input = parse_input(Kind::DualShock4, true, &bt).unwrap();
        assert_eq!(pressed(&input), [B::Back]);
    }

    #[test]
    fn small_stick_motion_is_dead_zoned() {
        assert_eq!(dead_zone(0.05, -0.05), (0.0, 0.0));
        let (x, y) = dead_zone(1.0, 0.0);
        assert!((x - 1.0).abs() < 1e-6 && y == 0.0);
    }

    #[test]
    fn dualsense_rumble_reports() {
        let v2 = rumble_report(DS5, false, 0, 200, 100);
        assert_eq!(v2.len(), 48);
        assert_eq!(&v2[..5], &[0x02, 0x02, 0x00, 100, 200]);
        assert_eq!(v2[39], 0x04);

        let v1 = rumble_report(Kind::DualSense { v2: false }, false, 0, 200, 100);
        assert_eq!(&v1[..5], &[0x02, 0x03, 0x00, 100, 200]);
        assert_eq!(v1[39], 0x00);

        let bt = rumble_report(DS5, true, 5, 200, 100);
        assert_eq!(bt.len(), BT_REPORT_LEN);
        assert_eq!(&bt[..7], &[0x31, 0x50, 0x10, 0x02, 0x00, 100, 200]);
        assert_eq!(bt[3 + 38], 0x04);
        let crc = crc32(&[&[0xa2], &bt[..74]]);
        assert_eq!(bt[74..], crc.to_le_bytes());
    }

    #[test]
    fn dualshock4_rumble_reports() {
        let usb = rumble_report(Kind::DualShock4, false, 0, 200, 100);
        assert_eq!(usb.len(), 32);
        assert_eq!(&usb[..6], &[0x05, 0x01, 0x00, 0x00, 100, 200]);

        let bt = rumble_report(Kind::DualShock4, true, 0, 200, 100);
        assert_eq!(&bt[..8], &[0x11, 0xc0, 0x00, 0x01, 0x00, 0x00, 100, 200]);
        let crc = crc32(&[&[0xa2], &bt[..74]]);
        assert_eq!(bt[74..], crc.to_le_bytes());
    }

    #[test]
    fn timed_rumble_expires() {
        let t0 = Instant::now();
        let mut r = Rumble::default();
        assert_eq!(r.set(1.0, 0.5, 0.25, t0), (255, 128));
        assert!(!r.expired(t0 + Duration::from_millis(100)));
        assert!(r.expired(t0 + Duration::from_millis(250)));
        assert_eq!(r.motors, (0, 0));
        assert!(!r.expired(t0 + Duration::from_secs(9)));
    }

    #[test]
    fn infinite_and_zero_rumble() {
        let t0 = Instant::now();
        let mut r = Rumble::default();
        r.set(0.6, 0.6, f32::INFINITY, t0);
        assert!(!r.expired(t0 + Duration::from_secs(3600)));
        assert_eq!(r.set(0.0, 0.0, 1.0, t0), (0, 0));
        assert_eq!(r.set(1.0, 1.0, 0.0, t0), (0, 0));
        assert!(r.until.is_none());
    }
}
