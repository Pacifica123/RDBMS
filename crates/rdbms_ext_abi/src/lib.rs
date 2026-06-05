//! C-compatible extension ABI sketches.

/// ABI version supported by this crate.
pub const RDBMS_EXT_ABI_VERSION: u32 = 1;

/// Status code returned through the C-compatible extension boundary.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RdbmsStatus {
    /// Operation succeeded.
    Ok = 0,
    /// Operation failed.
    Error = 1,
}

/// Opaque host handle for native extensions.
#[repr(C)]
pub struct RdbmsHost {
    _private: [u8; 0],
}

/// Extension descriptor returned by a future plugin entry point.
#[repr(C)]
pub struct RdbmsExtensionDescriptor {
    /// ABI version expected by the extension.
    pub abi_version: u32,
    /// UTF-8 name pointer.
    pub name_ptr: *const u8,
    /// UTF-8 name length.
    pub name_len: usize,
    /// Initialization function.
    pub init: Option<extern "C" fn(*mut RdbmsHost) -> RdbmsStatus>,
}
