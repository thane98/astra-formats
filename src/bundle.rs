use std::io::{BufReader, Cursor, Read, Seek, Write};
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use binrw::{binrw, BinRead, BinWrite, NullString};
use encoding_rs::UTF_8;
use indexmap::IndexMap;
use itertools::Itertools;
use lzma_rs::decompress::UnpackedSize;

use crate::sprite_atlas::SpriteAtlasWrapper;
use crate::{
    Asset, AssetFile, MessageMap, MonoBehavior, Sprite, SpriteAtlas, TerrainData, TextAsset,
    Texture2D,
};

#[derive(Debug)]
pub struct Bundle {
    files: IndexMap<String, BundleFile>,
}

impl Bundle {
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self> {
        Self::from_slice(&std::fs::read(path)?)
    }

    pub fn list_files<T>(input: &mut T) -> Result<Vec<String>>
    where
        T: Read + Seek,
    {
        let meta_data = Self::read_header_and_meta_data(input)?;
        Ok(meta_data
            .nodes
            .into_iter()
            .map(|node| node.path.to_string())
            .collect_vec())
    }

    pub fn from_slice(raw_bundle: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(raw_bundle);
        let meta_data = Self::read_header_and_meta_data(&mut cursor)?;

        let mut blob = vec![];
        for block in &meta_data.blocks {
            let mut buffer = vec![0; block.compressed_size as usize];
            cursor.read_exact(&mut buffer)?;
            match block.flags & 0x3F {
                0 => blob.extend(buffer),
                1 => {
                    let mut reader = BufReader::new(buffer.as_slice());
                    let mut output_buffer: Vec<u8> = vec![];
                    let options = lzma_rs::decompress::Options {
                        unpacked_size: UnpackedSize::UseProvided(Some(
                            block.decompressed_size as u64,
                        )),
                        ..Default::default()
                    };
                    lzma_rs::lzma_decompress_with_options(
                        &mut reader,
                        &mut output_buffer,
                        &options,
                    )?;
                    blob.extend(output_buffer);
                }
                2 | 3 => {
                    blob.extend(lz4_flex::decompress(
                        &buffer,
                        block.decompressed_size as usize,
                    )?);
                }
                _ => bail!("unsupported compression type '{}'", block.flags & 0x3F),
            };
        }

        let mut files = IndexMap::new();
        for node in meta_data.nodes {
            let start = node.offset as usize;
            let end = (node.offset + node.size) as usize;
            if end > blob.len() || start >= blob.len() {
                bail!("corrupted file offset/size for node '{}'", node.path);
            }
            files.insert(
                node.path.to_string(),
                match node.file_type {
                    BundleFileType::Raw => BundleFile::Raw(blob[start..end].to_vec()),
                    BundleFileType::Assets => {
                        let mut cursor = Cursor::new(&blob[start..end]);
                        BundleFile::Assets(AssetFile::read_le(&mut cursor)?)
                    }
                },
            );
        }
        Ok(Self { files })
    }

    fn read_header_and_meta_data<T>(reader: &mut T) -> Result<MetaData>
    where
        T: Read + Seek,
    {
        let header = Header::read_be(reader)?;
        let mut buffer = vec![0; header.compressed_size as usize];
        reader.read_exact(&mut buffer)?;
        let decompressed_data = match header.flags & 0x3F {
            0 => buffer,
            1 => {
                let mut reader = BufReader::new(buffer.as_slice());
                let mut output_buffer: Vec<u8> = vec![];
                let options = lzma_rs::decompress::Options {
                    unpacked_size: UnpackedSize::UseProvided(Some(header.decompressed_size as u64)),
                    ..Default::default()
                };
                lzma_rs::lzma_decompress_with_options(&mut reader, &mut output_buffer, &options)?;
                output_buffer
            }
            2 | 3 => lz4_flex::decompress(&buffer, header.decompressed_size as usize)?,
            _ => bail!("unsupported compression type '{}'", header.flags & 0x3F),
        };

        let mut meta_data_cursor = Cursor::new(&decompressed_data);
        let meta_data = MetaData::read_be(&mut meta_data_cursor)?;
        Ok(meta_data)
    }

    pub fn save<T: AsRef<Path>>(&self, path: T) -> Result<()> {
        std::fs::write(path, self.serialize()?)?;
        Ok(())
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        // Combine files into a single buffer and build node data.
        let mut nodes = vec![];
        let mut uncompressed_blob = vec![];
        for (key, file) in &self.files {
            let base_size = uncompressed_blob.len() as u64;
            match file {
                BundleFile::Raw(raw_file) => uncompressed_blob.extend_from_slice(raw_file),
                BundleFile::Assets(assets_file) => {
                    let mut cursor = Cursor::new(&mut uncompressed_blob);
                    cursor.set_position(base_size);
                    assets_file.write_le(&mut cursor)?
                }
            }
            nodes.push(Node {
                offset: base_size,
                size: (uncompressed_blob.len() as u64 - base_size),
                file_type: file.into(),
                path: NullString::from(key.clone()),
            });
        }

        // Chunk the buffer and compress as LZ4.
        let mut compressed_blob = vec![];
        let mut blocks = vec![];
        for chunk_start in (0..uncompressed_blob.len()).step_by(0x20000) {
            let chunk_end = (chunk_start + 0x20000).min(uncompressed_blob.len());
            let chunk_buffer = lz4_flex::compress(&uncompressed_blob[chunk_start..chunk_end]);
            blocks.push(Block {
                decompressed_size: (chunk_end - chunk_start) as u32,
                compressed_size: chunk_buffer.len() as u32,
                flags: 0x3,
            });
            compressed_blob.extend(chunk_buffer);
        }
        uncompressed_blob.clear(); // Large buffer. Clear to reduce memory pressure.

        let meta_data = MetaData {
            guid: 0, // TODO: Do we need to fill this in for any file?
            block_count: blocks.len() as u32,
            blocks,
            node_count: nodes.len() as u32,
            nodes,
        };
        let mut meta_data_buffer = vec![];
        meta_data.write_be(&mut Cursor::new(&mut meta_data_buffer))?;

        let header = Header {
            magic: NullString::from("UnityFS"),
            format_version: 7,
            major_version: NullString::from("5.x.x"),
            minor_version: NullString::from("2020.3.18f1"),
            file_size: (compressed_blob.len() + meta_data_buffer.len() + 0x40) as u64,
            compressed_size: meta_data_buffer.len() as u32,
            decompressed_size: meta_data_buffer.len() as u32,
            flags: 64,
        };

        let mut output_buffer: Vec<u8> = vec![];
        let mut cursor = Cursor::new(&mut output_buffer);
        header.write_be(&mut cursor)?;
        cursor.write_all(&meta_data_buffer)?;
        cursor.write_all(&compressed_blob)?;
        Ok(output_buffer)
    }

    pub fn get_cab(&self) -> Option<&str> {
        self.files
            .keys()
            .find(|key| key.len() == 36 && key.starts_with("CAB-"))
            .map(|key| key.as_str())
    }

    pub fn get(&self, path: &str) -> Option<&BundleFile> {
        self.files.get(path)
    }

    pub fn get_mut(&mut self, path: &str) -> Option<&mut BundleFile> {
        self.files.get_mut(path)
    }

    pub fn rename(&mut self, original_file_name: &str, new_file_name: String) -> Result<()> {
        if let Some(contents) = self.files.remove(original_file_name) {
            self.files.insert(new_file_name, contents);
            Ok(())
        } else {
            bail!("bundle does not contain file '{}'", original_file_name)
        }
    }

    pub fn rename_cab(&mut self, new_file_name: String) -> Result<()> {
        if let Some(cab) = self.get_cab().map(|c| c.to_string()) {
            self.rename(&cab, new_file_name)
        } else {
            bail!("could not identify cab file")
        }
    }

    pub fn files(&self) -> impl Iterator<Item = (&String, &BundleFile)> {
        self.files.iter()
    }
}

#[binrw(assert(format_version = 7), assert(magic = "UnityFS"))]
#[derive(Debug)]
struct Header {
    magic: NullString,
    #[brw(align_before = 4)]
    format_version: u32,
    major_version: NullString,
    minor_version: NullString,
    file_size: u64,
    compressed_size: u32,
    decompressed_size: u32,
    #[brw(align_after = 16)]
    flags: u32,
}

#[binrw]
#[derive(Debug)]
struct MetaData {
    guid: i128,
    block_count: u32,
    #[br(count = block_count)]
    blocks: Vec<Block>,
    node_count: u32,
    #[br(count = node_count)]
    nodes: Vec<Node>,
}

#[binrw]
#[derive(Debug)]
struct Block {
    decompressed_size: u32,
    compressed_size: u32,
    flags: u16,
}

#[binrw]
#[derive(Debug)]
struct Node {
    offset: u64,
    size: u64,
    file_type: BundleFileType,
    path: NullString,
}

#[binrw]
#[brw(repr = u32)]
#[derive(Debug, Clone, Copy)]
enum BundleFileType {
    Raw = 0,
    Assets = 4,
}

impl From<&BundleFile> for BundleFileType {
    fn from(value: &BundleFile) -> Self {
        match value {
            BundleFile::Raw(_) => BundleFileType::Raw,
            BundleFile::Assets(_) => BundleFileType::Assets,
        }
    }
}

#[derive(Debug)]
pub enum BundleFile {
    Raw(Vec<u8>),
    Assets(AssetFile),
}

#[derive(Debug)]
pub struct TextBundle(Bundle);

impl TextBundle {
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self> {
        Bundle::load(path).map(Self)
    }

    pub fn from_slice(raw_bundle: &[u8]) -> Result<Self> {
        Bundle::from_slice(raw_bundle).map(Self)
    }

    pub fn save<T: AsRef<Path>>(&self, path: T) -> Result<()> {
        self.0.save(path)
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        self.0.serialize()
    }

    pub fn rename(&mut self, original_file_name: &str, new_file_name: String) -> Result<()> {
        self.0.rename(original_file_name, new_file_name)
    }

    pub fn rename_cab(&mut self, new_file_name: String) -> Result<()> {
        self.0.rename_cab(new_file_name)
    }

    pub fn take_raw(&mut self) -> Result<Vec<u8>> {
        self.get_asset()
            .map(|text| std::mem::take(&mut text.data.items))
    }

    pub fn take_string(&mut self) -> Result<String> {
        self.get_asset().map(|text| {
            let data = std::mem::take(&mut text.data);
            let (text, _) = UTF_8.decode_with_bom_removal(&data);
            text.to_string()
        })
    }

    pub fn replace_raw(&mut self, new_data: Vec<u8>) -> Result<()> {
        let asset = self.get_asset()?;
        asset.data.items = new_data;
        Ok(())
    }

    pub fn replace_string(&mut self, new_data: String) -> Result<()> {
        self.replace_raw(new_data.into_bytes())
    }

    fn get_asset(&mut self) -> Result<&mut TextAsset> {
        self.0
            .files
            .values_mut()
            .find_map(|file| {
                if let BundleFile::Assets(assets_file) = file {
                    Some(assets_file)
                } else {
                    None
                }
            })
            .and_then(|assets_file| {
                assets_file.assets.iter_mut().find_map(|asset| {
                    if let Asset::Text(text) = asset {
                        Some(text)
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| anyhow!("bundle does not contain any text assets"))
    }
}

#[derive(Debug)]
pub struct TerrainBundle(Bundle);

impl TerrainBundle {
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self> {
        Bundle::load(path).map(Self)
    }

    pub fn from_slice(raw_bundle: &[u8]) -> Result<Self> {
        Bundle::from_slice(raw_bundle).map(Self)
    }

    pub fn save<T: AsRef<Path>>(&self, path: T) -> Result<()> {
        self.0.save(path)
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        self.0.serialize()
    }

    pub fn rename(&mut self, original_file_name: &str, new_file_name: String) -> Result<()> {
        self.0.rename(original_file_name, new_file_name)
    }

    pub fn rename_cab(&mut self, new_file_name: String) -> Result<()> {
        self.0.rename_cab(new_file_name)
    }

    pub fn take_data(&mut self) -> Result<MonoBehavior<TerrainData>> {
        self.get_asset().map(std::mem::take)
    }

    pub fn replace_data(&mut self, data: MonoBehavior<TerrainData>) -> Result<()> {
        let asset = self.get_asset()?;
        *asset = data;
        Ok(())
    }

    fn get_asset(&mut self) -> Result<&mut MonoBehavior<TerrainData>> {
        self.0
            .files
            .values_mut()
            .find_map(|file| {
                if let BundleFile::Assets(assets_file) = file {
                    Some(assets_file)
                } else {
                    None
                }
            })
            .and_then(|assets_file| {
                assets_file.assets.iter_mut().find_map(|asset| {
                    if let Asset::Terrain(terrain) = asset {
                        Some(terrain)
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| anyhow!("bundle does not contain any terrain assets"))
    }
}

#[derive(Debug)]
pub struct AtlasBundle(Bundle);

impl AtlasBundle {
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self> {
        Bundle::load(path).map(Self)
    }

    pub fn from_slice(raw_bundle: &[u8]) -> Result<Self> {
        Bundle::from_slice(raw_bundle).map(Self)
    }

    pub fn extract_data(mut self) -> Result<SpriteAtlasWrapper> {
        let resource_file = self.0.files.pop().map(|v| v.1);
        let assets_file = self.0.files.pop().map(|v| v.1);
        if let (Some(BundleFile::Assets(asset_file)), Some(BundleFile::Raw(image_data))) =
            (assets_file, resource_file)
        {
            let (texture, atlas, sprites) = extract_atlas_assets(asset_file)?;
            let texture_image = crate::texture::decode(&texture, &image_data)?;
            Ok(SpriteAtlasWrapper::new(texture_image, atlas, sprites))
        } else {
            bail!("could not identify asset and texture files in bundle")
        }
    }
}

fn extract_atlas_assets(asset_file: AssetFile) -> Result<(Texture2D, SpriteAtlas, Vec<Sprite>)> {
    let mut sprites = vec![];
    let mut texture = None;
    let mut atlas = None;
    for asset in asset_file.assets {
        match asset {
            Asset::Texture2D(asset) => texture = Some(asset),
            Asset::SpriteAtlas(asset) => atlas = Some(asset),
            Asset::Sprite(asset) => sprites.push(asset),
            _ => {}
        }
    }
    if let (Some(texture), Some(atlas)) = (texture, atlas) {
        Ok((texture, atlas, sprites))
    } else {
        bail!("could not extract assets required to build sprite atlas")
    }
}

#[derive(Debug)]
pub struct MessageBundle(TextBundle, MessageMap);

impl MessageBundle {
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self> {
        Self::from_slice(&std::fs::read(path)?)
    }

    pub fn from_slice(raw_bundle: &[u8]) -> Result<Self> {
        let mut text_bundle = TextBundle::from_slice(raw_bundle)?;
        let raw_msbt = text_bundle.take_raw()?;
        Ok(Self(text_bundle, MessageMap::from_slice(&raw_msbt)?))
    }

    pub fn save<T: AsRef<Path>>(&mut self, path: T) -> Result<()> {
        let raw_msbt = self.1.serialize()?;
        self.0.replace_raw(raw_msbt)?;
        self.0.save(path)
    }

    pub fn serialize(&mut self) -> Result<Vec<u8>> {
        let raw_msbt = self.1.serialize()?;
        self.0.replace_raw(raw_msbt)?;
        self.0.serialize()
    }

    pub fn rename(&mut self, original_file_name: &str, new_file_name: String) -> Result<()> {
        self.0.rename(original_file_name, new_file_name)
    }

    pub fn rename_cab(&mut self, new_file_name: String) -> Result<()> {
        self.0.rename_cab(new_file_name)
    }

    pub fn take_data(&mut self) -> IndexMap<String, String> {
        std::mem::take(&mut self.1.messages)
    }

    pub fn replace_data(&mut self, new_data: IndexMap<String, String>) {
        self.1.messages = new_data;
    }

    pub fn clear_entries(&mut self) {
        self.1.messages.clear();
    }

    pub fn insert_entry(&mut self, key: String, value: String) {
        self.1.messages.insert(key, value);
    }
}
