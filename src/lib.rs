mod book;
mod msbt;

pub use anyhow as error;
pub use indexmap;

pub use book::*;
pub use msbt::MessageMap;

#[cfg(feature = "ffi")]
mod ffi;

#[cfg(feature = "atlas")]
mod atlas;
#[cfg(feature = "atlas")]
pub use atlas::*;
#[cfg(feature = "atlas")]
pub use image;

#[cfg(feature = "bundle")]
pub use binrw;
#[cfg(feature = "bundle")]
mod asset;
#[cfg(feature = "bundle")]
pub use asset::*;
#[cfg(feature = "bundle")]
mod bundle;
#[cfg(feature = "bundle")]
pub use bundle::*;

#[cfg(feature = "msbt_script")]
mod astra_script;
#[cfg(feature = "msbt_script")]
mod msbt_script;
#[cfg(feature = "msbt_script")]
pub use astra_script::{
    pack_astra_script, parse_astra_script, parse_astra_script_entry, ParseError,
};
#[cfg(feature = "msbt_script")]
pub use msbt_script::{
    pack_msbt_entries, pack_msbt_entry, parse_msbt_entry, parse_msbt_script, MsbtToken,
};
