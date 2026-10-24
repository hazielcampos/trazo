//! Sparse tiled raster images.
//!
//! Every cel (one layer in one frame) is a [`TiledImage`]. Pixels are stored as
//! premultiplied RGBA8 in 64×64 tiles. Empty tiles are not allocated, which keeps
//! memory low for animations with hundreds of mostly-transparent frames
//! (important on phones). Tiles live behind `Arc`, so duplicating a frame or
//! keeping an undo snapshot is cheap: data is only copied when it is modified.
//!
//! Tiles that aren't being looked at can be *packed* (LZ4-compressed in
//! memory; drawings are mostly transparency and flat color, so they shrink
//! 10–50×). Reading a packed tile decodes it on the fly; painting on it opens
//! it again. Packing happens in the background (see `crate::memory`).

use std::ops::Deref;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use crate::geom::IRect;

pub const TILE: usize = 64;
pub const TILE_PX: usize = TILE * TILE;
pub const TILE_BYTES: usize = TILE_PX * 4;

enum TileData {
    Raw(Box<[u8; TILE_BYTES]>),
    /// LZ4 with the size in front.
    Packed(Box<[u8]>),
}

/// One 64×64 block of premultiplied RGBA8 pixels, open or packed.
pub struct Tile {
    data: RwLock<TileData>,
}

/// The pixels of a tile, borrowed (open tile) or decoded (packed tile).
pub struct Px<'a>(PxInner<'a>);

enum PxInner<'a> {
    Open(RwLockReadGuard<'a, TileData>),
    Decoded(Box<[u8; TILE_BYTES]>),
}

impl Deref for Px<'_> {
    type Target = [u8; TILE_BYTES];
    fn deref(&self) -> &Self::Target {
        match &self.0 {
            PxInner::Open(g) => match &**g {
                TileData::Raw(b) => b,
                TileData::Packed(_) => unreachable!("Px::Open is only made for open tiles"),
            },
            PxInner::Decoded(b) => b,
        }
    }
}

fn unpack(packed: &[u8]) -> Box<[u8; TILE_BYTES]> {
    lz4_flex::decompress_size_prepended(packed)
        .ok()
        .and_then(|v| v.into_boxed_slice().try_into().ok())
        // Can't happen (we packed it ourselves); a blank tile beats a crash.
        .unwrap_or_else(|| Box::new([0u8; TILE_BYTES]))
}

impl Tile {
    pub fn new() -> Self {
        Self { data: RwLock::new(TileData::Raw(Box::new([0u8; TILE_BYTES]))) }
    }

    fn read(&self) -> RwLockReadGuard<'_, TileData> {
        self.data.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Pixels without changing how the tile is kept (a packed tile is decoded
    /// into a temporary copy). For one-off reads: saving, exporting, playing.
    pub fn px_cold(&self) -> Px<'_> {
        let g = self.read();
        match &*g {
            TileData::Raw(_) => Px(PxInner::Open(g)),
            TileData::Packed(p) => Px(PxInner::Decoded(unpack(p))),
        }
    }

    /// Pixels, opening a packed tile (it is being worked on).
    pub fn px(&self) -> Px<'_> {
        for _ in 0..2 {
            {
                let g = self.read();
                if matches!(&*g, TileData::Raw(_)) {
                    return Px(PxInner::Open(g));
                }
            }
            // Never wait for the lock (no deadlock with a reader on this thread).
            match self.data.try_write() {
                Ok(mut w) => {
                    if let TileData::Packed(p) = &*w {
                        *w = TileData::Raw(unpack(p));
                    }
                }
                Err(_) => break,
            }
        }
        self.px_cold()
    }

    /// Writable pixels (the tile is opened if packed).
    pub fn px_mut(&mut self) -> &mut [u8; TILE_BYTES] {
        let d = self.data.get_mut().unwrap_or_else(|e| e.into_inner());
        if let TileData::Packed(p) = d {
            *d = TileData::Raw(unpack(p));
        }
        match d {
            TileData::Raw(b) => b,
            TileData::Packed(_) => unreachable!("opened above"),
        }
    }

    /// Compress the tile in memory. Returns the bytes saved (0 if it was
    /// already packed, busy, or doesn't compress).
    pub fn pack(&self) -> usize {
        let packed = {
            let g = self.read();
            match &*g {
                TileData::Raw(b) => lz4_flex::compress_prepend_size(&b[..]),
                TileData::Packed(_) => return 0,
            }
        };
        if packed.len() > TILE_BYTES * 3 / 4 {
            return 0; // noise-like pixels: not worth it
        }
        match self.data.try_write() {
            Ok(mut w) if matches!(&*w, TileData::Raw(_)) => {
                let saved = TILE_BYTES - packed.len();
                *w = TileData::Packed(packed.into_boxed_slice());
                saved
            }
            _ => 0,
        }
    }

    pub fn is_packed(&self) -> bool {
        matches!(&*self.read(), TileData::Packed(_))
    }

    /// Memory the pixels take right now.
    pub fn heap_bytes(&self) -> usize {
        match &*self.read() {
            TileData::Raw(_) => TILE_BYTES,
            TileData::Packed(p) => p.len(),
        }
    }

    pub fn is_transparent(&self) -> bool {
        self.px_cold().chunks_exact(4).all(|p| p[3] == 0)
    }
}

impl Clone for Tile {
    fn clone(&self) -> Self {
        let data = match &*self.read() {
            TileData::Raw(b) => TileData::Raw(b.clone()),
            TileData::Packed(p) => TileData::Packed(p.clone()),
        };
        Self { data: RwLock::new(data) }
    }
}

impl Default for Tile {
    fn default() -> Self {
        Self::new()
    }
}

pub type TileRef = Option<Arc<Tile>>;

#[derive(Clone)]
pub struct TiledImage {
    width: u32,
    height: u32,
    tiles_x: usize,
    tiles_y: usize,
    tiles: Vec<TileRef>,
}

impl std::fmt::Debug for TiledImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TiledImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("allocated_tiles", &self.allocated_tiles())
            .finish()
    }
}

impl TiledImage {
    pub fn new(width: u32, height: u32) -> Self {
        let tiles_x = (width as usize).div_ceil(TILE);
        let tiles_y = (height as usize).div_ceil(TILE);
        Self { width, height, tiles_x, tiles_y, tiles: vec![None; tiles_x * tiles_y] }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bounds(&self) -> IRect {
        IRect::from_size(self.width, self.height)
    }

    pub fn tiles_x(&self) -> usize {
        self.tiles_x
    }

    pub fn tiles_y(&self) -> usize {
        self.tiles_y
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn allocated_tiles(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    /// Cheap value that changes whenever the pixels change (tiles are
    /// copy-on-write, so an edited tile is a new allocation).
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (i, t) in self.tiles.iter().enumerate() {
            if let Some(t) = t {
                (i, Arc::as_ptr(t) as usize).hash(&mut h);
            }
        }
        h.finish()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.iter().all(|t| t.is_none())
    }

    /// The allocated tiles (to pack them, or measure memory).
    pub fn tiles(&self) -> impl Iterator<Item = &Arc<Tile>> + '_ {
        self.tiles.iter().flatten()
    }

    /// Pack every tile now (e.g. right after loading). Returns bytes saved.
    pub fn pack_all(&self) -> usize {
        self.tiles().map(|t| t.pack()).sum()
    }

    pub fn tile_index(&self, tx: usize, ty: usize) -> usize {
        ty * self.tiles_x + tx
    }

    pub fn tile_ref(&self, idx: usize) -> &TileRef {
        &self.tiles[idx]
    }

    /// Replace a whole tile (used by undo/redo).
    pub fn set_tile_ref(&mut self, idx: usize, tile: TileRef) {
        self.tiles[idx] = tile;
    }

    /// Mutable access to a tile, allocating it and un-sharing it if needed.
    pub fn tile_mut(&mut self, idx: usize) -> &mut Tile {
        let slot = &mut self.tiles[idx];
        if slot.is_none() {
            *slot = Some(Arc::new(Tile::new()));
        }
        Arc::make_mut(slot.as_mut().expect("tile allocated above"))
    }

    /// Indices of all tiles overlapping `rect` (clipped to the image).
    pub fn tiles_in_rect(&self, rect: &IRect) -> impl Iterator<Item = (usize, usize)> + '_ {
        let r = rect.intersect(&self.bounds());
        let (tx0, ty0, tx1, ty1) = if r.is_empty() {
            (0, 0, 0, 0)
        } else {
            (
                r.x0 as usize / TILE,
                r.y0 as usize / TILE,
                (r.x1 as usize - 1) / TILE + 1,
                (r.y1 as usize - 1) / TILE + 1,
            )
        };
        (ty0..ty1).flat_map(move |ty| (tx0..tx1).map(move |tx| (tx, ty)))
    }

    /// Premultiplied pixel, transparent if outside or unallocated.
    pub fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return [0; 4];
        }
        let (x, y) = (x as usize, y as usize);
        match &self.tiles[self.tile_index(x / TILE, y / TILE)] {
            None => [0; 4],
            Some(t) => {
                let o = ((y % TILE) * TILE + (x % TILE)) * 4;
                let px = t.px();
                [px[o], px[o + 1], px[o + 2], px[o + 3]]
            }
        }
    }

    /// Like [`TiledImage::pixel`] but leaves packed tiles packed, decoding
    /// each tile once (`cache` is kept by the caller between calls). For
    /// sampling many pixels (thumbnails).
    pub fn pixel_cached<'a>(&'a self, cache: &mut std::collections::HashMap<usize, Px<'a>>, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return [0; 4];
        }
        let (x, y) = (x as usize, y as usize);
        let idx = self.tile_index(x / TILE, y / TILE);
        let Some(t) = &self.tiles[idx] else { return [0; 4] };
        let px = cache.entry(idx).or_insert_with(|| t.px_cold());
        let o = ((y % TILE) * TILE + (x % TILE)) * 4;
        [px[o], px[o + 1], px[o + 2], px[o + 3]]
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, p: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        let idx = self.tile_index(x / TILE, y / TILE);
        if p[3] == 0 && self.tiles[idx].is_none() {
            return;
        }
        let t = self.tile_mut(idx).px_mut();
        let o = ((y % TILE) * TILE + (x % TILE)) * 4;
        t[o..o + 4].copy_from_slice(&p);
    }

    /// Free tiles that became fully transparent (e.g. after erasing).
    pub fn prune(&mut self) {
        for slot in &mut self.tiles {
            if slot.as_ref().is_some_and(|t| t.is_transparent()) {
                *slot = None;
            }
        }
    }

    /// Tight bounding box of non-transparent pixels.
    pub fn content_bounds(&self) -> Option<IRect> {
        let mut out = IRect::EMPTY;
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let Some(t) = &self.tiles[self.tile_index(tx, ty)] else { continue };
                let t = t.px_cold();
                for py in 0..TILE {
                    for px in 0..TILE {
                        if t[(py * TILE + px) * 4 + 3] != 0 {
                            let x = (tx * TILE + px) as i32;
                            let y = (ty * TILE + py) as i32;
                            out = out.union(&IRect::new(x, y, x + 1, y + 1));
                        }
                    }
                }
            }
        }
        (!out.is_empty()).then_some(out)
    }

    /// Copy a region out as a tightly packed premultiplied RGBA buffer.
    pub fn read_region(&self, rect: &IRect) -> Vec<u8> {
        let (w, h) = (rect.width() as usize, rect.height() as usize);
        let mut out = vec![0u8; w * h * 4];
        let clip = rect.intersect(&self.bounds());
        for (tx, ty) in self.tiles_in_rect(&clip).collect::<Vec<_>>() {
            let Some(t) = &self.tiles[self.tile_index(tx, ty)] else { continue };
            let t = t.px_cold();
            let r = clip.intersect(&tile_rect(tx, ty));
            let n = r.width() as usize * 4;
            for y in r.y0..r.y1 {
                let src = ((y as usize % TILE) * TILE + r.x0 as usize % TILE) * 4;
                let dst = ((y - rect.y0) as usize * w + (r.x0 - rect.x0) as usize) * 4;
                out[dst..dst + n].copy_from_slice(&t[src..src + n]);
            }
        }
        out
    }

    /// Write a premultiplied RGBA buffer at `(x0, y0)`. Fully transparent
    /// tiles are not allocated.
    pub fn write_region(&mut self, x0: i32, y0: i32, w: usize, h: usize, data: &[u8]) {
        let rect = IRect::new(x0, y0, x0 + w as i32, y0 + h as i32);
        let clip = rect.intersect(&self.bounds());
        for (tx, ty) in self.tiles_in_rect(&clip).collect::<Vec<_>>() {
            let r = clip.intersect(&tile_rect(tx, ty));
            let n = r.width() as usize * 4;
            let row = |y: i32| ((y - y0) as usize * w + (r.x0 - x0) as usize) * 4;
            let idx = self.tile_index(tx, ty);
            let any = (r.y0..r.y1).any(|y| data[row(y)..row(y) + n].chunks_exact(4).any(|p| p[3] != 0));
            if !any && self.tiles[idx].is_none() {
                continue;
            }
            let t = self.tile_mut(idx).px_mut();
            for y in r.y0..r.y1 {
                let dst = ((y as usize % TILE) * TILE + r.x0 as usize % TILE) * 4;
                t[dst..dst + n].copy_from_slice(&data[row(y)..row(y) + n]);
            }
        }
    }
}

/// Canvas-space rectangle covered by tile `(tx, ty)`.
pub fn tile_rect(tx: usize, ty: usize) -> IRect {
    let t = TILE as i32;
    IRect::new(tx as i32 * t, ty as i32 * t, tx as i32 * t + t, ty as i32 * t + t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_until_written() {
        let mut img = TiledImage::new(200, 100);
        assert_eq!(img.tile_count(), 4 * 2);
        assert!(img.is_empty());
        img.set_pixel(130, 70, [10, 20, 30, 255]);
        assert_eq!(img.allocated_tiles(), 1);
        assert_eq!(img.pixel(130, 70), [10, 20, 30, 255]);
        assert_eq!(img.pixel(-1, 0), [0; 4]);
        assert_eq!(img.content_bounds(), Some(IRect::new(130, 70, 131, 71)));
        img.set_pixel(130, 70, [0; 4]);
        img.prune();
        assert!(img.is_empty());
    }

    #[test]
    fn clone_is_copy_on_write() {
        let mut a = TiledImage::new(64, 64);
        a.set_pixel(1, 1, [1, 1, 1, 255]);
        let b = a.clone();
        a.set_pixel(1, 1, [9, 9, 9, 255]);
        assert_eq!(b.pixel(1, 1), [1, 1, 1, 255]);
        assert_eq!(a.pixel(1, 1), [9, 9, 9, 255]);
    }

    #[test]
    fn packed_tiles_read_and_write_the_same() {
        let mut a = TiledImage::new(128, 64);
        for x in 0..128 {
            a.set_pixel(x, 10, [200, 10, 10, 255]);
        }
        a.set_pixel(70, 40, [1, 2, 3, 4]);
        let saved = a.pack_all();
        assert!(saved > TILE_BYTES, "line art packs well ({saved} bytes saved)");
        assert!(a.tiles().all(|t| t.is_packed()));
        // Copies share the packed tiles; reading works without opening them.
        let b = a.clone();
        assert_eq!(b.read_region(&IRect::new(0, 0, 128, 64)), {
            let mut x = TiledImage::new(128, 64);
            for i in 0..128 {
                x.set_pixel(i, 10, [200, 10, 10, 255]);
            }
            x.set_pixel(70, 40, [1, 2, 3, 4]);
            x.read_region(&IRect::new(0, 0, 128, 64))
        });
        assert!(b.tiles().all(|t| t.is_packed()), "cold reads keep tiles packed");
        // pixel() opens the tile it reads; writing opens and unshares.
        assert_eq!(a.pixel(70, 40), [1, 2, 3, 4]);
        a.set_pixel(5, 5, [9, 9, 9, 255]);
        assert_eq!(a.pixel(5, 5), [9, 9, 9, 255]);
        assert_eq!(b.pixel(5, 5), [0; 4], "the copy didn't change");
        assert_eq!(b.pixel(127, 10), [200, 10, 10, 255]);
    }

    #[test]
    fn region_roundtrip() {
        let mut a = TiledImage::new(100, 100);
        a.set_pixel(70, 70, [5, 6, 7, 200]);
        let r = IRect::new(60, 60, 80, 80);
        let data = a.read_region(&r);
        let mut b = TiledImage::new(100, 100);
        b.write_region(60, 60, 20, 20, &data);
        assert_eq!(b.pixel(70, 70), [5, 6, 7, 200]);
        assert_eq!(b.allocated_tiles(), 1);
    }
}
