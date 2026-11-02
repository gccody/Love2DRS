//! iOS's audio session: how the game's sound gets along with other apps, and word
//! of interruptions like phone calls. cpal and rodio leave this to the app.

use std::ptr::NonNull;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategory, AVAudioSessionCategoryAmbient,
    AVAudioSessionCategoryPlayback, AVAudioSessionCategorySoloAmbient,
    AVAudioSessionInterruptionNotification, AVAudioSessionInterruptionType,
    AVAudioSessionInterruptionTypeKey, AVAudioSessionMediaServicesWereResetNotification,
    AVAudioSessionSetActiveOptions,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName, NSNumber};

use super::AudioSession;

// Notifications can arrive on any thread. The audio code acts on them in `poll`.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static MEDIA_SERVICES_RESET: AtomicBool = AtomicBool::new(false);

pub(super) fn is_interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

pub(super) fn clear_interrupted() {
    INTERRUPTED.store(false, Ordering::Relaxed);
}

pub(super) fn take_media_services_reset() -> bool {
    MEDIA_SERVICES_RESET.swap(false, Ordering::Relaxed)
}

fn category(session: AudioSession) -> Option<&'static AVAudioSessionCategory> {
    // SAFETY: These are constants from the framework.
    unsafe {
        match session {
            AudioSession::Ambient => AVAudioSessionCategoryAmbient,
            AudioSession::SoloAmbient => AVAudioSessionCategorySoloAmbient,
            AudioSession::Playback => AVAudioSessionCategoryPlayback,
        }
    }
}

/// Applies the category and starts listening for interruptions.
pub(super) fn apply(session: AudioSession) {
    listen_once();
    let Some(category) = category(session) else {
        log::warn!("iOS doesn't know the audio session category for {session:?}");
        return;
    };
    // SAFETY: The shared session is safe to use from any thread.
    let result = unsafe { AVAudioSession::sharedInstance().setCategory_error(category) };
    match result {
        Ok(()) => log::info!("audio session category is {category}"),
        Err(e) => log::warn!("can't set the audio session category: {e}"),
    }
}

/// Claims the audio hardware, or gives it back so that other apps' music can
/// carry on. Not fatal if it fails: iOS may refuse during a call, and a stream
/// that can't start then gets retried.
pub(super) fn set_active(active: bool) {
    // SAFETY: The shared session is safe to use from any thread.
    let result = unsafe {
        let session = AVAudioSession::sharedInstance();
        if active {
            session.setActive_error(true)
        } else {
            session.setActive_withOptions_error(
                false,
                AVAudioSessionSetActiveOptions::NotifyOthersOnDeactivation,
            )
        }
    };
    if let Err(e) = result {
        log::warn!("can't set the audio session active to {active}: {e}");
    }
}

fn observe(name: Option<&'static NSNotificationName>, handler: impl Fn(&NSNotification) + 'static) {
    let Some(name) = name else {
        log::warn!("iOS doesn't know an audio session notification");
        return;
    };
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
        // SAFETY: NSNotificationCenter passes a valid notification.
        handler(unsafe { notification.as_ref() });
    });
    // SAFETY: The block is copied, and only touches atomics. With no queue, it
    // runs on whichever thread posted, which is why it's made to be `'static`.
    let token = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(name),
            None,
            None,
            &block,
        )
    };
    // These listen for as long as the process lives.
    std::mem::forget(token);
}

fn listen_once() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: These are constants from the framework.
        let (interruption, reset) = unsafe {
            (
                AVAudioSessionInterruptionNotification,
                AVAudioSessionMediaServicesWereResetNotification,
            )
        };
        observe(interruption, |notification| {
            if let Some(began) = interruption_began(notification) {
                log::info!("audio interrupted: {began}");
                INTERRUPTED.store(began, Ordering::Relaxed);
            }
        });
        observe(reset, |_| {
            MEDIA_SERVICES_RESET.store(true, Ordering::Relaxed)
        });
    });
}

/// Reads whether an interruption notification is for its start or its end.
fn interruption_began(notification: &NSNotification) -> Option<bool> {
    let info = notification.userInfo()?;
    // SAFETY: This is a constant from the framework.
    let key = unsafe { AVAudioSessionInterruptionTypeKey }?;
    let value = info.objectForKey(key.as_ref())?;
    let value = value.downcast::<NSNumber>().ok()?;
    Some(AVAudioSessionInterruptionType(value.as_usize()) == AVAudioSessionInterruptionType::Began)
}
