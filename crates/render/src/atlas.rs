use elma_formats::pic::Pic8;

// Where an image lives in the atlas: layer, x, y, width | height << 16
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AtlasRect {
    pub layer: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl AtlasRect {
    pub fn packed(&self) -> [u32; 4] {
        [self.layer, self.x, self.y, self.width | (self.height << 16)]
    }
}

struct Shelf {
    y: usize,
    height: usize,
    x: usize,
}

struct Layer {
    shelves: Vec<Shelf>,
    used_height: usize,
}

// Packs every palette-indexed image into the layers of one texture array
pub struct AtlasBuilder {
    size: usize,
    layers: Vec<Layer>,
    pixels: Vec<Vec<u8>>,
}

impl AtlasBuilder {
    pub fn new(size: usize) -> AtlasBuilder {
        AtlasBuilder {
            size,
            layers: Vec::new(),
            pixels: Vec::new(),
        }
    }

    pub fn layer_size(&self) -> usize {
        self.size
    }

    pub fn add(&mut self, pic: &Pic8) -> AtlasRect {
        let (w, h) = (pic.width.min(self.size), pic.height.min(self.size));
        let rect = self.allocate(w, h);
        let layer = &mut self.pixels[rect.layer as usize];
        for y in 0..h {
            let dst = (rect.y as usize + y) * self.size + rect.x as usize;
            layer[dst..dst + w].copy_from_slice(&pic.row(y)[..w]);
        }
        rect
    }

    fn allocate(&mut self, w: usize, h: usize) -> AtlasRect {
        // One texel of padding keeps neighbouring images apart
        let (pw, ph) = ((w + 1).min(self.size), (h + 1).min(self.size));
        for (index, layer) in self.layers.iter_mut().enumerate() {
            for shelf in &mut layer.shelves {
                if ph <= shelf.height && shelf.x + pw <= self.size {
                    let rect = AtlasRect {
                        layer: index as u32,
                        x: shelf.x as u32,
                        y: shelf.y as u32,
                        width: w as u32,
                        height: h as u32,
                    };
                    shelf.x += pw;
                    return rect;
                }
            }
            if layer.used_height + ph <= self.size {
                let y = layer.used_height;
                layer.shelves.push(Shelf {
                    y,
                    height: ph,
                    x: pw,
                });
                layer.used_height += ph;
                return AtlasRect {
                    layer: index as u32,
                    x: 0,
                    y: y as u32,
                    width: w as u32,
                    height: h as u32,
                };
            }
        }
        self.layers.push(Layer {
            shelves: vec![Shelf {
                y: 0,
                height: ph,
                x: pw,
            }],
            used_height: ph,
        });
        self.pixels.push(vec![0; self.size * self.size]);
        AtlasRect {
            layer: self.layers.len() as u32 - 1,
            x: 0,
            y: 0,
            width: w as u32,
            height: h as u32,
        }
    }

    pub fn into_layers(self) -> Vec<Vec<u8>> {
        if self.pixels.is_empty() {
            return vec![vec![0; self.size * self.size]];
        }
        self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_without_overlap() {
        let mut atlas = AtlasBuilder::new(64);
        let mut rects = Vec::new();
        for i in 0..40 {
            rects.push(atlas.add(&Pic8::filled(10 + i % 7, 5 + i % 11, i as u8)));
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let overlap = a.layer == b.layer
                    && a.x < b.x + b.width
                    && b.x < a.x + a.width
                    && a.y < b.y + b.height
                    && b.y < a.y + a.height;
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
        let layers = atlas.into_layers();
        let r = rects[3];
        assert_eq!(
            layers[r.layer as usize][(r.y as usize) * 64 + r.x as usize],
            3
        );
    }
}
