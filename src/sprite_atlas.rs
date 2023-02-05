use std::collections::HashMap;

use image::DynamicImage;

use crate::{RenderDataKey, Sprite, SpriteAtlas, SpriteAtlasData};

pub struct SpriteAtlasWrapper {
    pub texture: DynamicImage,
    render_data: HashMap<RenderDataKey, SpriteAtlasData>,
    sprites: HashMap<String, Sprite>,
}

impl SpriteAtlasWrapper {
    pub fn new(texture: DynamicImage, atlas: SpriteAtlas, sprites: Vec<Sprite>) -> Self {
        // TODO: Validate that everything uses the supported packing flags
        Self {
            texture,
            render_data: atlas.render_data_map.items.into_iter().collect(),
            sprites: sprites
                .into_iter()
                .map(|sprite| (sprite.name.0.clone(), sprite))
                .collect(),
        }
    }

    pub fn get_sprite(&self, name: &str) -> Option<DynamicImage> {
        let sprite = self.sprites.get(name)?;
        let render_data = self.render_data.get(&sprite.render_data_key)?;
        let rect = &render_data.texture_rect;
        Some(
            self.texture
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
