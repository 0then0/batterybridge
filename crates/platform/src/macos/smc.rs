//! AppleSMC ABI boundary. No caller-supplied key or byte sequence is exposed over IPC.
//! The ABI is undocumented by Apple. See docs/hardware.md for provenance and limitations.
use batterybridge_core::*;
use std::{
    ffi::{c_char, c_void},
    mem::size_of,
};
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Version {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct PowerLimit {
    version: u16,
    length: u16,
    cpu: u32,
    gpu: u32,
    memory: u32,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct KeyInfo {
    size: u32,
    kind: u32,
    attributes: u8,
    reserved: [u8; 3],
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Payload {
    key: u32,
    version: Version,
    reserved_version: [u8; 2],
    limits: PowerLimit,
    info: KeyInfo,
    result: u8,
    status: u8,
    command: u8,
    reserved_command: u8,
    data: u32,
    bytes: [u8; 32],
}
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *const c_void;
    fn IOServiceGetMatchingService(port: u32, matching: *const c_void) -> u32;
    fn IOServiceOpen(service: u32, task: u32, kind: u32, connection: *mut u32) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> i32;
}
unsafe extern "C" {
    static mach_task_self_: u32;
}
pub struct Smc(u32);
impl Drop for Smc {
    fn drop(&mut self) {
        // SAFETY: owns one successfully opened connection.
        unsafe {
            IOServiceClose(self.0);
        }
    }
}
fn check(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else if code as u32 == 0xe00002c1 || code as u32 == 0xe00002e2 {
        Err(Error::PermissionDenied(format!(
            "SMC IOKit 0x{:08x}; root or firmware entitlement required",
            code as u32
        )))
    } else {
        Err(Error::HardwareError(format!(
            "SMC IOKit 0x{:08x}",
            code as u32
        )))
    }
}
fn validate_codec(kind: u32, length: usize) -> Result<()> {
    let valid = match kind.to_be_bytes() {
        [b'u', b'i', b'8', b' '] => length == 1,
        [b'u', b'i', b'3', b'2'] => length == 4,
        [b'h', b'e', b'x', b' '] => true,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::Unsupported(
            "unexpected SMC key type or numeric length".into(),
        ))
    }
}
impl Smc {
    pub fn open() -> Result<Self> {
        // SAFETY: static service name, matching dictionary consumed by IOKit.
        let service =
            unsafe { IOServiceGetMatchingService(0, IOServiceMatching(c"AppleSMC".as_ptr())) };
        if service == 0 {
            return Err(Error::BackendUnavailable("AppleSMC unavailable".into()));
        }
        let mut connection = 0;
        // SAFETY: service is live, output pointer points to initialized u32, task is current process.
        let code = unsafe { IOServiceOpen(service, mach_task_self_, 0, &mut connection) };
        // SAFETY: GetMatchingService returned an owned reference, no longer needed.
        unsafe {
            IOObjectRelease(service);
        }
        check(code)?;
        Ok(Self(connection))
    }
    fn call(&self, input: Payload) -> Result<Payload> {
        let mut output = Payload::default();
        let mut len = size_of::<Payload>();
        // SAFETY: repr(C) 80-byte structures match AppleSMC ABI. Both buffers are
        // fully initialized, non-overlapping, and valid for the supplied lengths.
        let code = unsafe {
            IOConnectCallStructMethod(
                self.0,
                2,
                (&input as *const Payload).cast(),
                size_of::<Payload>(),
                (&mut output as *mut Payload).cast(),
                &mut len,
            )
        };
        check(code)?;
        if len != size_of::<Payload>() || output.result != 0 {
            return Err(Error::HardwareError(format!(
                "SMC result {}, payload length {}",
                output.result, len
            )));
        }
        Ok(output)
    }
    fn info(&self, key: [u8; 4]) -> Result<KeyInfo> {
        Ok(self
            .call(Payload {
                key: u32::from_be_bytes(key),
                command: 9,
                ..Payload::default()
            })?
            .info)
    }
    pub fn read(&self, key: [u8; 4], length: usize) -> Result<Vec<u8>> {
        let info = self.info(key)?;
        if length == 0 || length > 32 || info.size as usize != length {
            return Err(Error::Unsupported(
                "SMC key absent or unexpected length".into(),
            ));
        }
        validate_codec(info.kind, length)?;
        Ok(self
            .call(Payload {
                key: u32::from_be_bytes(key),
                command: 5,
                info,
                ..Payload::default()
            })?
            .bytes[..length]
            .to_vec())
    }
    fn write(&self, key: [u8; 4], value: &[u8]) -> Result<()> {
        if self.read(key, value.len())? == value {
            return Ok(());
        }
        let info = self.info(key)?;
        let mut input = Payload {
            key: u32::from_be_bytes(key),
            command: 6,
            info,
            ..Payload::default()
        };
        input.bytes[..value.len()].copy_from_slice(value);
        self.call(input)?;
        // Firmware may apply a write a moment later, e.g. around a sleep
        // transition, so a stale first readback is retried briefly.
        for attempt in 0..5 {
            if self.read(key, value.len())? == value {
                return Ok(());
            }
            if attempt < 4 {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        Err(Error::HardwareError("SMC write readback mismatch".into()))
    }
    /// Intel firmware-managed maximum. Never expose arbitrary key writes over IPC.
    pub fn set_stop(&self, stop: u8) -> Result<()> {
        if !(50..=100).contains(&stop) {
            return Err(Error::InvalidLimit("Intel BCLM supports 50..100%".into()));
        }
        self.write(*b"BCLM", &[stop])
    }
    pub fn gate(&self, enabled: bool) -> Result<()> {
        if self.read(*b"CHTE", 4).is_ok() {
            return self.write(
                *b"CHTE",
                if enabled {
                    &[0, 0, 0, 0]
                } else {
                    &[1, 0, 0, 0]
                },
            );
        }
        if self.read(*b"CH0B", 1).is_err() {
            return self.write(*b"CH0C", &[u8::from(!enabled)]);
        }
        self.read(*b"CH0B", 1)?;
        self.read(*b"CH0C", 1)?;
        let v = if enabled { 0 } else { 2 };
        self.write(*b"CH0B", &[v])?;
        if let Err(error) = self.write(*b"CH0C", &[v]) {
            // Release a partially applied inhibition. Preserve the original error.
            let _ = self.write(*b"CH0B", &[0]);
            return Err(error);
        }
        Ok(())
    }
    pub fn led(&self, mode: LedMode) -> Result<()> {
        let value = match mode {
            LedMode::System => 0,
            LedMode::Off => 1,
            LedMode::Green => 3,
            LedMode::Orange => 4,
            LedMode::SlowBlinkOrange => 6,
            LedMode::FastBlinkOrange => 7,
            LedMode::Automatic => {
                return Err(Error::ProtocolError(
                    "LED automatic mode must be resolved by policy".into(),
                ));
            }
        };
        self.write(*b"ACLC", &[value])
    }
    pub fn discharge(&self, enabled: bool) -> Result<()> {
        for (key, disabled) in [(*b"CHIE", 0x08), (*b"CH0J", 0x20), (*b"CH0I", 1)] {
            if self.read(key, 1).is_ok() {
                return self.write(key, &[if enabled { disabled } else { 0 }]);
            }
        }
        Err(Error::Unsupported("SMC adapter gate".into()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_codecs_require_their_declared_width() {
        assert!(validate_codec(u32::from_be_bytes(*b"ui8 "), 1).is_ok());
        assert!(validate_codec(u32::from_be_bytes(*b"ui32"), 4).is_ok());
        assert!(validate_codec(u32::from_be_bytes(*b"ui32"), 1).is_err());
        assert!(validate_codec(u32::from_be_bytes(*b"ui8 "), 4).is_err());
        assert!(validate_codec(u32::from_be_bytes(*b"ch8*"), 1).is_err());
    }
    #[test]
    fn smc_abi_layout() {
        assert_eq!(size_of::<Payload>(), 80);
        assert_eq!(std::mem::offset_of!(Payload, bytes), 48);
        assert_eq!(std::mem::offset_of!(Payload, limits), 12);
        assert_eq!(std::mem::offset_of!(Payload, result), 40);
        // Explicit reserved fields account for every byte copied across FFI.
        assert_eq!(
            size_of::<Payload>(),
            4 + size_of::<Version>()
                + 2
                + size_of::<PowerLimit>()
                + size_of::<KeyInfo>()
                + 4
                + 4
                + 32
        );
    }
}
