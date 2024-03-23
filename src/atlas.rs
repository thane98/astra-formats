use std::collections::HashMap;
use std::path::Path;

use crate::{
    Asset, AssetFile, Bundle, BundleFile, RenderDataKey, Sprite, SpriteAtlas, SpriteAtlasData,
    Texture2D, TextureFormat,
};
use anyhow::{anyhow, bail, Result};
use astc_decode::Footprint;
use image::{DynamicImage, GrayImage, RgbaImage};

pub struct SpriteAtlasWrapper {
    pub textures: HashMap<i64, DynamicImage>,
    render_data: HashMap<RenderDataKey, SpriteAtlasData>,
    sprites: HashMap<String, Sprite>,
}

impl SpriteAtlasWrapper {
    pub fn new(
        textures: HashMap<i64, DynamicImage>,
        atlas: SpriteAtlas,
        sprites: Vec<Sprite>,
    ) -> Self {
        // TODO: Validate that everything uses the supported packing flags
        Self {
            textures,
            render_data: atlas.render_data_map.items.into_iter().collect(),
            sprites: sprites
                .into_iter()
                .map(|sprite| (sprite.name.0.clone(), sprite))
                .collect(),
        }
    }

    pub fn unwrap_sprites(&self) -> HashMap<String, DynamicImage> {
        self.sprites
            .keys()
            .filter_map(|key| self.get_sprite(key).map(|sprite| (key.to_owned(), sprite)))
            .collect()
    }

    pub fn get_sprite(&self, name: &str) -> Option<DynamicImage> {
        let sprite = self.sprites.get(name)?;
        let render_data = self.render_data.get(&sprite.render_data_key)?;
        let texture = self.textures.get(&render_data.texture.path_id)?;
        let rect = &render_data.texture_rect;
        Some(
            texture
                .crop_imm(
                    rect.x as u32,
                    rect.y as u32,
                    rect.w.ceil() as u32,
                    rect.h.ceil() as u32,
                )
                .flipv(),
        )
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
            let assets = extract_atlas_assets(asset_file)?;
            let mut textures = HashMap::new();
            let mut slice_start = 0;
            for (id, texture) in assets.textures {
                textures.insert(id as i64, decode(&texture, &image_data[slice_start..])?);
                slice_start += texture.width as usize * texture.height as usize;
            }
            Ok(SpriteAtlasWrapper::new(
                textures,
                assets.atlas,
                assets.sprites,
            ))
        } else {
            bail!("could not identify asset and texture files in bundle")
        }
    }
}

struct AtlasAssets {
    textures: Vec<(u64, Texture2D)>,
    sprites: Vec<Sprite>,
    atlas: SpriteAtlas,
}

fn extract_atlas_assets(asset_file: AssetFile) -> Result<AtlasAssets> {
    let mut sprites = vec![];
    let mut textures = vec![];
    let mut atlas = None;
    for asset in asset_file.assets {
        match asset {
            Asset::Texture2D(asset, id) => textures.push((id, asset)),
            Asset::SpriteAtlas(asset) => atlas = Some(asset),
            Asset::Sprite(asset) => sprites.push(asset),
            _ => {}
        }
    }
    if let Some(atlas) = atlas {
        Ok(AtlasAssets {
            textures,
            sprites,
            atlas,
        })
    } else {
        bail!("could not extract assets required to build sprite atlas")
    }
}

fn decode(texture: &Texture2D, image_data: &[u8]) -> Result<DynamicImage> {
    let width = texture.width as usize;
    let height = texture.height as usize;
    let size = width * height * 4;

    let (block_width, block_height, bytes_per_pixel) = match texture.texture_format {
        TextureFormat::ASTC_RGB_4x4 => (4, 4, 16),
        TextureFormat::ASTC_RGB_5x5 => (5, 5, 16),
        TextureFormat::R8 => (1, 1, 1),
        _ => bail!("unsupported texture format '{:?}'", texture.texture_format),
    };

    let block_height_mip0 = tegra_swizzle::block_height_mip0(tegra_swizzle::div_round_up(height, block_height));

    let input = tegra_swizzle::swizzle::deswizzle_block_linear(
        tegra_swizzle::div_round_up(width, block_width),
        tegra_swizzle::div_round_up(height, block_height),
        1,
        image_data,
        block_height_mip0,
        bytes_per_pixel,
    )?;

    match texture.texture_format {
        TextureFormat::ASTC_RGB_4x4 | TextureFormat::ASTC_RGB_5x5 => {
            let mut output = vec![[0u8; 4]; size];
            astc_decode::astc_decode(
                input.as_slice(),
                texture.width,
                texture.height,
                Footprint::new(block_width as u32, block_height as u32),
                |x, y, color| {
                    output[x as usize + y as usize * width] = color;
                },
            )?;
            RgbaImage::from_raw(width as u32, height as u32, output.concat())
                .ok_or_else(|| anyhow!("failed to build image"))
                .map(DynamicImage::ImageRgba8)
        }
        // This technically isn't correct, but the alternative is storing in a *much* larger texture.
        TextureFormat::R8 => GrayImage::from_raw(width as u32, height as u32, input)
            .ok_or_else(|| anyhow!("failed to build image"))
            .map(DynamicImage::ImageLuma8),
        _ => bail!("unsupported texture format '{:?}'", texture.texture_format),
    }
}
