//! Horizon patch: ask Core Animation for ProMotion refresh rates.
//!
//! On ProMotion iPhones the `CADisableMinimumFrameDurationOnPhone` Info.plist key only *permits*
//! rates above 60 Hz. The display still runs at the rate that the app's display links ask for,
//! and winit drives its redraws from run-loop observers rather than from a display link, so a
//! Metal layer presenting as fast as it can stays at 60 Hz. Apple's guidance for full-motion
//! games is a `CADisplayLink` whose `preferredFrameRateRange` reaches 120 fps; osu! and Unity
//! do the same. This link does no work when it fires: it exists to state that frame rate, while
//! the game's own loop and frame limiter keep deciding when to draw and present.
//!
//! https://developer.apple.com/documentation/quartzcore/optimizing-promotion-refresh-rates-for-iphone-13-pro-and-ipad-pro

use std::cell::RefCell;

use core_foundation::runloop::kCFRunLoopCommonModes;
use objc2::encode::{Encode, Encoding};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_foundation::{MainThreadMarker, NSObject};

/// `CAFrameRateRange` (iOS 15): three `float`s.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct CAFrameRateRange {
    minimum: f32,
    maximum: f32,
    preferred: f32,
}

unsafe impl Encode for CAFrameRateRange {
    const ENCODING: Encoding =
        Encoding::Struct("CAFrameRateRange", &[f32::ENCODING, f32::ENCODING, f32::ENCODING]);
}

/// The range Apple suggests for games, with the minimum at 80 so the display does not settle
/// at 60 while the game keeps up; ProMotion picks the highest rate the game sustains.
const GAME_FRAME_RATE_RANGE: CAFrameRateRange =
    CAFrameRateRange { minimum: 80.0, maximum: 120.0, preferred: 120.0 };

declare_class!(
    struct HorizonDisplayLinkTarget;

    unsafe impl ClassType for HorizonDisplayLinkTarget {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "HorizonDisplayLinkTarget";
    }

    impl DeclaredClass for HorizonDisplayLinkTarget {}

    unsafe impl HorizonDisplayLinkTarget {
        #[method(tick:)]
        fn tick(&self, _link: &AnyObject) {}
    }
);

thread_local! {
    /// The run loop retains the link and the link its target; held here as well so the link
    /// is plainly owned for the life of the app.
    static LINK: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// Adds the display link to the main run loop, once. Devices without ProMotion cap the
/// range at their own maximum, so this is harmless on 60 Hz screens.
pub(crate) fn start(mtm: MainThreadMarker) {
    if LINK.with(|link| link.borrow().is_some()) {
        return;
    }
    let (Some(display_link_class), Some(run_loop_class)) =
        (AnyClass::get("CADisplayLink"), AnyClass::get("NSRunLoop"))
    else {
        return;
    };

    let target = mtm.alloc::<HorizonDisplayLinkTarget>().set_ivars(());
    let target: Retained<HorizonDisplayLinkTarget> = unsafe { msg_send_id![super(target), init] };

    let link: Option<Retained<AnyObject>> = unsafe {
        msg_send_id![
            display_link_class,
            displayLinkWithTarget: &*target,
            selector: sel!(tick:)
        ]
    };
    let Some(link) = link else {
        return;
    };

    let responds: bool =
        unsafe { msg_send![&*link, respondsToSelector: sel!(setPreferredFrameRateRange:)] };
    if responds {
        let _: () = unsafe { msg_send![&*link, setPreferredFrameRateRange: GAME_FRAME_RATE_RANGE] };
    } else {
        let _: () = unsafe { msg_send![&*link, setPreferredFramesPerSecond: 120isize] };
    }

    let run_loop: Retained<AnyObject> = unsafe { msg_send_id![run_loop_class, mainRunLoop] };
    // `kCFRunLoopCommonModes` is toll-free bridged to `NSRunLoopCommonModes`.
    let common_modes = unsafe { kCFRunLoopCommonModes } as *const AnyObject;
    let _: () = unsafe { msg_send![&*link, addToRunLoop: &*run_loop, forMode: common_modes] };

    LINK.with(|cell| *cell.borrow_mut() = Some(link));
}
