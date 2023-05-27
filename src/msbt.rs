use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use indexmap::IndexMap;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use anyhow::{bail, Result};

fn align(operand: u64, alignment: u64) -> u64 {
    (operand + (alignment - 1)) & !(alignment - 1)
}

#[derive(Debug, Default)]
pub struct MessageMap {
    pub num_buckets: usize,
    pub messages: IndexMap<String, Vec<u16>>,
}

impl MessageMap {
    pub fn load<T: AsRef<Path>>(path: T) -> Result<Self> {
        Self::from_slice(&std::fs::read(path.as_ref())?)
    }

    pub fn from_slice(raw_msbt: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(raw_msbt);
        cursor.set_position(0x20);
        let label_groups = parse_lbl1(&mut cursor)?;
        let mut atr1 = parse_atr1(&mut cursor)?;
        atr1.retain(|b| *b != 0);
        if !atr1.is_empty() {
            bail!("archive actually uses the atr1 section");
        }
        let txt2 = parse_txt2(&mut cursor)?;

        let num_buckets = label_groups.len();
        let mut messages = IndexMap::new();
        let mut labels: Vec<(String, u32)> = label_groups
            .into_iter()
            .flat_map(|g| g.into_iter())
            .collect();
        labels.sort_by(|a, b| a.1.cmp(&b.1));
        for (label, id) in labels {
            let id = id as usize;
            if id >= txt2.len() {
                bail!("label ID '{}' for label '{}' is out of bounds", id, label);
            }
            messages.insert(label, txt2[id].clone());
        }
        Ok(Self {
            num_buckets,
            messages,
        })
    }

    pub fn save<T: AsRef<Path>>(&mut self, path: T) -> Result<()> {
        if self.num_buckets == 0 {
            self.rehash_and_save(path)
        } else {
            std::fs::write(path, self.serialize()?)?;
            Ok(())
        }
    }

    pub fn rehash_and_save<T: AsRef<Path>>(&mut self, path: T) -> Result<()> {
        let contents = self.rehash_and_serialize()?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    pub fn serialize(&mut self) -> Result<Vec<u8>> {
        if self.num_buckets == 0 {
            self.rehash_and_serialize()
        } else {
            self.serialize_with_bucket_count(self.num_buckets)
        }
    }

    pub fn rehash_and_serialize(&mut self) -> Result<Vec<u8>> {
        self.serialize_with_bucket_count(if self.messages.is_empty() {
            0
        } else {
            self.messages.len() / 2 + 1
        })
    }

    fn serialize_with_bucket_count(&self, num_buckets: usize) -> Result<Vec<u8>> {
        let lbl1 = serialize_lbl1(&self.messages, num_buckets)?;
        let atr1 = serialize_atr1(self.messages.len())?;
        let txt2 = serialize_txt2(&self.messages)?;
        let file_length = lbl1.len() + atr1.len() + txt2.len() + 0x20;
        let mut buffer = vec![0; file_length];
        let mut cursor = Cursor::new(buffer.as_mut_slice());
        write_utf8(&mut cursor, "MsgStdBn")?;
        cursor.write_u8(0xFF)?;
        cursor.write_u8(0xFE)?;
        cursor.set_position(cursor.position() + 2);
        cursor.write_u8(0x01)?;
        cursor.write_u8(0x03)?;
        cursor.write_u8(0x03)?;
        cursor.set_position(cursor.position() + 3);
        cursor.write_u32::<LittleEndian>(file_length as u32)?;
        cursor.set_position(0x20);
        cursor.write_all(&lbl1)?;
        cursor.write_all(&atr1)?;
        cursor.write_all(&txt2)?;
        Ok(buffer)
    }
}

fn parse_lbl1(cursor: &mut Cursor<&[u8]>) -> Result<Vec<Vec<(String, u32)>>> {
    let magic = read_utf8(4, cursor)?;
    if magic != "LBL1" {
        bail!("expected magic number 'LBL1', found '{}'", magic);
    }
    let section_length = cursor.read_u32::<LittleEndian>()? as u64;
    cursor.set_position(cursor.position() + 8);
    let base_position = cursor.position();
    let count = cursor.read_u32::<LittleEndian>()?;
    let mut groups = vec![];
    for _ in 0..count {
        groups.push(parse_label(cursor, base_position)?);
    }
    cursor.set_position(align(base_position + section_length, 0x10));
    Ok(groups)
}

fn read_utf8(length: usize, cursor: &mut Cursor<&[u8]>) -> Result<String> {
    let mut buffer = vec![0; length];
    cursor.read_exact(&mut buffer)?;
    let (value, _, has_errors) = encoding_rs::UTF_8.decode(&buffer);
    if has_errors {
        bail!("utf-8 decoding error for '{}'", value);
    }
    Ok(value.into_owned())
}

fn parse_label(cursor: &mut Cursor<&[u8]>, base_position: u64) -> Result<Vec<(String, u32)>> {
    let count = cursor.read_u32::<LittleEndian>()?;
    let offset = cursor.read_u32::<LittleEndian>()? as u64;
    let end_position = cursor.position();
    cursor.set_position(base_position + offset);
    let mut items = vec![];
    for _ in 0..count {
        let length = cursor.read_u8()? as usize;
        let label = read_utf8(length, cursor)?;
        let id = cursor.read_u32::<LittleEndian>()?;
        items.push((label, id));
    }
    cursor.set_position(end_position);
    Ok(items)
}

fn parse_atr1(cursor: &mut Cursor<&[u8]>) -> Result<Vec<u8>> {
    let magic = read_utf8(4, cursor)?;
    if magic != "ATR1" {
        bail!("expected magic number 'ATR1', found '{}'", magic);
    }
    let section_length = cursor.read_u32::<LittleEndian>()? as u64;
    cursor.set_position(cursor.position() + 8);
    let base_position = cursor.position();
    let count = cursor.read_u32::<LittleEndian>()?;
    let unknown = cursor.read_u32::<LittleEndian>()?;
    if unknown != 1 {
        bail!("unknown is '{}' but it should be 1 (I think)", unknown);
    }
    let mut buffer = vec![0; count as usize];
    cursor.read_exact(&mut buffer)?;
    cursor.set_position(align(base_position + section_length, 0x10));
    Ok(buffer)
}

fn parse_txt2(cursor: &mut Cursor<&[u8]>) -> Result<Vec<Vec<u16>>> {
    let magic = read_utf8(4, cursor)?;
    if magic != "TXT2" {
        bail!("expected magic number 'TXT2', found '{}'", magic);
    }
    let section_length = cursor.read_u32::<LittleEndian>()? as u64;
    cursor.set_position(cursor.position() + 8);
    let base_position = cursor.position();
    let count = cursor.read_u32::<LittleEndian>()?;
    let mut offsets = vec![];
    for _ in 0..count {
        offsets.push(base_position + cursor.read_u32::<LittleEndian>()? as u64);
    }
    let mut entries = vec![];
    for i in 0..offsets.len() {
        let offset = offsets[i];
        let end = if i + 1 < offsets.len() {
            offsets[i + 1]
        } else {
            base_position + section_length
        };
        cursor.set_position(offset);
        let mut buffer = vec![];
        while cursor.position() < end {
            buffer.push(cursor.read_u16::<LittleEndian>()?);
        }
        entries.push(buffer);
    }
    Ok(entries)
}

fn write_utf8(cursor: &mut Cursor<&mut [u8]>, value: &str) -> Result<()> {
    let (buffer, _, errors) = encoding_rs::UTF_8.encode(value);
    if errors {
        bail!("failed to encode string '{}'", value);
    }
    cursor.write_all(&buffer)?;
    Ok(())
}

fn serialize_lbl1(messages: &IndexMap<String, Vec<u16>>, num_buckets: usize) -> Result<Vec<u8>> {
    // Bucketize labels.
    let mut buckets: Vec<Vec<(String, u32)>> = vec![vec![]; num_buckets];
    for (i, label) in messages.keys().enumerate() {
        let hash = hash_label(label, num_buckets as u32);
        if hash > buckets.len() as u32 {
            bail!("broken hash function - index out of bounds");
        }
        buckets[hash as usize].push((label.clone(), i as u32));
    }

    // Build the string blob.
    let base_position = buckets.len() * 8 + 4;
    let mut raw_text = vec![];
    let mut bucket_info = vec![];
    for bucket in buckets {
        bucket_info.push((bucket.len(), base_position + raw_text.len()));
        for (label, id) in bucket {
            let (buffer, _, errors) = encoding_rs::UTF_16LE.encode(&label);
            if errors {
                bail!("failed to encode '{}'", label);
            }
            raw_text.push(label.len() as u8);
            raw_text.extend(buffer.iter());
            raw_text.extend(id.to_le_bytes().into_iter());
        }
    }

    // Finally, stitch together the section.
    let length_without_header = bucket_info.len() * 8 + raw_text.len();
    let padded_length = align(length_without_header as u64 + 0x10, 16);
    let mut buffer = vec![0u8; padded_length as usize];
    let mut cursor = Cursor::new(buffer.as_mut_slice());
    write_utf8(&mut cursor, "LBL1")?;
    cursor.write_u32::<LittleEndian>(length_without_header as u32)?;
    cursor.set_position(cursor.position() + 8);
    cursor.write_u32::<LittleEndian>(bucket_info.len() as u32)?;
    for (length, offset) in bucket_info {
        cursor.write_u32::<LittleEndian>(length as u32)?;
        cursor.write_u32::<LittleEndian>(offset as u32)?;
    }
    cursor.write_all(&raw_text)?;
    let padded_length = align(length_without_header as u64 + 0x10, 16);
    while cursor.position() < padded_length {
        cursor.write_u8(0xAB)?;
    }
    Ok(buffer)
}

fn hash_label(label: &str, num_buckets: u32) -> u32 {
    let mut sum = 0;
    for b in label.bytes() {
        sum = u32::wrapping_mul(sum, 0x492u32);
        sum = u32::wrapping_add(sum, b as u32);
        sum &= 0xFFFFFFFF;
    }
    sum % num_buckets
}

fn serialize_atr1(count: usize) -> Result<Vec<u8>> {
    let length_without_header = count + 8;
    let padded_length = align(length_without_header as u64 + 0x10, 16);
    let mut buffer = vec![0u8; padded_length as usize];
    let mut cursor = Cursor::new(buffer.as_mut_slice());
    write_utf8(&mut cursor, "ATR1")?;
    cursor.write_u32::<LittleEndian>(length_without_header as u32)?;
    cursor.set_position(cursor.position() + 8);
    cursor.write_u32::<LittleEndian>(count as u32)?;
    cursor.write_u32::<LittleEndian>(1)?;
    cursor.set_position(cursor.position() + count as u64);
    let padded_length = align(length_without_header as u64 + 0x10, 16);
    while cursor.position() < padded_length {
        cursor.write_u8(0xAB)?;
    }
    Ok(buffer)
}

fn serialize_txt2(messages: &IndexMap<String, Vec<u16>>) -> Result<Vec<u8>> {
    let mut raw_text: Vec<u8> = vec![];
    let mut text_offsets = vec![];
    let base_position = messages.len() * 4 + 4;
    for message in messages.values() {
        text_offsets.push(base_position + raw_text.len());
        raw_text.extend(message.iter().flat_map(|b| b.to_le_bytes().into_iter()));
    }

    let length_without_header = messages.len() * 4 + raw_text.len() + 4;
    let padded_length = align(length_without_header as u64 + 0x10, 16);
    let mut buffer = vec![0u8; padded_length as usize];
    let mut cursor = Cursor::new(buffer.as_mut_slice());
    write_utf8(&mut cursor, "TXT2")?;
    cursor.write_u32::<LittleEndian>(length_without_header as u32)?;
    cursor.set_position(cursor.position() + 8);
    cursor.write_u32::<LittleEndian>(messages.len() as u32)?;
    for offset in text_offsets.iter().take(messages.len()) {
        cursor.write_u32::<LittleEndian>(*offset as u32)?;
    }
    cursor.write_all(&raw_text)?;
    while cursor.position() < padded_length {
        cursor.write_u8(0xAB)?;
    }
    Ok(buffer)
}
