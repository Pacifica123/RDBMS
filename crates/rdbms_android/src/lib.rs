//! Android-facing dynamic library smoke boundary.
//!
//! Stage 10 deliberately keeps this crate small. It proves that the workspace
//! can produce an Android-compatible library artifact and that a JNI-shaped
//! symbol can cross the platform boundary without pulling Java, Android SDK or
//! native plugin loading into the storage engine crates.

use std::ffi::c_void;

/// Platform milestone exported by the Android smoke layer.
pub const ANDROID_PORT_STAGE: i32 = 10;

/// ABI version for the Android smoke boundary.
pub const ANDROID_SMOKE_ABI_VERSION: i32 = 1;

/// Return the current platform milestone as a plain Rust call.
pub fn android_port_stage() -> i32 {
    ANDROID_PORT_STAGE
}

/// Return the Android smoke ABI version as a plain Rust call.
pub fn android_smoke_abi_version() -> i32 {
    ANDROID_SMOKE_ABI_VERSION
}

/// Return a small string that forces the Android crate to link against the SQL
/// and core crates without opening files or allocating database state.
pub fn android_smoke_banner() -> String {
    let result_type = std::any::type_name::<rdbms_core::ExecResult>();
    let parser_type = std::any::type_name::<rdbms_sql::Statement>();
    format!("rdbms-android-stage-{ANDROID_PORT_STAGE}:{result_type}:{parser_type}")
}

/// JNI smoke function: `dev.rdbms.NativeSmoke.stage()`.
///
/// The raw pointer arguments are treated as opaque JNI handles. Stage 10 does
/// not dereference them and therefore does not require an unsafe block.
#[no_mangle]
pub extern "system" fn Java_dev_rdbms_NativeSmoke_stage(
    _env: *mut c_void,
    _class: *mut c_void,
) -> i32 {
    android_port_stage()
}

/// JNI smoke function: `dev.rdbms.NativeSmoke.abiVersion()`.
#[no_mangle]
pub extern "system" fn Java_dev_rdbms_NativeSmoke_abiVersion(
    _env: *mut c_void,
    _class: *mut c_void,
) -> i32 {
    android_smoke_abi_version()
}

/// JNI smoke function: `dev.rdbms.NativeSmoke.add(int, int)`.
#[no_mangle]
pub extern "system" fn Java_dev_rdbms_NativeSmoke_add(
    _env: *mut c_void,
    _class: *mut c_void,
    left: i32,
    right: i32,
) -> i32 {
    left.saturating_add(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn rust_smoke_reports_stage_and_abi() {
        assert_eq!(android_port_stage(), 10);
        assert_eq!(android_smoke_abi_version(), 1);
        assert!(android_smoke_banner().contains("rdbms-android-stage-10"));
    }

    #[test]
    fn jni_symbols_are_callable_as_host_smoke() {
        assert_eq!(Java_dev_rdbms_NativeSmoke_stage(ptr::null_mut(), ptr::null_mut()), 10);
        assert_eq!(
            Java_dev_rdbms_NativeSmoke_abiVersion(ptr::null_mut(), ptr::null_mut()),
            1
        );
        assert_eq!(
            Java_dev_rdbms_NativeSmoke_add(ptr::null_mut(), ptr::null_mut(), 20, 22),
            42
        );
    }
}
