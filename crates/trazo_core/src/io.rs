//! Saving and loading projects.
//!
//! A project file (`.anim`) is a zip archive containing `project.json` plus
//! one PNG per non-empty cel, cropped to its content to keep files small.

use std::fmt;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::composite::{premultiply, unpremultiply};
use crate::document::{CanvasSettings, Frame, FrameId, Layer, LayerId, Project};
use crate::raster::TiledImage;

pub const FILE_EXTENSION: &str = "anim";
const FORMAT_NAME: &str = "trazo-project";
/// Name used before the app was renamed; such files still open.
const OLD_FORMAT_NAME: &str = "animate-project";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Json(serde_json::Error),
    Png(String),
    Invalid(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "error de archivo: {e}"),
            ProjectError::Zip(e) => write!(f, "archivo dañado: {e}"),
            ProjectError::Json(e) => write!(f, "datos del proyecto dañados: {e}"),
            ProjectError::Png(e) => write!(f, "imagen dañada: {e}"),
            ProjectError::Invalid(e) => write!(f, "proyecto no válido: {e}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}
impl From<zip::result::ZipError> for ProjectError {
    fn from(e: zip::result::ZipError) -> Self {
        ProjectError::Zip(e)
    }
}
impl From<serde_json::Error> for ProjectError {
    fn from(e: serde_json::Error) -> Self {
        ProjectError::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    name: String,
    canvas: CanvasSettings,
    next_id: u64,
    layers: Vec<Layer>,
    frames: Vec<FrameManifest>,
    /// Cels of shared (background) layers.
    #[serde(default)]
    shared_cels: Vec<CelManifest>,
    #[serde(default)]
    audio: AudioManifest,
}

#[derive(Serialize, Deserialize, Default)]
struct AudioManifest {
    tracks: Vec<crate::audio::AudioTrack>,
    #[serde(default)]
    markers: Vec<crate::audio::Marker>,
    assets: Vec<AssetManifest>,
    #[serde(default = "one")]
    master_gain: f32,
    /// Clip positions are in frames at this rate.
    #[serde(default = "rate")]
    sample_rate: u32,
}

fn rate() -> u32 {
    crate::audio::SAMPLE_RATE
}

/// Encoded WAV per asset. Assets never change, so autosaves reuse the bytes
/// instead of re-encoding minutes of audio every time.
fn cached_wav(asset: &std::sync::Arc<crate::audio::AudioAsset>) -> Result<std::sync::Arc<Vec<u8>>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock, Weak};
    type Cache = Mutex<HashMap<usize, (Weak<crate::audio::AudioAsset>, Arc<Vec<u8>>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = Arc::as_ptr(asset) as usize;
    if let Ok(c) = cache.lock() {
        if let Some((w, bytes)) = c.get(&key) {
            if w.upgrade().is_some_and(|a| Arc::ptr_eq(&a, asset)) {
                return Ok(bytes.clone());
            }
        }
    }
    let bytes = Arc::new(crate::audio::encode_wav(&asset.samples)?);
    if let Ok(mut c) = cache.lock() {
        c.retain(|_, (w, _)| w.strong_count() > 0);
        c.insert(key, (Arc::downgrade(asset), bytes.clone()));
    }
    Ok(bytes)
}

fn one() -> f32 {
    1.0
}

#[derive(Serialize, Deserialize)]
struct AssetManifest {
    id: crate::audio::AssetId,
    name: String,
    file: String,
}

#[derive(Serialize, Deserialize)]
struct FrameManifest {
    id: FrameId,
    hold: u32,
    cels: Vec<CelManifest>,
}

#[derive(Serialize, Deserialize)]
struct CelManifest {
    layer: LayerId,
    x: i32,
    y: i32,
    file: String,
}

fn encode_png(w: u32, h: u32, straight_rgba: &[u8]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().map_err(|e| ProjectError::Png(e.to_string()))?;
        wr.write_image_data(straight_rgba).map_err(|e| ProjectError::Png(e.to_string()))?;
    }
    Ok(buf)
}

/// Decode a PNG into straight RGBA8.
/// Any color type / bit depth is converted. The size is checked against
/// `MAX_IMPORT_SIDE` *before* allocating, so a tiny file that claims a huge
/// image can't exhaust memory.
pub fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    decode_image(bytes)
}

/// Read a zip entry, refusing anything bigger than `max` bytes (zip bombs).
pub(crate) fn read_entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str, max: u64) -> Result<Vec<u8>> {
    let entry = zip.by_name(name)?;
    if entry.size() > max {
        return Err(ProjectError::Invalid(format!("«{name}» es demasiado grande")));
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.take(max + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > max {
        return Err(ProjectError::Invalid(format!("«{name}» es demasiado grande")));
    }
    Ok(buf)
}

/// Limits for data read from project and brush files.
pub(crate) const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;
/// 16-bit stereo WAV at 48 kHz for the longest allowed clip, plus headroom.
pub(crate) const MAX_AUDIO_BYTES: u64 = crate::audio::MAX_SECONDS * 48_000 * 4 + 1024 * 1024;

/// Largest image side accepted on import (protects memory on phones).
pub const MAX_IMPORT_SIDE: u32 = 8192;

/// Decode PNG, JPEG, GIF, BMP or WebP into straight RGBA8.
pub fn decode_image(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ProjectError::Png(e.to_string()))?;
    let (w, h) = reader.into_dimensions().map_err(|e| ProjectError::Png(e.to_string()))?;
    if w == 0 || h == 0 || w > MAX_IMPORT_SIDE || h > MAX_IMPORT_SIDE {
        return Err(ProjectError::Invalid(format!("imagen de {w}×{h}: el máximo es {MAX_IMPORT_SIDE} px por lado")));
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ProjectError::Png(e.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMPORT_SIDE);
    limits.max_image_height = Some(MAX_IMPORT_SIDE);
    limits.max_alloc = Some(MAX_IMAGE_BYTES * 2);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| ProjectError::Png(e.to_string()))?;
    let rgba = img.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// Scale straight RGBA8 to fit inside `max_w`×`max_h` (never enlarges).
pub fn fit_image(w: u32, h: u32, rgba: Vec<u8>, max_w: u32, max_h: u32) -> (u32, u32, Vec<u8>) {
    let f = (max_w as f32 / w as f32).min(max_h as f32 / h as f32);
    if f >= 1.0 {
        return (w, h, rgba);
    }
    let (nw, nh) = (((w as f32 * f).round() as u32).max(1), ((h as f32 * f).round() as u32).max(1));
    let Some(img) = image::RgbaImage::from_raw(w, h, rgba) else { return (0, 0, Vec::new()) };
    let out = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    (nw, nh, out.into_raw())
}

/// Encode straight RGBA8 as PNG (used by exporters too).
pub fn png_bytes(w: u32, h: u32, straight_rgba: &[u8]) -> Result<Vec<u8>> {
    encode_png(w, h, straight_rgba)
}

pub fn save_project(p: &Project, path: &Path) -> Result<()> {
    // Unique temp name so two saves can never write the same file.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("anim.{}.{n}.tmp", std::process::id()));
    let result = write_archive(p, &tmp, path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    // Replace the old file only once the new one is complete.
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Description of a video's pictures inside the project file.
#[derive(Serialize, Deserialize)]
struct VideoManifest {
    uid: u64,
    name: String,
    width: u32,
    height: u32,
    fps: f32,
    count: usize,
    thumb_w: u32,
    thumb_h: u32,
}

fn video_dir(uid: u64) -> String {
    format!("video/{uid}/")
}

/// Write the pictures of one video. When the previous version of the file
/// already holds them (media never changes) they are copied as they are,
/// so autosaves don't rewrite hundreds of megabytes.
fn write_video<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    old: &mut Option<zip::ZipArchive<File>>,
    m: &crate::video::VideoMedia,
    stored: zip::write::SimpleFileOptions,
) -> Result<()> {
    let dir = video_dir(m.uid);
    let meta = VideoManifest { uid: m.uid, name: m.name.clone(), width: m.width, height: m.height, fps: m.fps, count: m.frames.len(), thumb_w: m.thumb_w, thumb_h: m.thumb_h };
    let names: Vec<String> = (0..m.frames.len()).map(|i| format!("{dir}{i}")).chain([format!("{dir}thumbs.bin")]).collect();
    if let Some(old) = old.as_mut() {
        let complete = names.iter().all(|n| old.index_for_name(n).is_some());
        let same = read_entry(old, &format!("{dir}meta.json"), MAX_JSON_BYTES)
            .ok()
            .and_then(|b| serde_json::from_slice::<VideoManifest>(&b).ok())
            .is_some_and(|o| o.uid == m.uid && o.count == m.frames.len());
        if complete && same {
            for n in &names {
                zip.raw_copy_file(old.by_name(n)?)?;
            }
            zip.start_file(format!("{dir}meta.json"), stored)?;
            zip.write_all(&serde_json::to_vec(&meta)?)?;
            return Ok(());
        }
    }
    for (i, f) in m.frames.iter().enumerate() {
        zip.start_file(format!("{dir}{i}"), stored)?;
        zip.write_all(&f[..])?;
    }
    zip.start_file(format!("{dir}thumbs.bin"), stored)?;
    for t in &m.thumbs {
        zip.write_all(t)?;
    }
    // Written last: its presence means the pictures are complete.
    zip.start_file(format!("{dir}meta.json"), stored)?;
    zip.write_all(&serde_json::to_vec(&meta)?)?;
    Ok(())
}

/// Read the pictures of one video (None if missing or damaged).
fn read_video(zip: &mut zip::ZipArchive<File>, uid: u64) -> Result<crate::video::VideoMedia> {
    let dir = video_dir(uid);
    let meta: VideoManifest = serde_json::from_slice(&read_entry(zip, &format!("{dir}meta.json"), MAX_JSON_BYTES)?)?;
    let side = crate::io::MAX_IMPORT_SIDE;
    if meta.uid != uid
        || meta.count == 0
        || meta.count > crate::video::MAX_FRAMES
        || meta.width == 0
        || meta.height == 0
        || meta.width > side
        || meta.height > side
        || meta.thumb_w == 0
        || meta.thumb_h == 0
        || meta.thumb_w > 512
        || meta.thumb_h > 512
        || !(0.1..=240.0).contains(&meta.fps)
    {
        return Err(ProjectError::Invalid(format!("video «{}» no válido", meta.name)));
    }
    let mut frames = Vec::with_capacity(meta.count);
    for i in 0..meta.count {
        frames.push(read_entry(zip, &format!("{dir}{i}"), MAX_IMAGE_BYTES)?.into());
    }
    let tsize = (meta.thumb_w * meta.thumb_h * 4) as usize;
    let total = (tsize as u64).checked_mul(meta.count as u64).filter(|t| *t <= MAX_IMAGE_BYTES * 4).ok_or_else(|| ProjectError::Invalid("miniaturas demasiado grandes".into()))?;
    let all = read_entry(zip, &format!("{dir}thumbs.bin"), total)?;
    if all.len() as u64 != total {
        return Err(ProjectError::Invalid(format!("miniaturas del video «{}» dañadas", meta.name)));
    }
    let thumbs = all.chunks_exact(tsize).map(|c| c.to_vec().into()).collect();
    Ok(crate::video::VideoMedia { uid, name: meta.name, width: meta.width, height: meta.height, fps: meta.fps, frames, thumbs, thumb_w: meta.thumb_w, thumb_h: meta.thumb_h })
}

fn write_archive(p: &Project, tmp: &Path, previous: &Path) -> Result<()> {
    {
        let mut old = File::open(previous).ok().and_then(|f| zip::ZipArchive::new(f).ok());
        let file = File::create(tmp)?;
        let mut zip = zip::ZipWriter::new(file);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        let mut frames = Vec::with_capacity(p.frames.len());
        for f in &p.frames {
            let mut cels = Vec::new();
            for (&lid, img) in &f.cels {
                let Some(b) = img.content_bounds() else { continue };
                let mut data = img.read_region(&b);
                unpremultiply(&mut data);
                let file = format!("cels/{}_{}.png", f.id, lid);
                zip.start_file(&file, stored)?;
                zip.write_all(&encode_png(b.width() as u32, b.height() as u32, &data)?)?;
                cels.push(CelManifest { layer: lid, x: b.x0, y: b.y0, file });
            }
            frames.push(FrameManifest { id: f.id, hold: f.hold, cels });
        }
        let mut shared_cels = Vec::new();
        for (&lid, img) in &p.shared_cels {
            let Some(b) = img.content_bounds() else { continue };
            let mut data = img.read_region(&b);
            unpremultiply(&mut data);
            let file = format!("cels/shared_{lid}.png");
            zip.start_file(&file, stored)?;
            zip.write_all(&encode_png(b.width() as u32, b.height() as u32, &data)?)?;
            shared_cels.push(CelManifest { layer: lid, x: b.x0, y: b.y0, file });
        }
        let mut assets = Vec::new();
        for id in p.audio.used_assets() {
            let Some(a) = p.audio.assets.get(&id) else { continue };
            let file = format!("audio/{id}.wav");
            zip.start_file(&file, stored)?;
            zip.write_all(&cached_wav(a)?)?;
            assets.push(AssetManifest { id, name: a.name.clone(), file });
        }
        for m in p.video_media() {
            write_video(&mut zip, &mut old, &m, stored)?;
        }
        // Pictures that couldn't be loaded are copied as they were, so a save
        // never deletes them.
        let mut missing: Vec<u64> = p.layers.iter().filter_map(|l| l.props.video.as_ref().filter(|v| v.data.is_none()).map(|v| v.media)).collect();
        missing.sort_unstable();
        missing.dedup();
        if let Some(old) = old.as_mut() {
            for uid in missing {
                let prefix = video_dir(uid);
                let names: Vec<String> = old.file_names().filter_map(|n| n.ok()).filter(|n| n.starts_with(&prefix)).map(|n| n.into_owned()).collect();
                for n in names {
                    zip.raw_copy_file(old.by_name(&n)?)?;
                }
            }
        }
        let audio = AudioManifest { tracks: p.audio.tracks.clone(), markers: p.audio.markers.clone(), assets, master_gain: p.audio.master_gain, sample_rate: crate::audio::SAMPLE_RATE };
        let manifest = Manifest {
            format: FORMAT_NAME.into(),
            version: FORMAT_VERSION,
            name: p.name.clone(),
            canvas: p.canvas.clone(),
            next_id: p.next_id(),
            layers: p.layers.clone(),
            frames,
            shared_cels,
            audio,
        };
        zip.start_file("project.json", deflated)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        let file = zip.finish()?;
        file.sync_all()?;
    }
    Ok(())
}

pub fn load_project(path: &Path) -> Result<Project> {
    let mut zip = zip::ZipArchive::new(File::open(path)?)?;
    let manifest: Manifest = {
        let bytes = read_entry(&mut zip, "project.json", MAX_JSON_BYTES)?;
        serde_json::from_slice(&bytes)?
    };
    if manifest.format != FORMAT_NAME && manifest.format != OLD_FORMAT_NAME {
        return Err(ProjectError::Invalid("no es un proyecto de Trazo".into()));
    }
    if manifest.version > FORMAT_VERSION {
        return Err(ProjectError::Invalid("creado con una versión más nueva de la app".into()));
    }
    let (w, h) = (manifest.canvas.width, manifest.canvas.height);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(ProjectError::Invalid(format!("tamaño de lienzo {w}×{h}")));
    }
    if manifest.layers.is_empty() || manifest.frames.is_empty() {
        return Err(ProjectError::Invalid("sin capas o sin fotogramas".into()));
    }
    let mut read_cel = |c: &CelManifest| -> Result<TiledImage> {
        let bytes = read_entry(&mut zip, &c.file, MAX_IMAGE_BYTES)?;
        let (cw, ch, mut data) = decode_png(&bytes)?;
        // A cel can never be larger than the canvas or start outside it.
        if cw > w || ch > h || c.x < 0 || c.y < 0 || c.x as u32 + cw > w || c.y as u32 + ch > h {
            return Err(ProjectError::Invalid(format!("la imagen {} no cabe en el lienzo", c.file)));
        }
        premultiply(&mut data);
        let mut img = TiledImage::new(w, h);
        img.write_region(c.x, c.y, cw as usize, ch as usize, &data);
        Ok(img)
    };
    let mut frames = Vec::with_capacity(manifest.frames.len());
    for fm in &manifest.frames {
        let mut frame = Frame { id: fm.id, hold: fm.hold.max(1), cels: Default::default() };
        for c in &fm.cels {
            frame.cels.insert(c.layer, read_cel(c)?);
        }
        frames.push(frame);
    }
    let mut shared = std::collections::BTreeMap::new();
    for c in &manifest.shared_cels {
        shared.insert(c.layer, read_cel(c)?);
    }
    if manifest.audio.sample_rate != crate::audio::SAMPLE_RATE && !manifest.audio.tracks.is_empty() {
        return Err(ProjectError::Invalid(format!("audio a {} Hz no soportado", manifest.audio.sample_rate)));
    }
    let mut audio = crate::audio::AudioTimeline { master_gain: manifest.audio.master_gain.clamp(0.0, 4.0), ..Default::default() };
    for a in &manifest.audio.assets {
        // A damaged sound is skipped (its clips are dropped below) instead of
        // making the whole animation impossible to open.
        let decoded = read_entry(&mut zip, &a.file, MAX_AUDIO_BYTES).and_then(crate::audio::decode_audio);
        match decoded {
            Ok(samples) => {
                audio.assets.insert(a.id, std::sync::Arc::new(crate::audio::AudioAsset::new(a.name.clone(), samples)));
            }
            Err(e) => eprintln!("audio «{}» dañado, se omite: {e}", a.name),
        }
    }
    audio.tracks = manifest.audio.tracks;
    audio.markers = manifest.audio.markers;
    audio.markers.sort_by_key(|m| m.at);
    // Drop clips whose audio is missing and keep the rest inside their asset.
    for t in &mut audio.tracks {
        t.clips.retain(|c| audio.assets.contains_key(&c.asset));
        for c in &mut t.clips {
            let frames = audio.assets[&c.asset].frames();
            c.clamp_to(frames.max(1));
        }
    }
    // Attach video pictures; a damaged video leaves an empty layer (the
    // drawings stay usable).
    let mut layers = manifest.layers;
    let mut videos: std::collections::HashMap<u64, Option<std::sync::Arc<crate::video::VideoMedia>>> = Default::default();
    for l in &mut layers {
        if let Some(v) = &mut l.props.video {
            let media = videos
                .entry(v.media)
                .or_insert_with(|| match read_video(&mut zip, v.media) {
                    Ok(m) => Some(std::sync::Arc::new(m)),
                    Err(e) => {
                        eprintln!("video dañado, se omite: {e}");
                        None
                    }
                })
                .clone();
            v.data = media;
            v.clamp();
        }
    }
    Ok(Project::from_parts(manifest.name, manifest.canvas, layers, frames, shared, audio, manifest.next_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let mut p = Project::new("Prueba", CanvasSettings { width: 300, height: 200, fps: 24, ..Default::default() });
        let lid = p.layers[0].id;
        p.frames[0].cel_mut(lid, 300, 200).set_pixel(150, 100, [100, 50, 25, 128]);
        p.frames[0].hold = 3;
        let dir = std::env::temp_dir().join(format!("trazo_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.anim");
        save_project(&p, &path).unwrap();
        let q = load_project(&path).unwrap();
        assert_eq!(q.name, "Prueba");
        assert_eq!(q.canvas.fps, 24);
        assert_eq!(q.frames[0].hold, 3);
        let px = q.frames[0].cel(lid).unwrap().pixel(150, 100);
        assert!(px[3] == 128 && (px[0] as i32 - 100).abs() <= 2, "{px:?}");
        assert!(q.next_id() >= p.next_id());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn video_survives_save_and_autosave() {
        use crate::video::{VideoBuilder, VideoLayer};
        let mut p = Project::new("Video", CanvasSettings { width: 64, height: 48, ..Default::default() });
        let mut b = VideoBuilder::new(64, 48, 12.0);
        for i in 0..4u8 {
            b.push(16, 12, vec![i * 50, 20, 30, 255].repeat(16 * 12)).unwrap();
        }
        let media = std::sync::Arc::new(b.finish("clip").unwrap());
        let id = p.alloc_id();
        let mut props = crate::LayerProps::named("Video");
        props.video = Some(VideoLayer::new(media.clone(), 3, 64, 48));
        p.layers.insert(0, crate::Layer { id, props });
        // A copy of the layer shares the pictures: stored once.
        let id2 = p.alloc_id();
        let copy = p.layers[0].props.clone();
        p.layers.push(crate::Layer { id: id2, props: copy });
        let dir = std::env::temp_dir().join(format!("trazo_video_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.anim");
        save_project(&p, &path).unwrap();
        // Second save copies the stored pictures from the first file.
        save_project(&p, &path).unwrap();
        let q = load_project(&path).unwrap();
        let v = q.layers[0].props.video.as_ref().unwrap();
        assert_eq!(v, p.layers[0].props.video.as_ref().unwrap());
        let m = v.data.as_ref().unwrap();
        assert_eq!((m.len(), m.width, m.height, m.uid), (4, 16, 12, media.uid));
        assert_eq!(m.frames, media.frames);
        assert_eq!(m.thumbs, media.thumbs);
        assert!(std::sync::Arc::ptr_eq(m, q.layers[2].props.video.as_ref().unwrap().data.as_ref().unwrap()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn audio_survives_save_and_load() {
        use crate::audio::{AudioAsset, AudioClip, AudioTrack};
        let mut p = Project::new("Sonido", CanvasSettings { width: 64, height: 64, ..Default::default() });
        let samples: Vec<f32> = (0..9600).map(|i| ((i as f32) * 0.01).sin() * 0.3).collect();
        let aid = p.alloc_id();
        p.audio.assets.insert(aid, std::sync::Arc::new(AudioAsset::new("voz", samples)));
        // An unused asset must not be written.
        let unused = p.alloc_id();
        p.audio.assets.insert(unused, std::sync::Arc::new(AudioAsset::new("x", vec![0.0; 20])));
        let tid = p.alloc_id();
        let cid = p.alloc_id();
        let mut t = AudioTrack::new(tid, "Voz");
        t.clips.push(AudioClip { id: cid, asset: aid, start: 480, offset: 10, len: 4000, gain: 0.8, fade_in: 100, fade_out: 0, muted: false });
        p.audio.tracks.push(t);
        let dir = std::env::temp_dir().join(format!("trazo_audio_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.anim");
        save_project(&p, &path).unwrap();
        let q = load_project(&path).unwrap();
        assert_eq!(q.audio.tracks, p.audio.tracks);
        assert_eq!(q.audio.assets.len(), 1);
        assert_eq!(q.audio.assets[&aid].frames(), 4800);
        assert!(q.next_id() > cid);
        std::fs::remove_dir_all(&dir).ok();
    }
}
