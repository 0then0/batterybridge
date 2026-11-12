//! Sleep notifications. The policy thread releases adapter isolation before acknowledging sleep.
use crate::PowerEvent;
use batterybridge_core::*;
use std::{
    ffi::c_void,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
type Cf = *const c_void;
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        context: *mut c_void,
        port: *mut *mut c_void,
        callback: extern "C" fn(*mut c_void, u32, u32, *mut c_void),
        notifier: *mut u32,
    ) -> u32;
    fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> Cf;
    fn IONotificationPortDestroy(port: *mut c_void);
    fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IOAllowPowerChange(connection: u32, token: isize) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: Cf;
    fn CFRunLoopGetCurrent() -> Cf;
    fn CFRunLoopAddSource(runloop: Cf, source: Cf, mode: Cf);
    fn CFRunLoopRun();
}
/// Owns a notification acknowledgement, not the connection. The connection lives
/// on the power thread until process exit. Drop acknowledges once, including send failures.
pub struct SleepAck {
    connection: u32,
    token: isize,
}
impl Drop for SleepAck {
    fn drop(&mut self) {
        // SAFETY: only the callback constructs tokens delivered by IOKit;
        // the registration connection outlives all events and each ack is owned once.
        unsafe {
            IOAllowPowerChange(self.connection, self.token);
        }
    }
}
struct Context {
    callback: Box<dyn Fn(PowerEvent) + Send>,
    sleeping: Arc<AtomicBool>,
    connection: u32,
}
extern "C" fn notify(context: *mut c_void, _service: u32, message: u32, argument: *mut c_void) {
    // SAFETY: a stable Box<Context> is kept alive for the registered run loop.
    let context = unsafe { &*context.cast::<Context>() };
    match message {
        0xe0000270 => drop(SleepAck {
            connection: context.connection,
            token: argument as isize,
        }),
        0xe0000280 => {
            context.sleeping.store(true, Ordering::SeqCst);
            (context.callback)(PowerEvent::Sleep(SleepAck {
                connection: context.connection,
                token: argument as isize,
            }));
        }
        0xe0000300 => {
            context.sleeping.store(false, Ordering::SeqCst);
            (context.callback)(PowerEvent::Wake);
        }
        _ => {}
    }
}
pub fn watch(callback: impl Fn(PowerEvent) + Send + 'static) -> Result<Arc<AtomicBool>> {
    let sleeping = Arc::new(AtomicBool::new(false));
    let state = sleeping.clone();
    let (ready, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut context = Box::new(Context {
            callback: Box::new(callback),
            sleeping: state,
            connection: 0,
        });
        let mut port = ptr::null_mut();
        let mut notifier = 0;
        // SAFETY: exclusive initialized out parameters and stable context pointer;
        // notification processing starts only after the connection field is set.
        let connection = unsafe {
            IORegisterForSystemPower(
                (&mut *context as *mut Context).cast(),
                &mut port,
                notify,
                &mut notifier,
            )
        };
        if connection == 0 {
            let _ = ready.send(false);
            return;
        }
        context.connection = connection;
        // SAFETY: registration returned a live notification port and runloop source.
        unsafe {
            CFRunLoopAddSource(
                CFRunLoopGetCurrent(),
                IONotificationPortGetRunLoopSource(port),
                kCFRunLoopDefaultMode,
            );
        }
        let _ = ready.send(true);
        // SAFETY: processes callbacks while context and registration remain alive.
        unsafe {
            CFRunLoopRun();
        }
        // SAFETY: run loop stopped; unregister callbacks before dropping context.
        unsafe {
            IODeregisterForSystemPower(&mut notifier);
            IONotificationPortDestroy(port);
            IOServiceClose(connection);
        }
    });
    if rx.recv().unwrap_or(false) {
        Ok(sleeping)
    } else {
        Err(Error::BackendUnavailable(
            "IOKit sleep notifications unavailable; service refuses SMC writes".into(),
        ))
    }
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(kind: Cf, level: u32, name: Cf, assertion: *mut u32) -> i32;
    fn IOPMAssertionRelease(assertion: u32) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: Cf, text: *const std::ffi::c_char, encoding: u32)
    -> Cf;
    fn CFRelease(value: Cf);
}
/// A process-scoped `PreventSystemSleep` assertion. On AC power it also keeps a
/// closed-lid Mac in dark wake, so the service can stop charging at the limit
/// before the system sleeps; macOS ignores it on battery power. The OS releases
/// it if the service crashes. Global sleep settings are never changed.
#[derive(Default)]
pub struct SleepAssertion(Option<u32>);
impl SleepAssertion {
    pub fn set(&mut self, enabled: bool) -> Result<()> {
        if !enabled {
            if let Some(id) = self.0 {
                // SAFETY: id is owned by this guard and released at most once on success.
                let result = unsafe { IOPMAssertionRelease(id) };
                if result != 0 {
                    return Err(Error::PlatformError(format!(
                        "release sleep assertion: {result}"
                    )));
                }
                self.0 = None;
            }
        } else if self.0.is_none() {
            // SAFETY: static C strings, default allocator, and initialized output. CF references
            // are released after IOKit has consumed them, including error paths.
            let result = unsafe {
                let kind = CFStringCreateWithCString(
                    ptr::null(),
                    c"PreventSystemSleep".as_ptr(),
                    0x08000100,
                );
                let name = CFStringCreateWithCString(
                    ptr::null(),
                    c"BatteryBridge battery care".as_ptr(),
                    0x08000100,
                );
                if kind.is_null() || name.is_null() {
                    if !kind.is_null() {
                        CFRelease(kind);
                    }
                    if !name.is_null() {
                        CFRelease(name);
                    }
                    return Err(Error::PlatformError(
                        "allocate sleep assertion strings".into(),
                    ));
                }
                let mut id = 0;
                let result = IOPMAssertionCreateWithName(kind, 255, name, &mut id);
                CFRelease(kind);
                CFRelease(name);
                if result == 0 {
                    self.0 = Some(id);
                }
                result
            };
            if result != 0 {
                return Err(Error::PlatformError(format!(
                    "create sleep assertion: {result}"
                )));
            }
        }
        Ok(())
    }
}
impl Drop for SleepAssertion {
    fn drop(&mut self) {
        let _ = self.set(false);
    }
}
