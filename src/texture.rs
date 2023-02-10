use anyhow::{anyhow, bail, Result};
use astc_decode::Footprint;
use image::{DynamicImage, GrayImage, RgbaImage};
use tegra_swizzle::BlockHeight;

use crate::{Texture2D, TextureFormat};

// Borrowing heavily from https://github.com/gameltb/io_unity
pub(crate) fn decode(texture: &Texture2D, image_data: &[u8]) -> Result<DynamicImage> {
    let width = texture.width as usize;
    let height = texture.height as usize;
    let size = width * height * 4;

    let (block_width, block_height, bytes_per_pixel) = match texture.texture_format {
        TextureFormat::ASTC_RGB_4x4 => (4, 4, 16),
        TextureFormat::ASTC_RGB_5x5 => (5, 5, 16),
        TextureFormat::R8 => (1, 1, 1),
        _ => bail!("unsupported texture format '{:?}'", texture.texture_format),
    };

    let input = tegra_swizzle::swizzle::deswizzle_block_linear(
        tegra_swizzle::div_round_up(width, block_width),
        tegra_swizzle::div_round_up(height, block_height),
        1,
        &image_data,
        BlockHeight::Sixteen,
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
