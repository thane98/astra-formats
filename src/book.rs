use std::path::Path;

use anyhow::Result;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

const XML_PROLOG: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Book {
    #[serde(rename = "@Count")]
    pub count: usize,
    #[serde(rename = "Sheet")]
    pub sheets: Vec<Sheet>,
}

impl Book {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_str(&std::fs::read_to_string(path)?)
    }

    pub fn from_str(contents: &str) -> Result<Self> {
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
pub struct Sheet {
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
