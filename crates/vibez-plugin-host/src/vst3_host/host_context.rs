//! Host-side IHostApplication with IMessage / IAttributeList support.
//!
//! Plugins receive this as the `context` argument of
//! `IPluginBase::initialize` (component AND controller) and via
//! `IPluginFactory3::setHostContext`. It matters far beyond
//! politeness: dual-component plugins link their controller to their
//! processor by allocating an IMessage from the host application and
//! sending it over the connection points. With a null context, JUCE
//! plugins silently leave the editor bound to an orphaned processor
//! (GUI moves, sound never changes) and DPF refuses to create a view
//! at all.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

// FUnknown IID: {00000000-0000-0000-C000-000000000046}
const FUNKNOWN_IID: [u8; 16] = crate::vst3_tuid([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
]);
// IHostApplication IID: {58E595CC-DB2D-4969-8B6A-AF8C36A664E5}
pub(crate) const IHOSTAPPLICATION_IID: [u8; 16] = crate::vst3_tuid([
    0x58, 0xE5, 0x95, 0xCC, 0xDB, 0x2D, 0x49, 0x69, 0x8B, 0x6A, 0xAF, 0x8C, 0x36, 0xA6, 0x64, 0xE5,
]);
// IMessage IID: {936F033B-C6C0-47DB-BB08-82F813C1E613}
pub(crate) const IMESSAGE_IID: [u8; 16] = crate::vst3_tuid([
    0x93, 0x6F, 0x03, 0x3B, 0xC6, 0xC0, 0x47, 0xDB, 0xBB, 0x08, 0x82, 0xF8, 0x13, 0xC1, 0xE6, 0x13,
]);
// IAttributeList IID: {1E5F0AEB-CC7F-4533-A254-401138AD5EE4}
pub(crate) const IATTRIBUTELIST_IID: [u8; 16] = crate::vst3_tuid([
    0x1E, 0x5F, 0x0A, 0xEB, 0xCC, 0x7F, 0x45, 0x33, 0xA2, 0x54, 0x40, 0x11, 0x38, 0xAD, 0x5E, 0xE4,
]);
// IPluginFactory3 IID: {4555A2AB-C123-4E57-9B12-291036878931}
pub(crate) const IPLUGINFACTORY3_IID: [u8; 16] = crate::vst3_tuid([
    0x45, 0x55, 0xA2, 0xAB, 0xC1, 0x23, 0x4E, 0x57, 0x9B, 0x12, 0x29, 0x10, 0x36, 0x87, 0x89, 0x31,
]);

const K_RESULT_OK: i32 = 0;
const K_RESULT_FALSE: i32 = 1;
const K_NO_INTERFACE: i32 = -1;
const K_INVALID_ARGUMENT: i32 = 2;

unsafe fn iid_slice(iid: *const u8) -> &'static [u8] {
    std::slice::from_raw_parts(iid, 16)
}

// ── IAttributeList ──

enum AttrValue {
    Int(i64),
    Float(f64),
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

#[repr(C)]
struct AttrListVtbl {
    query_interface: unsafe extern "system" fn(*mut AttrList, *const u8, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut AttrList) -> u32,
    release: unsafe extern "system" fn(*mut AttrList) -> u32,
    set_int: unsafe extern "system" fn(*mut AttrList, *const i8, i64) -> i32,
    get_int: unsafe extern "system" fn(*mut AttrList, *const i8, *mut i64) -> i32,
    set_float: unsafe extern "system" fn(*mut AttrList, *const i8, f64) -> i32,
    get_float: unsafe extern "system" fn(*mut AttrList, *const i8, *mut f64) -> i32,
    set_string: unsafe extern "system" fn(*mut AttrList, *const i8, *const u16) -> i32,
    get_string: unsafe extern "system" fn(*mut AttrList, *const i8, *mut u16, u32) -> i32,
    set_binary: unsafe extern "system" fn(*mut AttrList, *const i8, *const c_void, u32) -> i32,
    get_binary:
        unsafe extern "system" fn(*mut AttrList, *const i8, *mut *const c_void, *mut u32) -> i32,
}

static ATTR_VTBL: AttrListVtbl = AttrListVtbl {
    query_interface: attr_query_interface,
    add_ref: attr_add_ref,
    release: attr_release,
    set_int,
    get_int,
    set_float,
    get_float,
    set_string,
    get_string,
    set_binary,
    get_binary,
};

#[repr(C)]
struct AttrList {
    vtbl: *const AttrListVtbl,
    refcount: AtomicU32,
    values: Mutex<HashMap<String, AttrValue>>,
}

fn new_attr_list() -> *mut AttrList {
    Box::into_raw(Box::new(AttrList {
        vtbl: &ATTR_VTBL,
        refcount: AtomicU32::new(1),
        values: Mutex::new(HashMap::new()),
    }))
}

unsafe fn attr_key(id: *const i8) -> Option<String> {
    if id.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(id as *const std::os::raw::c_char)
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe extern "system" fn attr_query_interface(
    this: *mut AttrList,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    let iid = iid_slice(iid);
    if iid == FUNKNOWN_IID || iid == IATTRIBUTELIST_IID {
        attr_add_ref(this);
        *obj = this as *mut c_void;
        K_RESULT_OK
    } else {
        *obj = std::ptr::null_mut();
        K_NO_INTERFACE
    }
}

unsafe extern "system" fn attr_add_ref(this: *mut AttrList) -> u32 {
    (*this).refcount.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn attr_release(this: *mut AttrList) -> u32 {
    let remaining = (*this).refcount.fetch_sub(1, Ordering::AcqRel) - 1;
    if remaining == 0 {
        drop(Box::from_raw(this));
    }
    remaining
}

unsafe extern "system" fn set_int(this: *mut AttrList, id: *const i8, value: i64) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    (*this)
        .values
        .lock()
        .map(|mut v| {
            v.insert(key, AttrValue::Int(value));
            K_RESULT_OK
        })
        .unwrap_or(K_INVALID_ARGUMENT)
}

unsafe extern "system" fn get_int(this: *mut AttrList, id: *const i8, out: *mut i64) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    match (*this).values.lock() {
        Ok(v) => match v.get(&key) {
            Some(AttrValue::Int(value)) => {
                *out = *value;
                K_RESULT_OK
            }
            _ => K_RESULT_FALSE,
        },
        Err(_) => K_INVALID_ARGUMENT,
    }
}

unsafe extern "system" fn set_float(this: *mut AttrList, id: *const i8, value: f64) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    (*this)
        .values
        .lock()
        .map(|mut v| {
            v.insert(key, AttrValue::Float(value));
            K_RESULT_OK
        })
        .unwrap_or(K_INVALID_ARGUMENT)
}

unsafe extern "system" fn get_float(this: *mut AttrList, id: *const i8, out: *mut f64) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    match (*this).values.lock() {
        Ok(v) => match v.get(&key) {
            Some(AttrValue::Float(value)) => {
                *out = *value;
                K_RESULT_OK
            }
            _ => K_RESULT_FALSE,
        },
        Err(_) => K_INVALID_ARGUMENT,
    }
}

unsafe extern "system" fn set_string(this: *mut AttrList, id: *const i8, value: *const u16) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    if value.is_null() {
        return K_INVALID_ARGUMENT;
    }
    let mut buf = Vec::new();
    let mut p = value;
    while *p != 0 {
        buf.push(*p);
        p = p.add(1);
    }
    buf.push(0);
    (*this)
        .values
        .lock()
        .map(|mut v| {
            v.insert(key, AttrValue::Str(buf));
            K_RESULT_OK
        })
        .unwrap_or(K_INVALID_ARGUMENT)
}

unsafe extern "system" fn get_string(
    this: *mut AttrList,
    id: *const i8,
    out: *mut u16,
    size_bytes: u32,
) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    match (*this).values.lock() {
        Ok(v) => match v.get(&key) {
            Some(AttrValue::Str(value)) => {
                let cap = (size_bytes as usize / 2).min(value.len());
                if cap == 0 {
                    return K_RESULT_FALSE;
                }
                std::ptr::copy_nonoverlapping(value.as_ptr(), out, cap);
                // Ensure termination even when truncated.
                *out.add(cap - 1) = 0;
                K_RESULT_OK
            }
            _ => K_RESULT_FALSE,
        },
        Err(_) => K_INVALID_ARGUMENT,
    }
}

unsafe extern "system" fn set_binary(
    this: *mut AttrList,
    id: *const i8,
    data: *const c_void,
    size: u32,
) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    let bytes = if data.is_null() || size == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(data as *const u8, size as usize).to_vec()
    };
    (*this)
        .values
        .lock()
        .map(|mut v| {
            v.insert(key, AttrValue::Bin(bytes));
            K_RESULT_OK
        })
        .unwrap_or(K_INVALID_ARGUMENT)
}

unsafe extern "system" fn get_binary(
    this: *mut AttrList,
    id: *const i8,
    data: *mut *const c_void,
    size: *mut u32,
) -> i32 {
    let Some(key) = attr_key(id) else {
        return K_INVALID_ARGUMENT;
    };
    match (*this).values.lock() {
        Ok(v) => match v.get(&key) {
            Some(AttrValue::Bin(value)) => {
                // Pointer stays valid while the attribute list lives
                // and the value is not overwritten, matching host
                // implementations in the wild.
                *data = value.as_ptr() as *const c_void;
                *size = value.len() as u32;
                K_RESULT_OK
            }
            _ => K_RESULT_FALSE,
        },
        Err(_) => K_INVALID_ARGUMENT,
    }
}

// ── IMessage ──

#[repr(C)]
struct MessageVtbl {
    query_interface: unsafe extern "system" fn(*mut Message, *const u8, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Message) -> u32,
    release: unsafe extern "system" fn(*mut Message) -> u32,
    get_message_id: unsafe extern "system" fn(*mut Message) -> *const i8,
    set_message_id: unsafe extern "system" fn(*mut Message, *const i8),
    get_attributes: unsafe extern "system" fn(*mut Message) -> *mut AttrList,
}

static MESSAGE_VTBL: MessageVtbl = MessageVtbl {
    query_interface: msg_query_interface,
    add_ref: msg_add_ref,
    release: msg_release,
    get_message_id,
    set_message_id,
    get_attributes,
};

#[repr(C)]
struct Message {
    vtbl: *const MessageVtbl,
    refcount: AtomicU32,
    /// NUL-terminated message id, owned.
    id: Mutex<Vec<u8>>,
    attributes: *mut AttrList,
}

unsafe extern "system" fn msg_query_interface(
    this: *mut Message,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    let iid = iid_slice(iid);
    if iid == FUNKNOWN_IID || iid == IMESSAGE_IID {
        msg_add_ref(this);
        *obj = this as *mut c_void;
        K_RESULT_OK
    } else {
        *obj = std::ptr::null_mut();
        K_NO_INTERFACE
    }
}

unsafe extern "system" fn msg_add_ref(this: *mut Message) -> u32 {
    (*this).refcount.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn msg_release(this: *mut Message) -> u32 {
    let remaining = (*this).refcount.fetch_sub(1, Ordering::AcqRel) - 1;
    if remaining == 0 {
        attr_release((*this).attributes);
        drop(Box::from_raw(this));
    }
    remaining
}

unsafe extern "system" fn get_message_id(this: *mut Message) -> *const i8 {
    match (*this).id.lock() {
        Ok(id) if !id.is_empty() => id.as_ptr() as *const i8,
        _ => std::ptr::null(),
    }
}

unsafe extern "system" fn set_message_id(this: *mut Message, id: *const i8) {
    let new_id = if id.is_null() {
        Vec::new()
    } else {
        let mut bytes = std::ffi::CStr::from_ptr(id as *const std::os::raw::c_char)
            .to_bytes()
            .to_vec();
        bytes.push(0);
        bytes
    };
    if let Ok(mut slot) = (*this).id.lock() {
        *slot = new_id;
    }
}

unsafe extern "system" fn get_attributes(this: *mut Message) -> *mut AttrList {
    // Per COM convention the caller does not own this reference; the
    // list lives as long as the message.
    (*this).attributes
}

fn new_message() -> *mut Message {
    Box::into_raw(Box::new(Message {
        vtbl: &MESSAGE_VTBL,
        refcount: AtomicU32::new(1),
        id: Mutex::new(Vec::new()),
        attributes: new_attr_list(),
    }))
}

// ── IHostApplication ──

#[repr(C)]
struct HostAppVtbl {
    query_interface: unsafe extern "system" fn(*mut HostApp, *const u8, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut HostApp) -> u32,
    release: unsafe extern "system" fn(*mut HostApp) -> u32,
    get_name: unsafe extern "system" fn(*mut HostApp, *mut u16) -> i32,
    create_instance:
        unsafe extern "system" fn(*mut HostApp, *const u8, *const u8, *mut *mut c_void) -> i32,
}

static HOST_APP_VTBL: HostAppVtbl = HostAppVtbl {
    query_interface: host_query_interface,
    add_ref: host_add_ref,
    release: host_release,
    get_name: host_get_name,
    create_instance: host_create_instance,
};

#[repr(C)]
struct HostApp {
    vtbl: *const HostAppVtbl,
}
unsafe impl Sync for HostApp {}

static HOST_APP: HostApp = HostApp {
    vtbl: &HOST_APP_VTBL,
};

unsafe extern "system" fn host_query_interface(
    this: *mut HostApp,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    let iid = iid_slice(iid);
    if iid == FUNKNOWN_IID || iid == IHOSTAPPLICATION_IID {
        *obj = this as *mut c_void;
        K_RESULT_OK
    } else {
        *obj = std::ptr::null_mut();
        K_NO_INTERFACE
    }
}

// Process-lifetime static: refcounting is a no-op.
unsafe extern "system" fn host_add_ref(_this: *mut HostApp) -> u32 {
    1
}
unsafe extern "system" fn host_release(_this: *mut HostApp) -> u32 {
    1
}

unsafe extern "system" fn host_get_name(_this: *mut HostApp, name: *mut u16) -> i32 {
    if name.is_null() {
        return K_INVALID_ARGUMENT;
    }
    // String128: UTF-16, 128 code units max including terminator.
    for (i, ch) in "vibez".encode_utf16().enumerate() {
        *name.add(i) = ch;
    }
    *name.add(5) = 0;
    K_RESULT_OK
}

unsafe extern "system" fn host_create_instance(
    _this: *mut HostApp,
    cid: *const u8,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    let cid = iid_slice(cid);
    let iid = iid_slice(iid);
    if cid == IMESSAGE_IID && (iid == IMESSAGE_IID || iid == FUNKNOWN_IID) {
        *obj = new_message() as *mut c_void;
        return K_RESULT_OK;
    }
    if cid == IATTRIBUTELIST_IID && (iid == IATTRIBUTELIST_IID || iid == FUNKNOWN_IID) {
        *obj = new_attr_list() as *mut c_void;
        return K_RESULT_OK;
    }
    *obj = std::ptr::null_mut();
    K_NO_INTERFACE
}

/// Process-wide IHostApplication pointer, valid forever. Pass as the
/// `context` to IPluginBase::initialize and IPluginFactory3::
/// setHostContext.
pub(crate) fn host_application() -> *mut c_void {
    static PTR: OnceLock<usize> = OnceLock::new();
    *PTR.get_or_init(|| &HOST_APP as *const HostApp as usize) as *mut c_void
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sdk_bytes(tuid: [::std::os::raw::c_char; 16]) -> Vec<u8> {
        tuid.iter().map(|b| *b as u8).collect()
    }

    #[test]
    fn iids_match_sdk() {
        assert_eq!(
            IHOSTAPPLICATION_IID.as_slice(),
            sdk_bytes(vst3::Steinberg::Vst::IHostApplication_iid).as_slice()
        );
        assert_eq!(
            IMESSAGE_IID.as_slice(),
            sdk_bytes(vst3::Steinberg::Vst::IMessage_iid).as_slice()
        );
        assert_eq!(
            IATTRIBUTELIST_IID.as_slice(),
            sdk_bytes(vst3::Steinberg::Vst::IAttributeList_iid).as_slice()
        );
        assert_eq!(
            IPLUGINFACTORY3_IID.as_slice(),
            sdk_bytes(vst3::Steinberg::IPluginFactory3_iid).as_slice()
        );
    }

    #[test]
    fn message_roundtrip_via_vtable() {
        unsafe {
            let app = host_application() as *mut HostApp;
            let mut obj: *mut c_void = std::ptr::null_mut();
            let hr =
                host_create_instance(app, IMESSAGE_IID.as_ptr(), IMESSAGE_IID.as_ptr(), &mut obj);
            assert_eq!(hr, K_RESULT_OK);
            let msg = obj as *mut Message;

            set_message_id(msg, c"JuceVST3EditController".as_ptr() as *const i8);
            let id = get_message_id(msg);
            assert_eq!(
                std::ffi::CStr::from_ptr(id as *const std::os::raw::c_char)
                    .to_str()
                    .unwrap(),
                "JuceVST3EditController"
            );

            let attrs = get_attributes(msg);
            assert!(!attrs.is_null());
            let key = c"JuceVST3EditController".as_ptr() as *const i8;
            assert_eq!(set_int(attrs, key, 0x1234), K_RESULT_OK);
            let mut out = 0i64;
            assert_eq!(get_int(attrs, key, &mut out), K_RESULT_OK);
            assert_eq!(out, 0x1234);

            // Binary roundtrip
            let payload = [1u8, 2, 3, 4];
            assert_eq!(
                set_binary(attrs, key, payload.as_ptr() as *const c_void, 4),
                K_RESULT_OK
            );
            let mut data: *const c_void = std::ptr::null();
            let mut size = 0u32;
            assert_eq!(get_binary(attrs, key, &mut data, &mut size), K_RESULT_OK);
            assert_eq!(size, 4);
            assert_eq!(std::slice::from_raw_parts(data as *const u8, 4), &payload);

            msg_release(msg);
        }
    }
}
