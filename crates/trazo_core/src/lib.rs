//! Trazo core: the platform-independent engine of the animation app.
//!
//! Nothing in this crate knows about the UI, so it can be tested on its own and
//! reused by any front end (desktop, Android, command-line export).

pub mod audio;
pub mod brush;
pub mod camera;
pub mod brush_io;
pub mod color;
pub mod composite;
pub mod document;
pub mod export;
pub mod fill;
pub mod geom;
pub mod history;
pub mod io;
pub mod ops;
pub mod preview;
pub mod raster;
pub mod shapes;
pub mod stroke;
pub mod templates;
pub mod text;
pub mod timeline;
pub mod transform;
pub mod video;

pub use color::{BlendMode, Rgba};
pub use document::{CanvasSettings, Frame, FrameId, Layer, LayerId, LayerProps, Project};
pub use geom::IRect;
pub use history::{Edit, History};
pub use raster::TiledImage;
