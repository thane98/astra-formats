use std::collections::HashMap;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::ops::{Deref, DerefMut};

use anyhow::{bail, Result};
use binrw::meta::{EndianKind, ReadEndian, WriteEndian};
use binrw::{binread, binrw, BinRead, BinResult, BinWrite, Endian, NullString};
use byteorder::{BigEndian, WriteBytesExt};
use encoding_rs::UTF_8;
use itertools::{izip, Itertools};

const ASSET_BUNDLE_HASH: i128 = -138975531846078832632480790701341156713;
const TEXT_ASSET_HASH: i128 = -73723634408196252373272760413176173752;
const MESH_HASH: i128 = -72083215265324370365192905875055095371;
const AVATAR_HASH: i128 = -125274292396291140701441925783063757818;
const MATERIAL_HASH: i128 = 38580764854673472068260433708032579683;
const TRANSFORM_HASH: i128 = 113257848714874300609633886051190185078;
const GAME_OBJECT_HASH: i128 = -105210905878938171704810466949008006266;
const SKINNED_MESH_RENDERER_HASH: i128 = 7922304469189766333664176646841079590;
const ANIMATOR_HASH: i128 = -6849386489486133465282423213743802251;
const EMPTY_MONO_BEHAVIOR_HASH: i128 = 106062145148120627758638137336021039985;
const SPRING_BONE_MONO_BEHAVIOR_HASH: i128 = 86498438524189983969365375508344518452;
const SPRING_JOB_MONO_BEHAVIOR_HASH: i128 = -157219901295754190086520324959963540033;
const MONO_SCRIPT_HASH: i128 = -23841687017746243486512824057502732556;
const TEXTURE_2D_HASH: i128 = 51401989309282493850807588349188048909;
const SPRITE_HASH: i128 = 45701628647153051529734544331337206412;
const SPRITE_ATLAS_HASH: i128 = -21517008777126347833343678527744186422;
const TERRAIN_MONO_BEHAVIOR_TYPE_HASH: i128 = 161821592088346330348225465071444734321;

fn write_padding<W: Write + Seek>(writer: &mut W, align: u64) -> BinResult<()> {
    while writer.stream_position()? % align != 0 {
        writer.write_u8(0)?;
    }
    Ok(())
}

#[binread]
#[derive(Debug)]
#[br(little, assert(ref_type_count == 0))]
pub struct AssetFile {
    #[brw(big)]
    header: AssetFileHeader,

    #[br(temp)]
    type_count: u32,
    #[br(count = type_count)]
    types: Vec<AssetFileType>,

    #[br(align_after = 4, temp)]
    object_count: u32,
    #[br(count = object_count, temp)]
    objects: Vec<AssetFileObject>,
    #[br(calc = objects.iter().map(|obj| obj.path_id).collect())]
    path_ids: Vec<u64>,
    #[br(calc = calculate_object_order(&objects))]
    object_order: Vec<usize>,

    #[br(temp)]
    script_count: u32,
    #[br(count = script_count)]
    scripts: Vec<AssetScript>,

    #[br(temp)]
    external_count: u32,
    #[br(count = external_count)]
    externals: Vec<AssetExternal>,
    #[br(temp)]
    ref_type_count: u32,
    user_info: NullString,

    #[br(parse_with = |reader, endian, _: ()| read_assets(reader, endian, &types, &objects, header.data_offset))]
    pub assets: Vec<Asset>,
}

impl BinWrite for AssetFile {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> BinResult<()> {
        // Reserve space for the header. Don't know enough to build it yet.
        let base_position = writer.stream_position()?;
        for _ in 0..(0x36 + self.header.unity_version.len()) {
            writer.write_u8(0)?;
        }

        // Write the rest of the file (ignoring the header)
        let meta_data_base = writer.stream_position()?;
        (self.types.len() as u32).write_options(writer, endian, ())?;
        self.types.write_options(writer, endian, ())?;
        (self.assets.len() as u32).write_options(writer, endian, ())?;
        write_padding(writer, 4)?;
        // Objects. Don't know the object sizes yet, so come back later.
        let objects_position = writer.stream_position()?;
        for _ in 0..(24 * self.assets.len()) {
            writer.write_u8(0)?;
        }
        (self.scripts.len() as u32).write_options(writer, endian, ())?;
        self.scripts.write_options(writer, endian, ())?;
        (self.externals.len() as u32).write_options(writer, endian, ())?;
        self.externals.write_options(writer, endian, ())?;
        // Ref types - not supported yet.
        writer.write_u32::<BigEndian>(0)?;
        self.user_info.write_options(writer, endian, ())?;

        let meta_data_size = writer.stream_position()? - meta_data_base;
        write_padding(writer, 16)?;
        let data_offset = (writer.stream_position()? - base_position).max(0x1000);
        while writer.stream_position()? < base_position + data_offset {
            writer.write_u8(0)?;
        }

        // Build the assets blob + objects table.
        let type_hash_to_id: HashMap<i128, usize> = self
            .types
            .iter()
            .enumerate()
            .map(|(index, ty)| (ty.type_hash, index))
            .collect();
        let mut objects = vec![AssetFileObject::default(); self.assets.len()];
        let start = writer.stream_position()?;
        for (asset, object_index) in izip!(&self.assets, &self.object_order) {
            write_padding(writer, 8)?;
            let offset = writer.stream_position()? - start;
            asset.write_options(writer, endian, ())?;
            write_padding(writer, 4)?;
            objects[*object_index] = AssetFileObject {
                path_id: 0,
                offset,
                size: (writer.stream_position()? - start - offset) as u32,
                type_id: type_hash_to_id
                    .get(&asset.type_hash())
                    .map(|id| *id as u32)
                    .ok_or_else(|| binrw::Error::AssertFail {
                        pos: writer.stream_position().unwrap_or_default(),
                        message: String::from("could not map asset back to its type ID"),
                    })?,
            };
        }
        write_padding(writer, 4)?;
        for (i, path_id) in self.path_ids.iter().enumerate() {
            objects[i].path_id = *path_id;
        }

        // Fill in the header and object table.
        let end_position = writer.stream_position()?;
        writer.seek(SeekFrom::Start(base_position))?;
        let mut header = self.header.clone();
        header.version = 22;
        // While the unity version, platform, etc. are part of the header conceptually,
        // they are actually part of the meta data for size calculations.
        // TODO: Create a meta data type that holds all of this instead.
        header.meta_data_size = (meta_data_size + header.unity_version.len() as u64 + 6) as u32;
        header.file_size = end_position - base_position;
        header.data_offset = data_offset;
        header.platform = 38;
        header.enable_type_tree = 1;
        header.write_be(writer)?;
        writer.seek(SeekFrom::Start(objects_position))?;
        objects.write_options(writer, endian, ())?;
        writer.seek(SeekFrom::Start(end_position))?;
        Ok(())
    }
}

impl WriteEndian for AssetFile {
    const ENDIAN: EndianKind = EndianKind::Endian(Endian::Little);
}

fn read_assets<R: Read + Seek>(
    reader: &mut R,
    endian: Endian,
    types: &[AssetFileType],
    objects: &[AssetFileObject],
    data_offset: u64,
) -> BinResult<Vec<Asset>> {
    let mut assets = vec![];
    let mut sorted_objects = objects.iter().collect_vec();
    sorted_objects.sort_by(|a, b| a.offset.cmp(&b.offset));
    for obj in sorted_objects {
        let ty = &types[obj.type_id as usize]; // TODO: Bounds check.
        reader.seek(SeekFrom::Start(data_offset + obj.offset))?;
        assets.push(Asset::read_options(reader, endian, ty.type_hash)?);
    }
    Ok(assets)
}

// Object table entries appear to be ordered randomly.
// Since we want to retain the order of the objects table and assets when saving,
// we read assets sequentially but remember the order they appeared in the table.
fn calculate_object_order(objects: &[AssetFileObject]) -> Vec<usize> {
    let mut offset_ordered_objects = objects.iter().enumerate().collect_vec();
    offset_ordered_objects.sort_by(|a, b| a.1.offset.cmp(&b.1.offset));
    offset_ordered_objects
        .into_iter()
        .map(|(original_index, _)| original_index)
        .collect()
}

#[binrw]
#[derive(Clone, Debug)]
pub struct AssetFileHeader {
    pub junk: u64,
    pub version: u32,
    pub junk2: u64,
    pub meta_data_size: u32,
    pub file_size: u64,
    pub data_offset: u64,
    pub junk3: u64,
    pub unity_version: NullString,
    #[brw(little)]
    pub platform: u32,
    pub enable_type_tree: u8,
}

#[binrw(little)]
#[derive(Debug)]
pub struct AssetFileType {
    pub class_id: u32,
    pub is_stripped_type: u8,
    pub script_type_index: i16,
    #[br(if(class_id == 114))]
    #[bw(if(*class_id == 114))]
    pub script_id: i128,
    pub type_hash: i128,
    pub type_tree: AssetFileTypeTree,
    pub junk: u32,
}

impl AssetFileType {
    pub fn dump_tree(&self) -> Result<()> {
        println!("{} {}", self.type_hash, self.script_id);
        self.type_tree.dump()
    }
}

#[binrw]
#[derive(Debug)]
pub struct AssetFileTypeTree {
    pub node_count: u32,
    pub str_buffer_size: u32,
    #[br(count = node_count)]
    pub nodes: Vec<AssetFileTypeTreeNode>,
    #[br(count = str_buffer_size)]
    pub str_buffer: Vec<u8>,
}

impl AssetFileTypeTree {
    pub fn dump(&self) -> Result<()> {
        for node in &self.nodes {
            println!(
                "{}{}: {}",
                " ".repeat((node.level * 4) as usize),
                self.get_string(node.name_str_offset)?,
                self.get_string(node.type_str_offset)?,
            )
        }
        Ok(())
    }

    pub fn get_string(&self, value: u32) -> Result<String> {
        if (value & 0x80000000) != 0 {
            Ok(match value & 0x7FFFFFFF {
                0 => "AABB",
                5 => "AnimationClip",
                19 => "AnimationCurve",
                34 => "AnimationState",
                49 => "Array",
                55 => "Base",
                60 => "BitField",
                69 => "bitset",
                76 => "bool",
                81 => "char",
                86 => "ColorRGBA",
                96 => "Component",
                106 => "data",
                111 => "deque",
                117 => "double",
                124 => "dynamic_array",
                138 => "FastPropertyName",
                155 => "first",
                161 => "float",
                167 => "Font",
                172 => "GameObject",
                183 => "Generic Mono",
                196 => "GradientNEW",
                208 => "GUID",
                213 => "GUIStyle",
                222 => "int",
                226 => "list",
                231 => "long long",
                241 => "map",
                245 => "Matrix4x4f",
                256 => "MdFour",
                263 => "MonoBehaviour",
                277 => "MonoScript",
                288 => "m_ByteSize",
                299 => "m_Curve",
                307 => "m_EditorClassIdentifier",
                331 => "m_EditorHideFlags",
                349 => "m_Enabled",
                359 => "m_ExtensionPtr",
                374 => "m_GameObject",
                387 => "m_Index",
                395 => "m_IsArray",
                405 => "m_IsStatic",
                416 => "m_MetaFlag",
                427 => "m_Name",
                434 => "m_ObjectHideFlags",
                452 => "m_PrefabInternal",
                469 => "m_PrefabParentObject",
                490 => "m_Script",
                499 => "m_StaticEditorFlags",
                519 => "m_Type",
                526 => "m_Version",
                536 => "Object",
                543 => "pair",
                548 => "PPtr<Component>",
                564 => "PPtr<GameObject>",
                581 => "PPtr<Material>",
                596 => "PPtr<MonoBehaviour>",
                616 => "PPtr<MonoScript>",
                633 => "PPtr<Object>",
                646 => "PPtr<Prefab>",
                659 => "PPtr<Sprite>",
                672 => "PPtr<TextAsset>",
                688 => "PPtr<Texture>",
                702 => "PPtr<Texture2D>",
                718 => "PPtr<Transform>",
                734 => "Prefab",
                741 => "Quaternionf",
                753 => "Rectf",
                759 => "RectInt",
                767 => "RectOffset",
                778 => "second",
                785 => "set",
                789 => "short",
                795 => "size",
                800 => "SInt16",
                807 => "SInt32",
                814 => "SInt64",
                821 => "SInt8",
                827 => "staticvector",
                840 => "string",
                847 => "TextAsset",
                857 => "TextMesh",
                866 => "Texture",
                874 => "Texture2D",
                884 => "Transform",
                894 => "TypelessData",
                907 => "UInt16",
                914 => "UInt32",
                921 => "UInt64",
                928 => "UInt8",
                934 => "unsigned int",
                947 => "unsigned long long",
                966 => "unsigned short",
                981 => "vector",
                988 => "Vector2f",
                997 => "Vector3f",
                1006 => "Vector4f",
                1015 => "m_ScriptingClassIdentifier",
                1042 => "Gradient",
                1051 => "Type*",
                1057 => "int2_storage",
                1070 => "int3_storage",
                1083 => "BoundsInt",
                1093 => "m_CorrespondingSourceObject",
                1121 => "m_PrefabInstance",
                1138 => "m_PrefabAsset",
                1152 => "FileSize",
                1161 => "Hash128",
                _ => bail!("unknown type value '{}'", value & 0x7FFFFFFF),
            }
            .to_string())
        } else if value as usize > self.str_buffer.len() {
            bail!("value '{}' is out of bounds for str buffer", value);
        } else {
            let mut cursor = Cursor::new(&self.str_buffer);
            cursor.set_position(value as u64);
            let text: NullString = NullString::read_le(&mut cursor)?;
            Ok(text.to_string())
        }
    }
}

#[binrw]
#[derive(Debug)]
pub struct AssetFileTypeTreeNode {
    pub node_version: u16,
    pub level: u8,
    pub type_flags: u8,
    pub type_str_offset: u32,
    pub name_str_offset: u32,
    pub byte_size: i32,
    pub index: i32,
    pub meta_flag: i32,
    pub ref_type_hash: u64,
}

#[binrw]
#[derive(Debug, Default, Clone)]
pub struct AssetFileObject {
    pub path_id: u64,
    pub offset: u64,
    pub size: u32,
    pub type_id: u32,
}

#[binrw]
#[derive(Debug)]
pub struct AssetScript {
    pub file_id: u32,
    pub object_id: u64,
}

#[binrw]
#[derive(Debug)]
pub struct AssetExternal {
    pub unknown: NullString,
    pub guid: i128,
    pub ty: u32,
    pub path: NullString,
}

#[derive(Debug, BinWrite)]
pub enum Asset {
    Bundle(AssetBundle),
    Text(TextAsset),
    Script(MonoScript),
    Terrain(MonoBehavior<TerrainData>),
    Texture2D(Texture2D),
    SpriteAtlas(SpriteAtlas),
    Sprite(Sprite),
    EmptyMonoBehavior(MonoBehavior<()>),
    GameObject(GameObject),
    Animator(Animator),
    Mesh(Mesh),
    Avatar(Avatar),
    Transform(Transform),
    Material(Material),
    SkinnedMeshRenderer(SkinnedMeshRenderer),
    SpringJob(MonoBehavior<SpringJob>),
    SpringBone(MonoBehavior<SpringBone>),
}

impl Asset {
    pub fn type_hash(&self) -> i128 {
        match self {
            Asset::Bundle(_) => ASSET_BUNDLE_HASH,
            Asset::Text(_) => TEXT_ASSET_HASH,
            Asset::Script(_) => MONO_SCRIPT_HASH,
            Asset::Terrain(_) => TERRAIN_MONO_BEHAVIOR_TYPE_HASH,
            Asset::Texture2D(_) => TEXTURE_2D_HASH,
            Asset::SpriteAtlas(_) => SPRITE_ATLAS_HASH,
            Asset::Sprite(_) => SPRITE_HASH,
            Asset::EmptyMonoBehavior(_) => EMPTY_MONO_BEHAVIOR_HASH,
            Asset::GameObject(_) => GAME_OBJECT_HASH,
            Asset::Animator(_) => ANIMATOR_HASH,
            Asset::Mesh(_) => MESH_HASH,
            Asset::Avatar(_) => AVATAR_HASH,
            Asset::Transform(_) => TRANSFORM_HASH,
            Asset::Material(_) => MATERIAL_HASH,
            Asset::SkinnedMeshRenderer(_) => SKINNED_MESH_RENDERER_HASH,
            Asset::SpringJob(_) => SPRING_JOB_MONO_BEHAVIOR_HASH,
            Asset::SpringBone(_) => SPRING_BONE_MONO_BEHAVIOR_HASH,
        }
    }
}

impl BinRead for Asset {
    type Args<'a> = i128;

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        type_hash: Self::Args<'_>,
    ) -> BinResult<Self> {
        match type_hash {
            ASSET_BUNDLE_HASH => AssetBundle::read_options(reader, endian, ()).map(Self::Bundle),
            TEXT_ASSET_HASH => TextAsset::read_options(reader, endian, ()).map(Self::Text),
            MONO_SCRIPT_HASH => MonoScript::read_options(reader, endian, ()).map(Self::Script),
            TERRAIN_MONO_BEHAVIOR_TYPE_HASH => {
                MonoBehavior::<TerrainData>::read_options(reader, endian, ()).map(Self::Terrain)
            }
            TEXTURE_2D_HASH => Texture2D::read_options(reader, endian, ()).map(Self::Texture2D),
            SPRITE_ATLAS_HASH => {
                SpriteAtlas::read_options(reader, endian, ()).map(Self::SpriteAtlas)
            }
            EMPTY_MONO_BEHAVIOR_HASH => {
                MonoBehavior::<()>::read_options(reader, endian, ()).map(Self::EmptyMonoBehavior)
            }
            SPRITE_HASH => Sprite::read_options(reader, endian, ()).map(Self::Sprite),
            GAME_OBJECT_HASH => GameObject::read_options(reader, endian, ()).map(Self::GameObject),
            ANIMATOR_HASH => Animator::read_options(reader, endian, ()).map(Self::Animator),
            MESH_HASH => Mesh::read_options(reader, endian, ()).map(Self::Mesh),
            AVATAR_HASH => Avatar::read_options(reader, endian, ()).map(Self::Avatar),
            TRANSFORM_HASH => Transform::read_options(reader, endian, ()).map(Self::Transform),
            MATERIAL_HASH => Material::read_options(reader, endian, ()).map(Self::Material),
            SKINNED_MESH_RENDERER_HASH => {
                SkinnedMeshRenderer::read_options(reader, endian, ()).map(Self::SkinnedMeshRenderer)
            }
            SPRING_JOB_MONO_BEHAVIOR_HASH => {
                MonoBehavior::<SpringJob>::read_options(reader, endian, ()).map(Self::SpringJob)
            }
            SPRING_BONE_MONO_BEHAVIOR_HASH => {
                MonoBehavior::<SpringBone>::read_options(reader, endian, ()).map(Self::SpringBone)
            }
            _ => Err(binrw::Error::NoVariantMatch {
                pos: reader.stream_position()?,
            }),
        }
    }
}

impl ReadEndian for Asset {
    const ENDIAN: EndianKind = EndianKind::Endian(Endian::Little);
}

#[derive(Debug)]
pub struct UArray<T: std::fmt::Debug> {
    pub items: Vec<T>,
}

impl<'b, T> BinRead for UArray<T>
where
    T: BinRead<Args<'b> = ()> + std::fmt::Debug,
{
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> BinResult<Self> {
        let pos = reader.stream_position()? as i64;
        if pos % 4 != 0 {
            reader.seek(SeekFrom::Current(4 - pos % 4))?;
        }
        let count: u32 = BinRead::read_options(reader, endian, ())?;
        let mut items = vec![];
        for _ in 0..count {
            items.push(BinRead::read_options(reader, endian, ())?);
        }
        Ok(Self { items })
    }
}

impl<'b, T> BinWrite for UArray<T>
where
    T: BinWrite<Args<'b> = ()> + std::fmt::Debug,
{
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> BinResult<()> {
        while writer.stream_position()? % 4 != 0 {
            writer.write_u8(0)?;
        }
        (self.items.len() as u32).write_options(writer, endian, ())?;
        for item in &self.items {
            item.write_options(writer, endian, ())?;
        }
        Ok(())
    }
}

impl<T> Default for UArray<T>
where
    T: std::fmt::Debug + Default,
{
    fn default() -> Self {
        Self {
            items: Default::default(),
        }
    }
}

impl<T> Deref for UArray<T>
where
    T: std::fmt::Debug,
{
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl<T> DerefMut for UArray<T>
where
    T: std::fmt::Debug,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.items
    }
}

#[derive(Default, Clone)]
pub struct UString(pub String);

impl std::fmt::Debug for UString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for UString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for UString {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for UString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl BinRead for UString {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> BinResult<Self> {
        let position = reader.stream_position()? as i64;
        if position % 4 != 0 {
            reader.seek(SeekFrom::Current(4 - position % 4))?;
        }
        let count: u32 = <_>::read_options(reader, endian, ())?;
        let mut buffer = vec![0; count as usize];
        reader.read_exact(&mut buffer)?;
        let (cow, _) = UTF_8.decode_with_bom_removal(&buffer);
        Ok(Self(cow.to_string()))
    }
}

impl BinWrite for UString {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> BinResult<()> {
        write_padding(writer, 4)?;
        (self.len() as u32).write_options(writer, endian, ())?;
        writer.write_all(self.0.as_bytes())?;
        Ok(())
    }
}

#[binrw]
#[derive(Debug)]
pub struct AssetBundle {
    pub name: UString,
    pub preloads: UArray<PPtr>,
    pub container_map: UArray<(UString, AssetInfo)>,
    pub main_asset: AssetInfo,
    pub runtime_compatibility: u32,
    pub asset_bundle_name: UString,
    pub dependencies: UArray<UString>,
    #[brw(align_after = 4)]
    pub is_streamed_asset_bundle: u8,
    pub explicit_data_layout: u32,
    pub path_flags: u32,
    pub scene_hashes: UArray<(UString, UString)>,
}

#[binrw]
#[derive(Debug, Default)]
pub struct PPtr {
    #[brw(align_before = 4)]
    pub file_id: i32,
    pub path_id: i64,
}

#[binrw]
#[derive(Debug)]
pub struct AssetInfo {
    pub preload_index: u32,
    pub preload_size: u32,
    pub asset: PPtr,
}

#[binrw]
#[derive(Debug)]
pub struct GameObject {
    pub component: UArray<PPtr>,
    pub layer: u32,
    pub name: UString,
    #[brw(align_before = 4)]
    pub tag: u16,
    pub is_active: u8,
}

#[binrw]
#[derive(Debug)]
pub struct Transform {
    pub game_object: PPtr,
    pub local_rotation: Quaternionf,
    pub local_position: Vector3f,
    pub local_scale: Vector3f,
    pub children: UArray<PPtr>,
    pub father: PPtr,
}

#[binrw]
#[derive(Debug)]
pub struct Animator {
    pub game_object: PPtr,
    pub enabled: u8,
    #[brw(align_before = 4)]
    pub avatar: PPtr,
    pub controller: PPtr,
    pub culling_mode: u32,
    pub update_mode: u32,
    pub apply_root_motion: u8,
    #[brw(align_after = 4)]
    pub linear_velocity_blending: u8,
    pub has_transform_hierarchy: u8,
    pub allow_constant_clip_sampling_optimization: u8,
    pub keep_animator_controller_state_on_disable: u8,
}

#[binrw]
#[derive(Debug)]
pub struct TextAsset {
    pub name: UString,
    pub data: UArray<u8>,
}

#[binrw]
#[derive(Debug)]
pub struct MonoScript {
    pub name: UString,
    #[brw(align_before = 4)]
    pub execution_order: i32,
    pub properties_hash: i128,
    pub class_name: UString,
    pub namespace: UString,
    pub assembly_name: UString,
}

#[derive(Debug)]
pub struct MonoBehavior<T: std::fmt::Debug> {
    pub game_object: PPtr,
    // #[brw(align_after = 4)]
    pub enabled: u8,
    pub script: PPtr,
    pub name: UString,
    pub data: T,
}

impl<'b, T> BinRead for MonoBehavior<T>
where
    T: BinRead<Args<'b> = ()> + std::fmt::Debug,
{
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> BinResult<Self> {
        let game_object: PPtr = BinRead::read_options(reader, endian, ())?;
        let enabled: u8 = BinRead::read_options(reader, endian, ())?;
        reader.seek(SeekFrom::Current(3))?;
        Ok(Self {
            game_object,
            enabled,
            script: BinRead::read_options(reader, endian, ())?,
            name: BinRead::read_options(reader, endian, ())?,
            data: BinRead::read_options(reader, endian, ())?,
        })
    }
}

impl<'b, T> BinWrite for MonoBehavior<T>
where
    T: BinWrite<Args<'b> = ()> + std::fmt::Debug,
{
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> BinResult<()> {
        self.game_object.write_options(writer, endian, ())?;
        self.enabled.write_options(writer, endian, ())?;
        writer.write_u8(0)?;
        writer.write_u8(0)?;
        writer.write_u8(0)?;
        self.script.write_options(writer, endian, ())?;
        self.name.write_options(writer, endian, ())?;
        self.data.write_options(writer, endian, ())?;
        Ok(())
    }
}

impl<T> Default for MonoBehavior<T>
where
    T: std::fmt::Debug + Default,
{
    fn default() -> Self {
        Self {
            game_object: Default::default(),
            enabled: Default::default(),
            script: Default::default(),
            name: Default::default(),
            data: Default::default(),
        }
    }
}

#[binrw]
#[derive(Debug, Default)]
pub struct TerrainData {
    pub x: i32,
    pub z: i32,
    pub width: i32,
    pub height: i32,
    pub layers: UArray<TerrainLayerData>,
    pub overlaps: UArray<TerrainOverlapData>,
    pub terrains: UArray<UString>,
}

#[binrw(align_after = 4)]
#[derive(Debug, Default)]
pub struct TerrainLayerData {
    pub x: u8,
    pub y: u8,
    pub w: u8,
    pub h: u8,
    pub group: i32,
    pub attr: UArray<UString>,
}

#[binrw(align_after = 4)]
#[derive(Debug, Default)]
pub struct TerrainOverlapData {
    pub x: u8,
    pub y: u8,
    pub attr: UArray<UString>,
}

#[binrw]
#[derive(Debug)]
pub struct Texture2D {
    pub name: UString,
    #[brw(align_before = 4)]
    pub forced_fallback_format: i32,
    pub downscale_fallback: u8,
    pub is_alpha_channel_optional: u8,
    #[brw(align_before = 4)]
    pub width: u32,
    pub height: u32,
    pub complete_image_size: u32,
    pub mips_stripped: u32,
    pub texture_format: TextureFormat,
    pub mip_count: u32,
    pub is_readable: u8,
    pub is_pre_processed: u8,
    pub ignore_master_texture_limit: u8,
    pub streaming_mipmaps: u8,
    pub streaming_mipmaps_priority: i32,
    pub image_count: u32,
    pub texture_dimension: u32,
    pub texture_settings: GlTextureSettings,
    pub lightmap_format: i32,
    pub color_space: i32,
    pub platform_blob: UArray<u8>,
    pub image_data: UArray<u8>,
    pub stream_data: StreamingInfo,
}

#[binrw]
#[derive(Debug)]
pub struct GlTextureSettings {
    pub filter_mode: i32,
    pub aniso: i32,
    pub mip_bias: f32,
    pub wrap_u: i32,
    pub wrap_v: i32,
    pub wrap_w: i32,
}

#[binrw]
#[derive(Debug)]
pub struct StreamingInfo {
    pub offset: u64,
    pub size: u32,
    pub path: UString,
}

// Borrowed from https://github.com/gameltb/io_unity/
#[binrw]
#[brw(repr = u32)]
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
#[allow(non_camel_case_types)]
pub enum TextureFormat {
    Alpha8 = 1,
    ARGB4444,
    RGB24,
    RGBA32,
    ARGB32,
    ARGBFloat,
    RGB565,
    BGR24,
    R16,
    DXT1,
    DXT3,
    DXT5,
    RGBA4444,
    BGRA32,
    RHalf,
    RGHalf,
    RGBAHalf,
    RFloat,
    RGFloat,
    RGBAFloat,
    YUY2,
    RGB9e5Float,
    RGBFloat,
    BC6H,
    BC7,
    BC4,
    BC5,
    DXT1Crunched,
    DXT5Crunched,
    PVRTC_RGB2,
    PVRTC_RGBA2,
    PVRTC_RGB4,
    PVRTC_RGBA4,
    ETC_RGB4,
    ATC_RGB4,
    ATC_RGBA8,
    EAC_R = 41,
    EAC_R_SIGNED,
    EAC_RG,
    EAC_RG_SIGNED,
    ETC2_RGB,
    ETC2_RGBA1,
    ETC2_RGBA8,
    ASTC_RGB_4x4,
    ASTC_RGB_5x5,
    ASTC_RGB_6x6,
    ASTC_RGB_8x8,
    ASTC_RGB_10x10,
    ASTC_RGB_12x12,
    ASTC_RGBA_4x4,
    ASTC_RGBA_5x5,
    ASTC_RGBA_6x6,
    ASTC_RGBA_8x8,
    ASTC_RGBA_10x10,
    ASTC_RGBA_12x12,
    ETC_RGB4_3DS,
    ETC_RGBA8_3DS,
    RG16,
    R8,
    ETC_RGB4Crunched,
    ETC2_RGBA8Crunched,
    ASTC_HDR_4x4,
    ASTC_HDR_5x5,
    ASTC_HDR_6x6,
    ASTC_HDR_8x8,
    ASTC_HDR_10x10,
    ASTC_HDR_12x12,
    RG32,
    RGB48,
    RGBA64,
}

#[binrw]
#[derive(Debug)]
pub struct SpriteAtlas {
    pub name: UString,
    pub packed_sprites: UArray<PPtr>,
    pub sprite_names_to_index: UArray<UString>,
    pub render_data_map: UArray<(RenderDataKey, SpriteAtlasData)>,
    pub tag: UString,
    pub is_variant: u32,
}

#[binrw]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderDataKey {
    #[brw(align_before = 4)]
    pub guid: i128,
    pub second: u64,
}

#[binrw]
#[derive(Debug)]
pub struct SpriteAtlasData {
    pub texture: PPtr,
    pub alpha_texture: PPtr,
    pub texture_rect: RectF,
    pub texture_rect_offset: Vector2f,
    pub atlas_rect_offset: Vector2f,
    pub uv_transform: Vector4f,
    pub downscale_multiplier: f32,
    pub settings_raw: u32,
    pub secondary_textures: UArray<SecondarySpriteTexture>,
}

#[binrw]
#[derive(Debug)]
pub struct Sprite {
    pub name: UString,
    pub rect: RectF,
    pub offset: Vector2f,
    pub border: Vector4f,
    pub pixels_to_units: f32,
    pub pivot: Vector2f,
    pub extrude: u32,
    pub is_polygon: u8,
    pub render_data_key: RenderDataKey,
    pub atlas_tags: UArray<UString>,
    pub sprite_atlas: PPtr,
    pub sprite_render_data: SpriteRenderData,
    pub physics_shape: UArray<UArray<Vector2f>>,
    pub bones: UArray<SpriteBone>,
}

#[binrw]
#[derive(Debug)]
pub struct RectF {
    #[brw(align_before = 4)]
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[binrw]
#[derive(Debug)]
pub struct Vector2f {
    #[brw(align_before = 4)]
    pub x: f32,
    pub y: f32,
}

#[binrw]
#[derive(Debug)]
pub struct Vector3f {
    #[brw(align_before = 4)]
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[binrw]
#[derive(Debug)]
pub struct Vector4f {
    #[brw(align_before = 4)]
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[binrw]
#[derive(Debug)]
pub struct SpriteRenderData {
    pub texture: PPtr,
    pub alpha_texture: PPtr,
    pub secondary_textures: UArray<SecondarySpriteTexture>,
    pub sub_meshes: UArray<SubMesh>,
    pub index_buffer: UArray<u8>,
    pub vertex_data: VertexData,
    pub bind_pose: UArray<Matrix4x4f>,
    pub texture_rect: RectF,
    pub texture_rect_offset: Vector2f,
    pub atlas_rect_offset: Vector2f,
    pub settings_raw: u32,
    pub uv_transform: Vector4f,
    pub downscale_multiplier: f32,
}

#[binrw]
#[derive(Debug)]
pub struct SecondarySpriteTexture {
    pub texture: PPtr,
    pub name: UString,
}

#[binrw]
#[derive(Debug)]
pub struct SubMesh {
    #[brw(align_before = 4)]
    pub first_byte: u32,
    pub index_count: u32,
    pub topology: i32,
    pub base_vertex: u32,
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub aabb: AABB,
}

#[binrw]
#[derive(Debug)]
pub struct AABB {
    pub center: Vector3f,
    pub extent: Vector3f,
}

#[binrw]
#[derive(Debug)]
pub struct VertexData {
    #[brw(align_before = 4)]
    pub vertex_count: u32,
    pub channels: UArray<ChannelInfo>,
    pub data: UArray<u8>,
}

#[binrw]
#[derive(Debug)]
pub struct ChannelInfo {
    pub stream: u8,
    pub offset: u8,
    pub format: u8,
    pub dimension: u8,
}

#[binrw]
#[derive(Debug)]
pub struct Matrix4x4f {
    pub e00: f32,
    pub e01: f32,
    pub e02: f32,
    pub e03: f32,
    pub e10: f32,
    pub e11: f32,
    pub e12: f32,
    pub e13: f32,
    pub e20: f32,
    pub e21: f32,
    pub e22: f32,
    pub e23: f32,
    pub e30: f32,
    pub e31: f32,
    pub e32: f32,
    pub e33: f32,
}

#[binrw]
#[derive(Debug)]
pub struct SpriteBone {
    pub name: UString,
    pub position: Vector3f,
    pub rotation: Quaternionf,
    pub length: f32,
    pub parent_id: u32,
}

#[binrw]
#[derive(Debug)]
pub struct Quaternionf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[binrw]
#[derive(Debug)]
pub struct Mesh {
    pub name: UString,
    pub sub_meshes: UArray<SubMesh>,
    pub shapes: BlendShapeData,
    pub bind_pose: UArray<Matrix4x4f>,
    pub bone_name_hashes: UArray<u32>,
    pub root_bone_name_hash: u32,
    pub bones_aabb: UArray<MinMaxAABB>,
    pub variable_bone_count_weights: UArray<u32>,
    pub mesh_compression: u8,
    pub is_readable: u8,
    pub keep_vertices: u8,
    pub keep_indices: u8,
    pub index_format: u32,
    pub index_buffer: UArray<u8>,
    pub vertex_data: VertexData,
    #[brw(align_before = 4)]
    pub compressed_mesh: CompressedMesh,
    pub local_aabb: AABB,
    pub mesh_usage_flags: u32,
    pub baked_convex_collision_mesh: UArray<u8>,
    pub baked_triangle_collision_mesh: UArray<u8>,
    pub mesh_metrics_0: f32,
    pub mesh_metrics_1: f32,
    pub stream_data: StreamingInfo,
}

#[binrw]
#[derive(Debug)]
pub struct BlendShapeData {
    pub vertices: UArray<BlendShapeVertex>,
    pub shapes: UArray<MeshBlendShape>,
    pub channels: UArray<MeshBlendShapeChannel>,
    pub full_weights: UArray<f32>,
}

#[binrw]
#[derive(Debug)]
pub struct BlendShapeVertex {
    pub vertex: Vector3f,
    pub normal: Vector3f,
    pub tangent: Vector3f,
    pub index: u32,
}

#[binrw]
#[derive(Debug)]
pub struct MeshBlendShape {
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub has_normals: u8,
    pub has_tangents: u8,
}

#[binrw]
#[derive(Debug)]
pub struct MeshBlendShapeChannel {
    pub name: UString,
    pub name_hash: u32,
    pub frame_index: u32,
    pub frame_count: u32,
}

#[binrw]
#[derive(Debug)]
pub struct MinMaxAABB {
    min: Vector3f,
    max: Vector3f,
}

#[binrw]
#[derive(Debug)]
pub struct CompressedMesh {
    pub vertices: PackedBitVector,
    pub uv: PackedBitVector,
    pub normals: PackedBitVector,
    pub tangents: PackedBitVector,
    pub weights: PackedBitVector2,
    pub normal_signs: PackedBitVector2,
    pub tangent_signs: PackedBitVector2,
    pub float_colors: PackedBitVector,
    pub bone_indices: PackedBitVector2,
    pub triangles: PackedBitVector2,
    #[brw(align_before = 4)]
    pub uv_info: u32,
}

#[binrw]
#[derive(Debug)]
pub struct PackedBitVector {
    #[brw(align_before = 4)]
    pub num_items: u32,
    pub range: f32,
    pub start: f32,
    pub data: UArray<u8>,
    pub bit_size: u8,
}

#[binrw]
#[derive(Debug)]
pub struct PackedBitVector2 {
    #[brw(align_before = 4)]
    pub num_items: u32,
    pub data: UArray<u8>,
    pub bit_size: u8,
}

#[binrw]
#[derive(Debug)]
pub struct Avatar {
    pub name: UString,
    pub avatar_size: u32,
    pub avatar: AvatarConstant,
    pub tos: UArray<TosPair>,
    pub human_description: HumanDescription,
}

#[binrw]
#[derive(Debug)]
pub struct TosPair {
    #[brw(align_before = 4)]
    pub first: u32,
    pub second: UString,
}

#[binrw]
#[derive(Debug)]
pub struct AvatarConstant {
    pub skeleton: Skeleton,
    pub avatar_skeleton_pose: SkeletonPose,
    pub default_pose: SkeletonPose,
    pub skeleton_name_id_array: UArray<u32>,
    pub human: AvatarHuman,
    pub human_skeleton_index_array: UArray<u32>,
    pub human_skeleton_reverse_index_array: UArray<u32>,
    pub root_motion_bone_index: u32,
    pub root_motion_bone_x: SkeletonTransform,
    pub root_motion_skeleton: Skeleton,
    pub root_motion_skeleton_pose: SkeletonPose,
    pub root_motion_skeleton_index_array: UArray<u32>,
}

#[binrw]
#[derive(Debug)]
pub struct Skeleton {
    pub node: UArray<SkeletonNode>,
    pub id: UArray<u32>,
    pub axes: UArray<SkeletonAxes>,
}

#[binrw]
#[derive(Debug)]
pub struct SkeletonNode {
    pub parent_id: u32,
    pub axes_id: u32,
}

#[binrw]
#[derive(Debug)]
pub struct SkeletonAxes {
    pub pre_q: Vector4f,
    pub post_q: Vector4f,
    pub sgn: Vector3f,
    pub limit: SkeletonLimit,
    pub length: f32,
    pub ty: u32,
}

#[binrw]
#[derive(Debug)]
pub struct SkeletonLimit {
    pub min: Vector3f,
    pub max: Vector3f,
}

#[binrw]
#[derive(Debug)]
pub struct SkeletonPose {
    pub transform: UArray<SkeletonTransform>,
}

#[binrw]
#[derive(Debug)]
pub struct SkeletonTransform {
    pub transform: Vector3f,
    pub quaternion: Quaternionf,
    pub scale: Vector3f,
}

#[binrw]
#[derive(Debug)]
pub struct AvatarHuman {
    pub root_x: SkeletonTransform,
    pub skeleton: Skeleton,
    pub skeleton_pose: SkeletonPose,
    pub left_hand: UArray<u32>,
    pub right_hand: UArray<u32>,
    pub human_bone_index: UArray<u32>,
    pub human_bone_mass: UArray<f32>,
    pub scale: f32,
    pub arm_twist: f32,
    pub forearm_twist: f32,
    pub upper_left_twist: f32,
    pub leg_twist: f32,
    pub arm_stretch: f32,
    pub leg_stretch: f32,
    pub feet_spacing: f32,
    pub has_left_hand: u8,
    pub has_right_hand: u8,
    pub has_tdof: u8,
}

#[binrw]
#[derive(Debug)]
pub struct HumanDescription {
    pub human: UArray<HumanBone>,
    pub skeleton: UArray<SkeletonBone>,
    pub arm_twist: f32,
    pub forearm_twist: f32,
    pub upper_leg_twist: f32,
    pub leg_twist: f32,
    pub arm_stretch: f32,
    pub leg_stretch: f32,
    pub feet_spacing: f32,
    pub global_scale: f32,
    pub root_motion_bone_name: UString,
    pub has_translation_dof: u8,
    pub has_extra_root: u8,
    pub skeleton_has_parents: u8,
}

#[binrw]
#[derive(Debug)]
pub struct HumanBone {
    pub bone_name: UString,
    pub human_name: UString,
    pub limit: SkeletonBoneLimit,
}

#[binrw]
#[derive(Debug)]
pub struct SkeletonBoneLimit {
    pub min: Vector3f,
    pub max: Vector3f,
    pub value: Vector3f,
    pub length: f32,
    pub modified: u8,
}

#[binrw]
#[derive(Debug)]
pub struct SkeletonBone {
    pub name: UString,
    pub parent_name: UString,
    pub position: Vector3f,
    pub rotation: Vector3f,
    pub scale: Vector3f,
}

#[binrw]
#[derive(Debug)]
pub struct Material {
    pub name: UString,
    pub shader: PPtr,
    pub shader_keywords: UString,
    #[brw(align_before = 4)]
    pub lightmap_flags: u32,
    pub enable_instancing_variants: u8,
    pub double_sided_gi: u8,
    #[brw(align_before = 4)]
    pub custom_render_queue: u32,
    pub string_tag_map: UArray<(UString, UString)>,
    pub disabled_shader_passes: UArray<UString>,
    pub saved_properties: UnityPropertySheet,
    pub build_texture_stacks: UArray<(UString, UString)>,
}

#[binrw]
#[derive(Debug)]
pub struct UnityPropertySheet {
    pub text_envs: UArray<(UString, TexEnv)>,
    pub floats: UArray<FloatPropertySheetPair>,
    pub colors: UArray<(UString, ColorRGBA)>,
}

#[binrw]
#[derive(Debug)]
pub struct TexEnv {
    #[brw(align_before = 4)]
    pub texture: PPtr,
    pub scale: Vector2f,
    pub offset: Vector2f,
}

#[binrw]
#[derive(Debug)]
pub struct FloatPropertySheetPair {
    pub key: UString,
    #[brw(align_before = 4)]
    pub value: f32,
}

#[binrw]
#[derive(Debug)]
pub struct ColorRGBA {
    #[brw(align_before = 4)]
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[binrw]
#[derive(Debug)]
pub struct SkinnedMeshRenderer {
    pub game_object: PPtr,
    pub enabled: u8,
    pub cast_shadows: u8,
    pub receive_shadows: u8,
    pub dynamic_occludee: u8,
    pub motion_vectors: u8,
    pub light_probe_usage: u8,
    pub reflection_probe_usage: u8,
    pub ray_tracing_mode: u8,
    pub ray_trace_procedural: u8,
    #[brw(align_before = 4)]
    pub rendering_layer_mask: u32,
    pub renderer_priority: u32,
    pub lightmap_index: u16,
    pub lightmap_index_dynamic: u16,
    pub lightmap_tiling_offset: Vector4f,
    pub lightmap_tiling_offset_dynamic: Vector4f,
    pub materials: UArray<PPtr>,
    pub first_sub_mesh: u16,
    pub sub_mesh_count: u16,
    pub static_batch_root: PPtr,
    pub probe_anchor: PPtr,
    pub light_probe_volume_override: PPtr,
    pub sorting_layer_id: u32,
    pub sorting_layer: u16,
    pub sorting_order: u16,
    pub quality: u32,
    pub update_when_offscreen: u8,
    pub skinned_motion_vectors: u8,
    #[brw(align_before = 4)]
    pub mesh: PPtr,
    pub bones: UArray<PPtr>,
    pub blend_shape_weights: UArray<f32>,
    pub root_bone: PPtr,
    pub aabb: AABB,
    pub dirty_aabb: u8,
}

#[binrw]
#[derive(Debug)]
pub struct SpringJob {
    pub optimize_transform: u32,
    pub is_paused: u32,
    pub simulation_frame_rate: u32,
    pub dynamic_ratio: f32,
    pub gravity: Vector3f,
    pub bounce: f32,
    pub friction: f32,
    pub time: f32,
    pub enable_angle_limits: u32,
    pub enable_collision: u32,
    pub enable_length_limtis: u32,
    pub collide_width_ground: u32,
    pub ground_height: f32,
    pub wind_disabled: u32,
    pub wind_influence: f32,
    pub wind_power: Vector3f,
    pub wind_dir: Vector3f,
    pub distance_rate: Vector3f,
    pub automatic_reset: u32,
    pub reset_distance: f32,
    pub reset_angle: f32,
    pub sorted_bones: UArray<PPtr>,
    pub job_colliders: UArray<PPtr>,
    pub job_properties: UArray<SpringBoneProperties>,
    pub init_local_rotations: UArray<Quaternionf>,
    pub job_col_properties: UArray<SpringColliderProperty>,
    pub job_length_properties: UArray<LengthLimitProperty>,
}

#[binrw]
#[derive(Debug)]
pub struct SpringBoneProperties {
    pub stiffness_force: f32,
    pub drag_force: f32,
    pub spring_force: Vector3f,
    pub wind_influence: f32,
    pub angular_stiffness: f32,
    pub y_angle_limits: AngleLimits,
    pub z_angle_limits: AngleLimits,
    pub radius: f32,
    pub spring_length: f32,
    pub bone_axis: Vector3f,
    pub local_position: Vector3f,
    pub initial_local_rotation: Quaternionf,
    pub parent_index: u32,
    pub pivot_index: u32,
    pub pivot_local_matrix: Matrix4x4f,
}

#[binrw]
#[derive(Debug)]
pub struct AngleLimits {
    pub active: u8,
    #[brw(align_before = 4)]
    pub min: f32,
    pub max: f32,
}

#[binrw]
#[derive(Debug)]
pub struct SpringColliderProperty {
    pub ty: u32,
    pub radius: f32,
    pub width: f32,
    pub height: f32,
}

#[binrw]
#[derive(Debug)]
pub struct LengthLimitProperty {
    pub target_index: u32,
    pub target: f32,
}

#[binrw]
#[derive(Debug)]
pub struct SpringBone {
    pub index: u32,
    pub enabled_job_system: u8,
    pub job_colliders: UArray<PPtr>,
    pub valid_children: UArray<PPtr>,
    pub stiffness_force: f32,
    pub drag_force: f32,
    pub spring_force: Vector3f,
    pub wind_influence: f32,
    pub pivot_node: PPtr,
    pub angular_stiffness: f32,
    pub y_angle_limits: AngleLimits,
    pub z_angle_limits: AngleLimits,
    pub length_limit_targets: UArray<PPtr>,
    pub radius: f32,
    pub sphere_colliders: UArray<PPtr>,
    pub capsule_colliders: UArray<PPtr>,
    pub panel_colliders: UArray<PPtr>,
}
