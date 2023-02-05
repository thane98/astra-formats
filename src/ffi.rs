use std::ffi::{CStr, CString};

use crate::{AtlasBundle, MessageMap, SpriteAtlasWrapper, TextBundle};

#[no_mangle]
pub unsafe extern "C" fn text_bundle_open(path: *const i8) -> Option<Box<TextBundle>> {
    let path = CStr::from_ptr(path).to_string_lossy().to_string();
    Some(Box::new(TextBundle::load(path).ok()?))
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_take_raw(bundle: &mut TextBundle) -> FfiVec<u8> {
    bundle.take_raw().ok().into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_take_string(bundle: &mut TextBundle) -> *mut i8 {
    let c_string = bundle
        .take_string()
        .ok()
        .map(|string| CString::new(string).unwrap())
        .unwrap_or_default();
    c_string.into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_put_raw(bundle: &mut TextBundle, bytes: FfiVec<u8>) {
    let bytes = Box::from_raw(std::slice::from_raw_parts_mut(bytes.data, bytes.len));
    let _ = bundle.replace_raw(bytes.into());
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_put_string(bundle: &mut TextBundle, text: *const i8) {
    let cstr = CStr::from_ptr(text);
    let _ = bundle.replace_string(cstr.to_string_lossy().to_string());
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_free(_: Box<TextBundle>) {}

#[no_mangle]
pub unsafe extern "C" fn sprite_atlas_open(path: *const i8) -> Option<Box<SpriteAtlasWrapper>> {
    let path = CStr::from_ptr(path).to_string_lossy().to_string();
    Some(Box::new(
        AtlasBundle::load(path)
            .and_then(|bundle| bundle.extract_data())
            .ok()?,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn sprite_atlas_get_sprite(
    atlas: &SpriteAtlasWrapper,
    key: *const i8,
) -> FfiVec<u8> {
    let key = CStr::from_ptr(key).to_string_lossy();
    atlas
        .get_sprite(&key)
        .map(|image| image.into_bytes())
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn sprite_atlas_free(_: Box<SpriteAtlasWrapper>) {}

#[no_mangle]
pub unsafe extern "C" fn msbt_parse(bytes: FfiVec<u8>) -> FfiVec<KeyValuePair> {
    let raw_msbt = Box::from_raw(std::slice::from_raw_parts_mut(bytes.data, bytes.len));
    if let Err(err) = MessageMap::from_slice(&raw_msbt) {
        println!("{:?}", err);
    }
    MessageMap::from_slice(&raw_msbt)
        .map(|msbt| {
            msbt.messages
                .into_iter()
                .map(|(k, v)| {
                    KeyValuePair {
                        key: k.into(),
                        value: v.into(),
                    }
                })
                .collect()
        })
        .ok()
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn msbt_free(msbt: FfiVec<KeyValuePair>) {
    let msbt = Box::from_raw(std::slice::from_raw_parts_mut(msbt.data, msbt.len));
    for pair in &*msbt {
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(pair.key.data, pair.key.len));
        let _ = Box::from_raw(std::slice::from_raw_parts_mut(pair.value.data, pair.value.len));
    }
}

#[no_mangle]
pub unsafe extern "C" fn astra_string_free(string: *mut i8) {
    let _ = CString::from_raw(string);
}

#[no_mangle]
pub unsafe extern "C" fn astra_bytes_free(bytes: FfiVec<u8>) {
    let _ = Box::from_raw(std::slice::from_raw_parts_mut(bytes.data, bytes.len));
}

#[repr(C)]
pub struct KeyValuePair {
    pub key: FfiVec<u8>,
    pub value: FfiVec<u8>,
}

#[repr(C)]
pub struct FfiVec<T> {
    pub len: usize,
    pub data: *mut T,
}

impl From<String> for FfiVec<u8> {
    fn from(value: String) -> Self {
        Self {
            len: value.len(),
            data: Box::leak(value.into_boxed_str()).as_mut_ptr(),
        }
    }
}

impl<T: Sized> From<Option<Vec<T>>> for FfiVec<T> {
    fn from(value: Option<Vec<T>>) -> Self {
        match value {
            Some(value) => Self {
                len: value.len(),
                data: Box::leak(value.into_boxed_slice()).as_mut_ptr(),
            },
            None => Self {
                len: 0,
                data: std::ptr::null_mut(),
            },
        }
    }
}
