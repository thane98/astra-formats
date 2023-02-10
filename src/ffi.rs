use std::cell::RefCell;
use std::ffi::{CStr, CString};

use anyhow::Result;
use image::DynamicImage;
use indexmap::IndexMap;

use crate::{AtlasBundle, MessageBundle, SpriteAtlasWrapper, TextBundle};

thread_local!(static ERROR_MESSAGE: RefCell<Option<String>> = RefCell::new(None));

#[no_mangle]
pub unsafe extern "C" fn text_bundle_open(path: *const i8) -> FfiResult<Box<TextBundle>> {
    let path = CStr::from_ptr(path).to_string_lossy().to_string();
    TextBundle::load(path).map(Box::new).into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_parse(
    data: *const u8,
    len: usize,
) -> FfiResult<Box<TextBundle>> {
    let slice = std::slice::from_raw_parts(data, len);
    TextBundle::from_slice(slice)
        .map(Box::new)
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_save(bundle: &TextBundle, path: *const i8) -> FfiResult<()> {
    let path = CStr::from_ptr(path).to_string_lossy().to_string();
    bundle.save(path).into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_serialize(bundle: &TextBundle) -> FfiResult<FfiVec<u8>> {
    bundle.serialize().map(|v| v.into()).into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_take_raw(bundle: &mut TextBundle) -> FfiResult<FfiVec<u8>> {
    bundle.take_raw().map(|bytes| Some(bytes).into()).into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_take_string(bundle: &mut TextBundle) -> FfiResult<*mut i8> {
    bundle
        .take_string()
        .map(|string| CString::new(string).unwrap().into_raw())
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_put_raw(
    bundle: &mut TextBundle,
    bytes: *const u8,
    length: usize,
) -> FfiResult<()> {
    let bytes = std::slice::from_raw_parts(bytes, length);
    bundle.replace_raw(bytes.into()).into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_put_string(
    bundle: &mut TextBundle,
    text: *const i8,
) -> FfiResult<()> {
    let cstr = CStr::from_ptr(text);
    bundle
        .replace_string(cstr.to_string_lossy().to_string())
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn text_bundle_free(_: Box<TextBundle>) {}

#[no_mangle]
pub unsafe extern "C" fn sprite_atlas_open(path: *const i8) -> FfiResult<Box<SpriteAtlasWrapper>> {
    let path = CStr::from_ptr(path).to_string_lossy().to_string();
    AtlasBundle::load(path)
        .and_then(|bundle| bundle.extract_data())
        .map(Box::new)
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn sprite_atlas_get_sprite(
    atlas: &SpriteAtlasWrapper,
    key: *const i8,
) -> FfiImage {
    let key = CStr::from_ptr(key).to_string_lossy();
    atlas.get_sprite(&key).into()
}

#[no_mangle]
pub unsafe extern "C" fn sprite_atlas_free(_: Box<SpriteAtlasWrapper>) {}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_open(path: *const i8) -> FfiResult<Box<MessageBundle>> {
    let path = CStr::from_ptr(path).to_string_lossy().to_string();
    MessageBundle::load(path)
        .map(Box::new)
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_parse(
    data: *const u8,
    len: usize,
) -> FfiResult<Box<MessageBundle>> {
    let slice = std::slice::from_raw_parts(data, len);
    MessageBundle::from_slice(slice)
        .map(Box::new)
        .into()
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_save(
    bundle: &mut MessageBundle,
    path: *const i8,
) -> FfiResult<()> {
    let path = CStr::from_ptr(path).to_string_lossy().to_string();
    bundle.save(path).into()
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_serialize(
    bundle: &mut MessageBundle,
) -> FfiResult<FfiVec<u8>> {
    bundle.serialize().map(|v| v.into()).into()
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_take_data(
    bundle: &mut MessageBundle,
) -> FfiVec<KeyValuePair> {
    bundle.take_data().into()
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_put_data(
    bundle: &mut MessageBundle,
    data: *const KeyValuePair,
    len: usize,
) {
    let mut map = IndexMap::new();
    for pair in std::slice::from_raw_parts(data, len) {
        map.insert((&pair.key).into(), (&pair.value).into());
    }
    bundle.replace_data(map);
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_clear(bundle: &mut MessageBundle) {
    bundle.clear_entries();
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_insert_entry(
    bundle: &mut MessageBundle,
    key_data: *const u8,
    key_len: usize,
    value_data: *const u8,
    value_len: usize,
) {
    let raw_key = std::slice::from_raw_parts(key_data, key_len);
    let raw_value = std::slice::from_raw_parts(value_data, value_len);
    bundle.insert_entry(
        String::from_utf8_lossy(raw_key).to_string(),
        String::from_utf8_lossy(raw_value).to_string(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_free(_: Box<MessageBundle>) {}

#[no_mangle]
pub unsafe extern "C" fn message_bundle_free_data(data: FfiVec<KeyValuePair>) {
    let pairs = Box::from_raw(std::slice::from_raw_parts_mut(data.data, data.len));
    for pair in &*pairs {
        let _ = Box::from_raw(pair.key.data);
        let _ = Box::from_raw(pair.value.data);
    }
}

#[no_mangle]
pub unsafe extern "C" fn astra_get_error_message() -> *mut i8 {
    ERROR_MESSAGE.with(|value| match value.borrow().as_ref() {
        Some(value) => CString::new(value.to_string()).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    })
}

#[no_mangle]
pub unsafe extern "C" fn astra_string_free(string: *mut i8) {
    let _ = CString::from_raw(string);
}

#[no_mangle]
pub unsafe extern "C" fn astra_bytes_free(bytes: FfiVec<u8>) {
    let _ = Box::from_raw(std::slice::from_raw_parts_mut(bytes.data, bytes.len));
}

#[repr(u32)]
pub enum FfiResult<T> {
    Ok(T),
    Err,
}

impl<T> From<Result<T>> for FfiResult<T> {
    fn from(value: Result<T>) -> Self {
        match value {
            Ok(value) => {
                ERROR_MESSAGE.with(|error_message| {
                    *error_message.borrow_mut() = None;
                });
                Self::Ok(value)
            }
            Err(err) => {
                ERROR_MESSAGE.with(|error_message| {
                    *error_message.borrow_mut() = Some(format!("{:?}", err));
                });
                Self::Err
            }
        }
    }
}

#[repr(C)]
pub struct FfiImage {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub data: FfiVec<u8>,
}

impl From<Option<DynamicImage>> for FfiImage {
    fn from(value: Option<DynamicImage>) -> Self {
        match value {
            Some(value) => Self {
                width: value.width(),
                height: value.height(),
                format: (&value).into(),
                data: Some(value.into_bytes()).into(),
            },
            None => Self {
                width: 0,
                height: 0,
                format: ImageFormat::Rgba8,
                data: None.into(),
            },
        }
    }
}

#[repr(u32)]
pub enum ImageFormat {
    Rgba8 = 0,
    L8 = 1,
}

impl From<&DynamicImage> for ImageFormat {
    fn from(value: &DynamicImage) -> Self {
        match value {
            DynamicImage::ImageRgba8(_) => ImageFormat::Rgba8,
            DynamicImage::ImageLuma8(_) => ImageFormat::L8,
            _ => unimplemented!(),
        }
    }
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

impl From<&FfiVec<u8>> for String {
    fn from(value: &FfiVec<u8>) -> Self {
        unsafe {
            let raw_slice = std::slice::from_raw_parts(value.data, value.len);
            String::from_utf8_lossy(raw_slice).to_string()
        }
    }
}

impl From<String> for FfiVec<u8> {
    fn from(value: String) -> Self {
        Self {
            len: value.len(),
            data: Box::leak(value.into_boxed_str()).as_mut_ptr(),
        }
    }
}

impl From<IndexMap<String, String>> for FfiVec<KeyValuePair> {
    fn from(map: IndexMap<String, String>) -> Self {
        let mut entries = Vec::with_capacity(map.len());
        for (key, value) in map {
            entries.push(KeyValuePair {
                key: key.into(),
                value: value.into(),
            })
        }
        entries.into()
    }
}

impl<T: Sized> From<Option<Vec<T>>> for FfiVec<T> {
    fn from(value: Option<Vec<T>>) -> Self {
        match value {
            Some(value) => value.into(),
            None => Self {
                len: 0,
                data: std::ptr::null_mut(),
            },
        }
    }
}

impl<T: Sized> From<Vec<T>> for FfiVec<T> {
    fn from(value: Vec<T>) -> Self {
        Self {
            len: value.len(),
            data: Box::leak(value.into_boxed_slice()).as_mut_ptr(),
        }
    }
}
