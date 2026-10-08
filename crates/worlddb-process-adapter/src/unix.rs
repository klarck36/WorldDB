#[cfg(target_os = "macos")]
use std::ffi::{CStr, c_char, c_void};
use std::io;

const IDENTITY_DOMAIN: &[u8] = b"worlddb.unix-host-account.v1\0";

pub(super) fn current_host_account_identity_bytes() -> io::Result<Vec<u8>> {
    // SAFETY: geteuid has no pointer arguments and returns this process's
    // effective UID, the credential the OS uses for access checks.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let effective_uid = unsafe { libc::geteuid() } as u64;

    #[cfg(target_os = "linux")]
    {
        Ok(encode_identity(
            b"linux",
            effective_uid,
            linux_machine_id()?,
        ))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(encode_identity(
            b"macos",
            effective_uid,
            macos_platform_uuid()?,
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = effective_uid;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Unix host-account identity is not implemented for this operating system",
        ))
    }
}

fn encode_identity(platform: &[u8], effective_uid: u64, machine_id: [u8; 16]) -> Vec<u8> {
    let mut identity = Vec::with_capacity(IDENTITY_DOMAIN.len() + platform.len() + 1 + 8 + 16);
    identity.extend_from_slice(IDENTITY_DOMAIN);
    identity.extend_from_slice(platform);
    identity.push(0);
    identity.extend_from_slice(&effective_uid.to_le_bytes());
    identity.extend_from_slice(&machine_id);
    identity
}

#[cfg(target_os = "linux")]
fn linux_machine_id() -> io::Result<[u8; 16]> {
    let machine_id = std::fs::read("/etc/machine-id")?;
    parse_linux_machine_id(&machine_id)
}

#[cfg(target_os = "macos")]
// SAFETY: these declarations mirror the documented Apple IOKit ABI; handles are
// validated and released in macos_platform_uuid before their references expire.
// TEST: unix_identity::current_host_identity_is_stable_and_bounded.
// REVIEW: process-platform-adapter-owner.
#[allow(unsafe_code, reason = "WDB-EXC-0009")]
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: u32, matching: *const c_void) -> u32;
    fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: *const c_void,
        allocator: *const c_void,
        options: u32,
    ) -> *const c_void;
    fn IOObjectRelease(object: u32) -> i32;
}

#[cfg(target_os = "macos")]
// SAFETY: these declarations mirror the documented Apple CoreFoundation ABI;
// CF objects are type-checked and released in macos_platform_uuid.
// TEST: unix_identity::current_host_identity_is_stable_and_bounded.
// REVIEW: process-platform-adapter-owner.
#[allow(unsafe_code, reason = "WDB-EXC-0009")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        c_string: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetCString(
        value: *const c_void,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(value: *const c_void);
}

#[cfg(target_os = "macos")]
fn macos_platform_uuid() -> io::Result<[u8; 16]> {
    const IORESOURCES_CLASS: &[u8] = b"IOResources\0";
    const PLATFORM_UUID_KEY: &[u8] = b"IOPlatformUUID\0";
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    // SAFETY: IORESOURCES_CLASS is a static NUL-terminated class name; the
    // returned matching dictionary is consumed by IOServiceGetMatchingService.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let matching = unsafe { IOServiceMatching(IORESOURCES_CLASS.as_ptr().cast()) };
    if matching.is_null() {
        return Err(io::Error::other("IOKit could not create a service match"));
    }

    // SAFETY: matching is a live dictionary returned by IOServiceMatching;
    // the default main port is MACH_PORT_NULL (zero) and IOKit consumes it.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let service = unsafe { IOServiceGetMatchingService(0, matching.cast_const()) };
    if service == 0 {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "IOKit IOResources service is unavailable",
        ));
    }

    // SAFETY: PLATFORM_UUID_KEY is NUL-terminated; NULL selects the default
    // CoreFoundation allocator and UTF-8 is a supported string encoding.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let key = unsafe {
        CFStringCreateWithCString(
            std::ptr::null(),
            PLATFORM_UUID_KEY.as_ptr().cast(),
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    if key.is_null() {
        // SAFETY: service is the owned IOKit handle returned above.
        // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
        // REVIEW: process-platform-adapter-owner.
        #[allow(unsafe_code, reason = "WDB-EXC-0009")]
        let _ = unsafe { IOObjectRelease(service) };
        return Err(io::Error::other(
            "CoreFoundation could not create the platform UUID key",
        ));
    }

    // SAFETY: service is a live IOKit registry entry and key is a live CFString;
    // NULL selects the default allocator and zero means no special options.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let value = unsafe { IORegistryEntryCreateCFProperty(service, key, std::ptr::null(), 0) };
    // SAFETY: key is an owned CoreFoundation object and service is an owned
    // IOKit handle; each is released exactly once after the property lookup.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    unsafe {
        CFRelease(key);
        let _ = IOObjectRelease(service);
    }
    if value.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "IOKit IOPlatformUUID property is unavailable",
        ));
    }

    // SAFETY: value is the retained CoreFoundation object returned by IOKit;
    // type-ID queries accept any CFTypeRef.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let is_string = unsafe { CFGetTypeID(value) == CFStringGetTypeID() };
    if !is_string {
        // SAFETY: value is the owned CFTypeRef returned by IOKit.
        // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
        // REVIEW: process-platform-adapter-owner.
        #[allow(unsafe_code, reason = "WDB-EXC-0009")]
        unsafe {
            CFRelease(value);
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IOKit IOPlatformUUID property is not a string",
        ));
    }

    let mut output = [0_i8; 64];
    // SAFETY: value is a CFStringRef after the type check; output is a writable
    // 64-byte buffer and the size includes space for a terminating NUL.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let copied = unsafe {
        CFStringGetCString(
            value,
            output.as_mut_ptr(),
            output.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
        )
    } != 0;
    // SAFETY: value is the owned CoreFoundation object returned by IOKit.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    unsafe {
        CFRelease(value);
    }
    if !copied {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IOKit IOPlatformUUID could not be encoded as UTF-8",
        ));
    }

    // SAFETY: CoreFoundation reported success and wrote a NUL-terminated UTF-8
    // string into output, whose lifetime covers this parse.
    // TEST: unix_identity::current_host_identity_is_stable_and_bounded.
    // REVIEW: process-platform-adapter-owner.
    #[allow(unsafe_code, reason = "WDB-EXC-0009")]
    let bytes = unsafe { CStr::from_ptr(output.as_ptr()) }.to_bytes();
    parse_macos_machine_uuid(bytes)
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_machine_id(input: &[u8]) -> io::Result<[u8; 16]> {
    let input = trim_ascii_whitespace(input);
    if input.len() != 32 || input.contains(&b'-') {
        return Err(invalid_machine_id());
    }
    parse_hex_id(input)
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_machine_uuid(input: &[u8]) -> io::Result<[u8; 16]> {
    let input = input.strip_suffix(&[0]).unwrap_or(input);
    let input = trim_ascii_whitespace(input);
    if input.len() != 36
        || ![8, 13, 18, 23]
            .into_iter()
            .all(|index| input.get(index) == Some(&b'-'))
    {
        return Err(invalid_machine_id());
    }
    decode_hex_id(input, true)
}

#[cfg(any(target_os = "linux", test))]
fn parse_hex_id(input: &[u8]) -> io::Result<[u8; 16]> {
    decode_hex_id(input, false)
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn decode_hex_id(input: &[u8], is_canonical_uuid: bool) -> io::Result<[u8; 16]> {
    let mut digits = [0_u8; 32];
    let mut digit_slots = digits.iter_mut();
    let mut digit_count = 0;
    for (index, byte) in input.iter().enumerate() {
        if is_canonical_uuid && [8, 13, 18, 23].contains(&index) {
            continue;
        }
        if !byte.is_ascii_hexdigit() {
            return Err(invalid_machine_id());
        }
        let Some(slot) = digit_slots.next() else {
            return Err(invalid_machine_id());
        };
        *slot = byte.to_ascii_lowercase();
        digit_count += 1;
    }
    if digit_count != digits.len() {
        return Err(invalid_machine_id());
    }

    let mut result = [0_u8; 16];
    let mut result_slots = result.iter_mut();
    for pair in digits.chunks_exact(2) {
        let [high, low] = pair else {
            return Err(invalid_machine_id());
        };
        let Some(slot) = result_slots.next() else {
            return Err(invalid_machine_id());
        };
        *slot = (hex_nibble(*high)? << 4) | hex_nibble(*low)?;
    }
    if result.iter().all(|byte| *byte == 0) {
        return Err(invalid_machine_id());
    }
    Ok(result)
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn trim_ascii_whitespace(mut bytes: &[u8]) -> &[u8] {
    while let Some((first, rest)) = bytes.split_first() {
        if !first.is_ascii_whitespace() {
            break;
        }
        bytes = rest;
    }
    while let Some((last, rest)) = bytes.split_last() {
        if !last.is_ascii_whitespace() {
            break;
        }
        bytes = rest;
    }
    bytes
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn hex_nibble(byte: u8) -> io::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(invalid_machine_id()),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn invalid_machine_id() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "operating system returned an invalid host UUID",
    )
}

#[cfg(test)]
mod tests {
    use super::{encode_identity, parse_linux_machine_id, parse_macos_machine_uuid};

    #[test]
    fn machine_id_accepts_exact_hex_and_rejects_invalid_values() {
        assert_eq!(
            parse_linux_machine_id(b"0123456789abcdef0123456789abcdef\n").ok(),
            Some([
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ])
        );
        assert!(parse_linux_machine_id(b"00000000000000000000000000000000").is_err());
        assert!(parse_linux_machine_id(b"01234567-89abcdef0123456789abcdef").is_err());
        assert!(parse_linux_machine_id(b"not-a-machine-id").is_err());
    }

    #[test]
    fn macos_uuid_accepts_canonical_hyphens() {
        assert_eq!(
            parse_macos_machine_uuid(b"00112233-4455-6677-8899-aabbccddeeff\0").ok(),
            Some([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ])
        );
        assert!(parse_macos_machine_uuid(b"001-122334-455667-78899-aabbccddeeff").is_err());
    }

    #[test]
    fn host_and_account_components_are_bound_into_the_identity() {
        let machine_a = [1_u8; 16];
        let machine_b = [2_u8; 16];
        let identity = encode_identity(b"linux", 1000, machine_a);
        assert_eq!(identity, encode_identity(b"linux", 1000, machine_a));
        assert_ne!(identity, encode_identity(b"linux", 1001, machine_a));
        assert_ne!(identity, encode_identity(b"linux", 1000, machine_b));
        assert_ne!(identity, encode_identity(b"macos", 1000, machine_a));
        assert!(identity.len() <= 64);
    }

    #[test]
    fn current_host_identity_is_stable_and_bounded() {
        let first = super::current_host_account_identity_bytes();
        assert!(first.as_ref().is_ok_and(|identity| identity.len() <= 64));
        let second = super::current_host_account_identity_bytes();
        assert!(second.as_ref().is_ok_and(|identity| identity.len() <= 64));
        assert_eq!(first.as_ref().ok(), second.as_ref().ok());
    }
}
