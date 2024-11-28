use std::fmt::Debug;
use std::path::Path;
use std::str::FromStr;

use anyhow::{anyhow, bail, Result};
use indexmap::IndexMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

const XML_PROLOG: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Book {
    #[serde(rename = "@Count")]
    pub count: usize,
    #[serde(rename = "Sheet")]
    pub sheets: Vec<RawSheet>,
}

impl Book {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_string(&std::fs::read_to_string(path)?)
    }

    pub fn from_string(contents: &str) -> Result<Self> {
        let book: Self = quick_xml::de::from_str(contents)?;
        Ok(book)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        std::fs::write(path, self.serialize()?)?;
        Ok(())
    }

    pub fn serialize(&self) -> Result<String> {
        let mut text = String::from(XML_PROLOG);
        quick_xml::se::to_writer(&mut text, self)?;
        Ok(text)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RawSheet {
    #[serde(rename = "@Name")]
    pub name: String,
    #[serde(rename = "@Count")]
    pub count: usize,
    #[serde(rename = "Header")]
    pub header: SheetHeader,
    #[serde(rename = "Data")]
    pub data: SheetData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SheetHeader {
    #[serde(rename = "Param")]
    pub params: Vec<SheetHeaderParam>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SheetHeaderParam {
    #[serde(rename = "@Name")]
    pub name: String,
    #[serde(rename = "@Ident")]
    pub ident: String,
    #[serde(rename = "@Type")]
    pub type_name: String,
    #[serde(rename = "@Min")]
    pub min: Option<String>,
    #[serde(rename = "@Max")]
    pub max: Option<String>,
    #[serde(rename = "@Chg")]
    pub chg: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SheetData {
    #[serde(rename = "Param")]
    pub params: Vec<SheetDataParam>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SheetDataParam {
    #[serde(flatten)]
    pub values: IndexMap<String, String>,
}

pub trait PublicArrayEntry {
    fn get_key(&self) -> &str;
    fn key_identifier() -> &'static str;
}

pub trait UniqueBookEntry {
    fn get_id(&self) -> &str;
}

pub trait AstraBook: Sized {
    fn load<PathTy: AsRef<Path>>(path: PathTy) -> Result<Self>;
    fn save<PathTy: AsRef<std::path::Path>>(&self, path: PathTy) -> Result<()>;
    fn from_string(contents: impl AsRef<str>) -> Result<Self>;
    fn to_string(&self) -> Result<String>;
}

pub struct Sheet<T> {
    pub name: String,
    pub header: SheetHeader,
    pub data: T,
}

impl<T> Clone for Sheet<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            header: self.header.clone(),
            data: self.data.clone(),
        }
    }
}

impl<T> Debug for Sheet<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sheet")
            .field("name", &self.name)
            .field("header", &self.header)
            .field("data", &self.data)
            .finish()
    }
}

impl<T> TryFrom<RawSheet> for Sheet<T>
where
    T: FromSheetData,
{
    type Error = anyhow::Error;

    fn try_from(value: RawSheet) -> Result<Self> {
        Ok(Self {
            name: value.name,
            header: value.header,
            data: T::from_sheet_data(value.data)?,
        })
    }
}

impl<T> From<&Sheet<T>> for RawSheet
where
    T: ToSheetData,
{
    fn from(value: &Sheet<T>) -> Self {
        let data = value.data.to_sheet_data(&value.header);
        Self {
            name: value.name.clone(),
            count: data.params.len(),
            header: value.header.clone(),
            data,
        }
    }
}

impl<T> From<Sheet<T>> for RawSheet
where
    T: ToSheetData,
{
    fn from(value: Sheet<T>) -> Self {
        let data = value.data.to_sheet_data(&value.header);
        Self {
            name: value.name,
            count: data.params.len(),
            header: value.header,
            data,
        }
    }
}

pub trait FromSheetData: Sized {
    fn from_sheet_data(sheet: SheetData) -> Result<Self>;
}

impl<T> FromSheetData for Vec<T>
where
    T: FromSheetDataParam,
{
    fn from_sheet_data(sheet: SheetData) -> Result<Self> {
        let mut items = vec![];
        for row in sheet.params {
            items.push(T::from_sheet_data_param(row.values)?);
        }
        Ok(items)
    }
}

impl<T> FromSheetData for IndexMap<String, T>
where
    T: FromSheetDataParam + UniqueBookEntry,
{
    fn from_sheet_data(sheet: SheetData) -> Result<Self> {
        let mut items = IndexMap::new();
        for row in sheet.params {
            let item = T::from_sheet_data_param(row.values)?;
            let key = item.get_id();
            items.insert(key.to_string(), item);
        }
        Ok(items)
    }
}

impl<T> FromSheetData for IndexMap<String, Vec<T>>
where
    T: FromSheetDataParam + PublicArrayEntry,
{
    fn from_sheet_data(sheet: SheetData) -> Result<Self> {
        let mut items = IndexMap::new();
        let mut key = None;
        let mut bucket = vec![];
        for row in sheet.params.into_iter() {
            let new_key = row
                .values
                .get(T::key_identifier())
                .cloned()
                .unwrap_or_default();
            if new_key.is_empty() {
                if key.is_none() {
                    bail!("found values before a key in public array");
                }
                bucket.push(T::from_sheet_data_param(row.values)?);
            } else {
                if let Some(key) = key {
                    items.insert(key, std::mem::take(&mut bucket));
                }
                key = Some(new_key.to_string());
            }
        }
        if let Some(key) = key {
            items.insert(key, bucket);
        }
        Ok(items)
    }
}

pub trait FromSheetDataParam: Sized {
    fn from_sheet_data_param(values: IndexMap<String, String>) -> Result<Self>;
}

pub trait FromSheetParamAttribute: Sized {
    fn from_sheet_param_attribute(value: String) -> Result<Self>;
}

impl<T> FromSheetParamAttribute for Option<T>
where
    T: FromStr,
{
    fn from_sheet_param_attribute(value: String) -> Result<Self> {
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(T::from_str(&value).map_err(|_| {
                anyhow!("unable to parse from value '{}'", value)
            })?))
        }
    }
}

impl<T> FromSheetParamAttribute for Vec<T>
where
    T: FromStr + Default,
    <T as FromStr>::Err: std::fmt::Debug,
{
    fn from_sheet_param_attribute(value: String) -> Result<Self> {
        let mut items = vec![];
        for part in value.split(';').filter(|p| !p.is_empty()) {
            items.push(T::from_str(part).map_err(|err| anyhow!("{:?}", err))?);
        }
        Ok(items)
    }
}

impl FromSheetParamAttribute for String {
    fn from_sheet_param_attribute(value: String) -> Result<Self> {
        Ok(value)
    }
}

impl FromSheetParamAttribute for bool {
    fn from_sheet_param_attribute(value: String) -> Result<Self> {
        let value: bool = value.parse()?;
        Ok(value)
    }
}

pub trait ToSheetData {
    fn to_sheet_data(&self, header: &SheetHeader) -> SheetData;
}

impl<T> ToSheetData for Vec<T>
where
    T: ToSheetDataParam,
{
    fn to_sheet_data(&self, _header: &SheetHeader) -> SheetData {
        let mut params = vec![];
        for row in self {
            params.push(row.to_sheet_data_param());
        }
        SheetData { params }
    }
}

impl<T> ToSheetData for IndexMap<String, T>
where
    T: ToSheetDataParam,
{
    fn to_sheet_data(&self, _header: &SheetHeader) -> SheetData {
        let mut params = vec![];
        for row in self.values() {
            params.push(row.to_sheet_data_param());
        }
        SheetData { params }
    }
}

impl<T> ToSheetData for IndexMap<String, Vec<T>>
where
    T: ToSheetDataParam + PublicArrayEntry,
{
    fn to_sheet_data(&self, header: &SheetHeader) -> SheetData {
        let mut params = vec![];
        for (key, bucket) in self {
            params.push(SheetDataParam {
                values: header
                    .params
                    .iter()
                    .map(|param| {
                        let attribute_name = format!("@{}", param.ident);
                        if attribute_name == T::key_identifier() {
                            (attribute_name, key.clone())
                        } else if param.type_name == "flag" {
                            (attribute_name, "0".to_owned())
                        } else if param.type_name == "bool" {
                            (attribute_name, "false".to_owned())
                        } else {
                            (attribute_name, String::new())
                        }
                    })
                    .collect(),
            });
            for item in bucket {
                params.push(item.to_sheet_data_param());
            }
        }
        SheetData { params }
    }
}

pub trait ToSheetDataParam {
    fn to_sheet_data_param(&self) -> SheetDataParam {
        SheetDataParam {
            values: self.to_sheet_data_param_values(),
        }
    }

    fn to_sheet_data_param_values(&self) -> IndexMap<String, String>;
}

pub trait ToSheetParamAttribute {
    fn to_sheet_param_attribute(&self) -> String;
}

impl<T> ToSheetParamAttribute for Option<T>
where
    T: ToString,
{
    fn to_sheet_param_attribute(&self) -> String {
        match self {
            Some(value) => value.to_string(),
            None => String::new(),
        }
    }
}

impl<T> ToSheetParamAttribute for Vec<T>
where
    T: ToString,
{
    fn to_sheet_param_attribute(&self) -> String {
        let mut attr: String = self.iter().map(|item| item.to_string()).join(";");
        if !attr.is_empty() {
            attr.push(';');
        }
        attr
    }
}

impl ToSheetParamAttribute for String {
    fn to_sheet_param_attribute(&self) -> String {
        self.clone()
    }
}

impl ToSheetParamAttribute for bool {
    fn to_sheet_param_attribute(&self) -> String {
        self.to_string()
    }
}

macro_rules! sheet_number {
    ($target:ty) => {
        impl FromSheetParamAttribute for $target {
            fn from_sheet_param_attribute(value: String) -> Result<Self> {
                let value: $target = value.parse()?;
                Ok(value)
            }
        }

        impl ToSheetParamAttribute for $target {
            fn to_sheet_param_attribute(&self) -> String {
                self.to_string()
            }
        }
    };
}

sheet_number!(u8);
sheet_number!(i8);
sheet_number!(u16);
sheet_number!(i16);
sheet_number!(u32);
sheet_number!(i32);
sheet_number!(u64);
sheet_number!(i64);
sheet_number!(u128);
sheet_number!(i128);
sheet_number!(usize);
sheet_number!(isize);
sheet_number!(f32);
sheet_number!(f64);
