use elma_formats::pic::Pic8;

// Where an image lives in the atlas
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AtlasRect {
    pub layer: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl AtlasRect {
    // As the shader gets it, with the layer apart. Every value stays under 16 bits: some
    // phones' GPUs pass integers between shader stages with only 16 (Adreno, in Firefox).
    pub fn packed(&self) -> [u32; 4] {
        [self.x, self.y, self.width, self.height]
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

// Where the images of one texture array's layers are, and where there is room for more
#[derive(Default)]
pub struct Packer {
    size: usize,
    layers: Vec<Layer>,
}

// Packs every palette-indexed image into the layers of one texture array
pub struct AtlasBuilder {
    packer: Packer,
    pixels: Vec<Vec<u8>>,
}

impl AtlasBuilder {
    pub fn new(size: usize) -> AtlasBuilder {
        AtlasBuilder {
            packer: Packer {
                size,
                layers: Vec::new(),
            },
            pixels: Vec::new(),
        }
    }

    pub fn layer_size(&self) -> usize {
        self.packer.size
    }

    pub fn add(&mut self, pic: &Pic8) -> AtlasRect {
        let size = self.packer.size;
        let (w, h) = (pic.width.min(size), pic.height.min(size));
        let rect = self.packer.allocate(w, h);
        if self.pixels.len() < self.packer.layers.len() {
            self.pixels.push(vec![0; size * size]);
        }
        let layer = &mut self.pixels[rect.layer as usize];
        for y in 0..h {
            let dst = (rect.y as usize + y) * size + rect.x as usize;
            layer[dst..dst + w].copy_from_slice(&pic.row(y)[..w]);
        }
        rect
    }

    // The layers' pixels, and the packer to find room in them for more images
    pub fn finish(mut self) -> (Vec<Vec<u8>>, Packer) {
        if self.pixels.is_empty() {
            let size = self.packer.size;
            self.pixels.push(vec![0; size * size]);
        }
        self.packer.add_layers(self.pixels.len());
        (self.pixels, self.packer)
    }

    pub fn into_layers(self) -> Vec<Vec<u8>> {
        self.finish().0
    }
}

impl Packer {
    // Empty layers up to `count`, as the texture has
    pub fn add_layers(&mut self, count: usize) {
        while self.layers.len() < count {
            self.layers.push(Layer {
                shelves: Vec::new(),
                used_height: 0,
            });
        }
    }

    // Room for a `w` by `h` image in the layers there are, if any is left
    pub fn find_room(&mut self, w: usize, h: usize) -> Option<AtlasRect> {
        if w >= self.size || h >= self.size {
            return None;
        }
        self.fit(w, h)
    }

    // Room for a `w` by `h` image in the layers there are, or else in a new one
    fn allocate(&mut self, w: usize, h: usize) -> AtlasRect {
        if let Some(rect) = self.fit(w, h) {
            return rect;
        }
        let (pw, ph) = ((w + 1).min(self.size), (h + 1).min(self.size));
        self.layers.push(Layer {
            shelves: vec![Shelf {
                y: 0,
                height: ph,
                x: pw,
            }],
            used_height: ph,
        });
        AtlasRect {
            layer: self.layers.len() as u32 - 1,
            x: 0,
            y: 0,
            width: w as u32,
            height: h as u32,
        }
    }

    fn fit(&mut self, w: usize, h: usize) -> Option<AtlasRect> {
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
                    return Some(rect);
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
                return Some(AtlasRect {
                    layer: index as u32,
                    x: 0,
                    y: y as u32,
                    width: w as u32,
                    height: h as u32,
                });
            }
        }
        None
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
