//! Minimal in-memory IBStream COM object, required by
//! IComponent::getState / setState and
//! IEditController::setComponentState for plugin state persistence.
//!
//! VST3 hosts must hand plugins a host-implemented stream; there is
//! nothing to borrow from the plugin side, so this builds the COM
//! object by hand: a #[repr(C)] struct whose first field points at a
//! static vtable of extern "system" fns.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

// FUnknown IID: {00000000-0000-0000-C000-000000000046}
const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

// IBStream IID: {C3BF6EA2-3099-4752-9B6B-F9901EE33E9B}
const IBSTREAM_IID: [u8; 16] = [
    0xC3, 0xBF, 0x6E, 0xA2, 0x30, 0x99, 0x47, 0x52, 0x9B, 0x6B, 0xF9, 0x90, 0x1E, 0xE3, 0x3E, 0x9B,
];

const K_RESULT_OK: i32 = 0;
const K_NO_INTERFACE: i32 = -1;
const K_INVALID_ARGUMENT: i32 = 2;

// IBStream::seek modes
const K_IB_SEEK_SET: i32 = 0;
const K_IB_SEEK_CUR: i32 = 1;
const K_IB_SEEK_END: i32 = 2;

#[repr(C)]
struct Vtbl {
    // FUnknown
    query_interface:
        unsafe extern "system" fn(*mut Stream, *const u8, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Stream) -> u32,
    release: unsafe extern "system" fn(*mut Stream) -> u32,
    // IBStream
    read: unsafe extern "system" fn(*mut Stream, *mut c_void, i32, *mut i32) -> i32,
    write: unsafe extern "system" fn(*mut Stream, *mut c_void, i32, *mut i32) -> i32,
    seek: unsafe extern "system" fn(*mut Stream, i64, i32, *mut i64) -> i32,
    tell: unsafe extern "system" fn(*mut Stream, *mut i64) -> i32,
}

static VTBL: Vtbl = Vtbl {
    query_interface,
    add_ref,
    release,
    read,
    write,
    seek,
    tell,
};

#[repr(C)]
struct Stream {
    vtbl: *const Vtbl,
    refcount: AtomicU32,
    data: Vec<u8>,
    pos: usize,
}

unsafe extern "system" fn query_interface(
    this: *mut Stream,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    let iid = std::slice::from_raw_parts(iid, 16);
    if iid == FUNKNOWN_IID || iid == IBSTREAM_IID {
        add_ref(this);
        *obj = this as *mut c_void;
        K_RESULT_OK
    } else {
        *obj = std::ptr::null_mut();
        K_NO_INTERFACE
    }
}

unsafe extern "system" fn add_ref(this: *mut Stream) -> u32 {
    (*this).refcount.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn release(this: *mut Stream) -> u32 {
    let remaining = (*this).refcount.fetch_sub(1, Ordering::AcqRel) - 1;
    if remaining == 0 {
        drop(Box::from_raw(this));
    }
    remaining
}

unsafe extern "system" fn read(
    this: *mut Stream,
    buffer: *mut c_void,
    num_bytes: i32,
    num_read: *mut i32,
) -> i32 {
    let s = &mut *this;
    let want = num_bytes.max(0) as usize;
    let available = s.data.len().saturating_sub(s.pos);
    let n = want.min(available);
    if n > 0 {
        std::ptr::copy_nonoverlapping(s.data.as_ptr().add(s.pos), buffer as *mut u8, n);
        s.pos += n;
    }
    if !num_read.is_null() {
        *num_read = n as i32;
    }
    K_RESULT_OK
}

unsafe extern "system" fn write(
    this: *mut Stream,
    buffer: *mut c_void,
    num_bytes: i32,
    num_written: *mut i32,
) -> i32 {
    let s = &mut *this;
    let n = num_bytes.max(0) as usize;
    if n > 0 {
        let src = std::slice::from_raw_parts(buffer as *const u8, n);
        if s.pos + n > s.data.len() {
            s.data.resize(s.pos + n, 0);
        }
        s.data[s.pos..s.pos + n].copy_from_slice(src);
        s.pos += n;
    }
    if !num_written.is_null() {
        *num_written = n as i32;
    }
    K_RESULT_OK
}

unsafe extern "system" fn seek(this: *mut Stream, pos: i64, mode: i32, result: *mut i64) -> i32 {
    let s = &mut *this;
    let base: i64 = match mode {
        K_IB_SEEK_SET => 0,
        K_IB_SEEK_CUR => s.pos as i64,
        K_IB_SEEK_END => s.data.len() as i64,
        _ => return K_INVALID_ARGUMENT,
    };
    let new_pos = base + pos;
    if new_pos < 0 {
        return K_INVALID_ARGUMENT;
    }
    s.pos = new_pos as usize;
    if !result.is_null() {
        *result = new_pos;
    }
    K_RESULT_OK
}

unsafe extern "system" fn tell(this: *mut Stream, pos: *mut i64) -> i32 {
    if pos.is_null() {
        return K_INVALID_ARGUMENT;
    }
    *pos = (*this).pos as i64;
    K_RESULT_OK
}

/// Owned in-memory IBStream. Hand `as_ibstream()` to plugin calls;
/// the object stays alive until this wrapper drops (plugins may
/// addRef/release around their use, the wrapper holds its own ref).
pub(crate) struct MemoryStream {
    ptr: *mut Stream,
}

impl MemoryStream {
    /// Empty stream for the plugin to write into (getState).
    pub(crate) fn for_writing() -> Self {
        Self::with_data(Vec::new())
    }

    /// Stream pre-filled with saved state for the plugin to read
    /// (setState / setComponentState). Position starts at 0.
    pub(crate) fn with_data(data: Vec<u8>) -> Self {
        let boxed = Box::new(Stream {
            vtbl: &VTBL,
            refcount: AtomicU32::new(1),
            data,
            pos: 0,
        });
        Self {
            ptr: Box::into_raw(boxed),
        }
    }

    pub(crate) fn as_ibstream(&self) -> *mut c_void {
        self.ptr as *mut c_void
    }

    /// Copy out everything written so far.
    pub(crate) fn data(&self) -> Vec<u8> {
        unsafe { (*self.ptr).data.clone() }
    }
}

impl Drop for MemoryStream {
    fn drop(&mut self) {
        unsafe { release(self.ptr) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ibstream_iid_matches_sdk() {
        let sdk: Vec<u8> = vst3::Steinberg::IBStream_iid
            .iter()
            .map(|b| *b as u8)
            .collect();
        assert_eq!(IBSTREAM_IID.as_slice(), sdk.as_slice());
    }

    #[test]
    fn write_read_roundtrip_via_vtable() {
        let stream = MemoryStream::for_writing();
        let ptr = stream.as_ibstream() as *mut Stream;
        let payload = b"vibez state";
        unsafe {
            let mut written = 0i32;
            assert_eq!(
                write(
                    ptr,
                    payload.as_ptr() as *mut c_void,
                    payload.len() as i32,
                    &mut written
                ),
                K_RESULT_OK
            );
            assert_eq!(written as usize, payload.len());

            let mut result = 0i64;
            assert_eq!(seek(ptr, 0, K_IB_SEEK_SET, &mut result), K_RESULT_OK);

            let mut buf = [0u8; 32];
            let mut read_n = 0i32;
            assert_eq!(
                read(ptr, buf.as_mut_ptr() as *mut c_void, 32, &mut read_n),
                K_RESULT_OK
            );
            assert_eq!(&buf[..read_n as usize], payload);
        }
        assert_eq!(stream.data(), payload);
    }

    #[test]
    fn query_interface_and_refcount() {
        let stream = MemoryStream::for_writing();
        let ptr = stream.as_ibstream() as *mut Stream;
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                query_interface(ptr, IBSTREAM_IID.as_ptr(), &mut obj),
                K_RESULT_OK
            );
            assert!(!obj.is_null());
            assert_eq!(release(ptr), 1); // back to the wrapper's ref

            let mut obj: *mut c_void = std::ptr::null_mut();
            let bogus = [0xABu8; 16];
            assert_eq!(
                query_interface(ptr, bogus.as_ptr(), &mut obj),
                K_NO_INTERFACE
            );
        }
    }
}
