//! Decoder port. The pipeline names a capability, never a platform.
//!
//! The controlled PNG corpus, Apple adapter and experimental LibRaw/Bayer
//! adapters share `RasterInfo` and premultiplied linear Rec.2020 output.
//! Their recipe and input-colour provenance remain explicitly distinguishable.
//!
//! Trust is a property of the caller, not of this port. `Trust::External`
//! belongs to a sandboxed entry point that accepts arbitrary bytes; the
//! unconfined pipe worker stays on `Trust::Controlled`. Adding a
//! platform means implementing `Decoder` and publishing it from
//! `external_decoder`, never widening this contract.
use crate::{color::LinearImage, protocol::RasterInfo};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Input interpretation changes must invalidate persisted bitmap pixels.
pub const BITMAP_RECIPE: &str = "bitmap-gamma-ifd-v5";

/// RAW development identity. It belongs to each job, never to mutable worker
/// globals: a probe, decode and cache write must describe the same recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum RawEngine {
    Apple,
    LibRawBilinear,
    LibRawAhd,
    TrueRenderer,
}
impl Default for RawEngine {
    fn default() -> Self {
        if cfg!(target_os = "macos") {
            Self::Apple
        } else {
            Self::LibRawBilinear
        }
    }
}
impl RawEngine {
    pub const fn recipe(self) -> &'static str {
        match self {
            Self::Apple => "Apple-TR-linear-v1",
            Self::LibRawBilinear => "LibRaw-0.22.2-TR-libraw-linear-v1",
            Self::LibRawAhd => "LibRaw-0.22.2-TR-ahd-rec2020-v1",
            Self::TrueRenderer => "LibRaw-0.22.2-TR-directional-f32-v1",
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Apple => "Apple RAW",
            Self::LibRawBilinear => "LibRaw bilineare (storico)",
            Self::LibRawAhd => "LibRaw AHD",
            Self::TrueRenderer => "TrueRenderer fp32 (sperimentale)",
        }
    }
    pub fn available(self) -> bool {
        cfg!(target_os = "macos") || (cfg!(windows) && self != Self::Apple)
    }
    pub fn choices() -> impl Iterator<Item = Self> {
        [
            Self::Apple,
            Self::LibRawBilinear,
            Self::LibRawAhd,
            Self::TrueRenderer,
        ]
        .into_iter()
        .filter(|engine| engine.available())
    }
}

/// Historical platform recipe, retained for old cache compatibility and shim
/// regression checks. New jobs use `RawEngine::recipe` as their identity.
pub const RECIPE: &str = if cfg!(windows) {
    "TR-libraw-linear-v1"
} else if cfg!(target_os = "macos") {
    "Apple-TR-linear-v1"
} else {
    "nessuno-sviluppo-mosaico"
};

/// Who is allowed to decode the bytes of this request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Sources whose digest is in the built-in corpus manifest.
    Controlled,
    /// Arbitrary sources. Only a sandboxed host may ask for this.
    External,
}

/// How the input colour space was established.
///
/// Every decoder converts into the same linear Rec.2020 working space, so the
/// samples alone cannot say whether the conversion honoured the file or merely
/// guessed. That difference decides whether the render can be called faithful
/// to the original, which is why it is a type and not a line of prose.
///
/// There is deliberately no variant for "declared but not honoured". A file
/// that states a colour space this build cannot apply must be refused, because
/// substituting another one would change the meaning of its pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorSource {
    Scientific,
    /// The file states its colour space and this build applied it.
    Declared(String),
    /// RAW interpreted using a named recipe and decoder calibration. This is
    /// not an ICC declaration by the file or a measured colour-fidelity badge.
    Developed(String),
    /// The file states nothing. sRGB was assumed, so the render matches the
    /// original only where that assumption happens to hold.
    Assumed(String),
    /// Transfer is applied; only the primaries remain assumed.
    AssumedPrimaries(String),
}

impl ColorSource {
    /// Historical API: true for an applied declaration or named RAW recipe.
    /// This distinguishes assumed colour; it is not measured colour fidelity.
    pub fn faithful(&self) -> bool {
        matches!(self, Self::Declared(_) | Self::Developed(_))
    }

    /// Provenance line for the wire protocol and the render panel. Assumption
    /// is always visible in the text, never softened into a profile name.
    pub fn provenance(&self) -> String {
        match self {
            Self::Scientific => "Dati scientifici; proxy di vista normalizzato, nessuna interpretazione RGB/ICC del dato".into(),
            Self::Declared(profile) => format!("{profile} · dichiarato dal file"),
            Self::Developed(recipe) => format!("{recipe} · sviluppo RAW"),
            Self::Assumed(reason) => format!("sRGB assunto · {reason}"),
            Self::AssumedPrimaries(transfer) => format!("{transfer} · primarie sRGB assunte"),
        }
    }
}

pub trait Decoder: Send + Sync {
    /// Stable identity for provenance, reports and cache keys.
    fn name(&self) -> &'static str;

    /// Metadata and colour contract only. Must not allocate a raster.
    fn probe(&self, bytes: &[u8]) -> Result<(RasterInfo, ColorSource)>;

    /// Full development into the linear working space. `max_edge` of zero
    /// asks for source resolution; any other value is an upper bound on the
    /// longest returned edge.
    ///
    /// An implementation that meets a colour space it cannot apply returns an
    /// error naming it. Falling back to sRGB is not an acceptable recovery.
    fn decode(&self, bytes: &[u8], max_edge: u32)
    -> Result<(RasterInfo, ColorSource, LinearImage)>;
}
