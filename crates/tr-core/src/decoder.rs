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
pub const BITMAP_RECIPE: &str = if cfg!(windows) {
    "bitmap-icc-lcms219-rgb-matrix-relative-v6"
} else {
    "bitmap-gamma-ifd-v5"
};

/// RAW development identity. It belongs to each job, never to mutable worker
/// globals: a probe, decode and cache write must describe the same recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum RawEngine {
    Apple,
    LibRawBilinear,
    LibRawAhd,
    TrueRenderer,
    TrueRendererExperimental,
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
            Self::TrueRenderer => "LibRaw-0.22.2-TR-directional-f32-sensor-highlights-v2",
            Self::TrueRendererExperimental => {
                "TRExp-raw3-nikon2-fuji1-lj2-cal1-dir2-xt7-wb3-mat2-hl4-extended1"
            }
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Apple => "Apple RAW",
            Self::LibRawBilinear => "LibRaw bilineare (storico)",
            Self::LibRawAhd => "LibRaw AHD",
            Self::TrueRenderer => "(old)truerenderer",
            Self::TrueRendererExperimental => "trueRendererExperimental 32fp",
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
            Self::TrueRendererExperimental,
        ]
        .into_iter()
        .filter(|engine| engine.available())
    }
}

/// Native WB: Apple temperature/tint, or relative sensor gains before demosaic.
/// Integers make request/cache identity exact; 1000 means a sensor gain of one.
/// Apple temperature zero selects as-shot, otherwise it is expressed in kelvin.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawWhiteBalance {
    pub red: u16,
    pub blue: u16,
    /// Optional millionths for Experimental only. Absent in historical recipes.
    /// Legacy fields retain their nearest-thousandth values for display/migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub micro_gains: Option<[u32; 2]>,
    #[serde(default)]
    pub apple_temperature: u16,
    #[serde(default)]
    pub apple_tint: i16,
}
impl Default for RawWhiteBalance {
    fn default() -> Self {
        Self {
            red: 1000,
            blue: 1000,
            micro_gains: None,
            apple_temperature: 0,
            apple_tint: 0,
        }
    }
}
impl RawWhiteBalance {
    pub fn is_as_shot(&self) -> bool {
        *self == Self::default()
    }
    pub fn validate(self) -> Result<()> {
        anyhow::ensure!(
            (250..=4000).contains(&self.red) && (250..=4000).contains(&self.blue),
            "Guadagni WB RAW fuori scala"
        );
        if let Some([red, blue]) = self.micro_gains {
            anyhow::ensure!(
                (250_000..=4_000_000).contains(&red)
                    && (250_000..=4_000_000).contains(&blue)
                    && u32::from(self.red) == (red + 500) / 1000
                    && u32::from(self.blue) == (blue + 500) / 1000,
                "Guadagni WB RAW precisi fuori scala o incoerenti"
            );
        }
        anyhow::ensure!(
            (self.apple_temperature == 0 && self.apple_tint == 0)
                || ((2000..=50000).contains(&self.apple_temperature)
                    && (-150..=150).contains(&self.apple_tint)),
            "Temperatura/tinta Apple RAW fuori scala"
        );
        Ok(())
    }
    pub fn validate_for(self, engine: RawEngine) -> Result<()> {
        self.validate()?;
        anyhow::ensure!(
            self.micro_gains.is_none() || engine == RawEngine::TrueRendererExperimental,
            "WB RAW preciso riservato al motore autonomo"
        );
        anyhow::ensure!(
            if engine == RawEngine::Apple {
                self.red == 1000 && self.blue == 1000
            } else {
                self.apple_temperature == 0 && self.apple_tint == 0
            },
            "Parametri WB RAW incompatibili con il motore"
        );
        Ok(())
    }
    pub fn gains(self) -> [f32; 3] {
        if let Some([red, blue]) = self.micro_gains {
            [red as f32 / 1_000_000., 1., blue as f32 / 1_000_000.]
        } else {
            [self.red as f32 / 1000., 1., self.blue as f32 / 1000.]
        }
    }
    pub fn gains_f64(self) -> [f64; 3] {
        if let Some([red, blue]) = self.micro_gains {
            [
                f64::from(red) / 1_000_000.,
                1.,
                f64::from(blue) / 1_000_000.,
            ]
        } else {
            // Preserve the exact previous interpretation of stored recipes.
            self.gains().map(f64::from)
        }
    }
    pub fn precise_sensor_gains(gains: [f64; 3]) -> Result<Self> {
        anyhow::ensure!(
            gains[1] == 1.
                && gains
                    .iter()
                    .all(|v| v.is_finite() && (0.25..=4.).contains(v)),
            "Guadagni WB RAW precisi fuori scala"
        );
        let micro = [gains[0], gains[2]].map(|v| (v * 1_000_000.).round() as u32);
        let result = Self {
            red: ((micro[0] + 500) / 1000) as u16,
            blue: ((micro[1] + 500) / 1000) as u16,
            micro_gains: if micro == [1_000_000; 2] {
                None
            } else {
                Some(micro)
            },
            ..Default::default()
        };
        result.validate_for(RawEngine::TrueRendererExperimental)?;
        Ok(result)
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

#[cfg(test)]
mod wb_tests {
    use super::*;
    #[test]
    fn precise_wb_preserves_old_json_and_separates_sub_millisteps() {
        let old = r#"{"red":1333,"blue":800,"apple_temperature":0,"apple_tint":0}"#;
        let legacy: RawWhiteBalance = serde_json::from_str(old).unwrap();
        assert_eq!(serde_json::to_string(&legacy).unwrap(), old);
        assert_eq!(
            legacy.gains_f64(),
            [f64::from(1.333f32), 1., f64::from(0.8f32)]
        );
        let a = RawWhiteBalance::precise_sensor_gains([0.25049049, 1., 1.333317]).unwrap();
        let b = RawWhiteBalance::precise_sensor_gains([0.25049149, 1., 1.333317]).unwrap();
        assert_eq!((a.red, a.blue), (b.red, b.blue));
        assert_ne!(a, b);
        assert_ne!(a.gains_f64(), b.gains_f64());
        let roundtrip: RawWhiteBalance =
            serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(a, roundtrip);
        assert_eq!(a.gains_f64(), [0.25049, 1., 1.333317]);
        assert!(a.validate_for(RawEngine::TrueRenderer).is_err());
        assert!(a.validate_for(RawEngine::Apple).is_err());
        assert!(RawWhiteBalance { red: 251, ..a }.validate().is_err());
        assert!(
            RawWhiteBalance {
                micro_gains: Some([249999, 1000000]),
                ..a
            }
            .validate()
            .is_err()
        );
        for gains in [[f64::NAN, 1., 1.], [1., 2., 1.], [4.000001, 1., 1.]] {
            assert!(RawWhiteBalance::precise_sensor_gains(gains).is_err());
        }
        assert!(
            RawWhiteBalance::precise_sensor_gains([1.; 3])
                .unwrap()
                .is_as_shot()
        );
    }
    #[test]
    fn white_balance_capabilities_and_bounds_are_engine_specific() {
        let apple = RawWhiteBalance {
            apple_temperature: 4500,
            apple_tint: 12,
            ..Default::default()
        };
        apple.validate_for(RawEngine::Apple).unwrap();
        assert!(apple.validate_for(RawEngine::TrueRenderer).is_err());
        for temperature in [1, 1999, 50001] {
            assert!(
                RawWhiteBalance {
                    apple_temperature: temperature,
                    ..apple
                }
                .validate()
                .is_err()
            );
        }
        for tint in [-151, 151] {
            assert!(
                RawWhiteBalance {
                    apple_tint: tint,
                    ..apple
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            RawWhiteBalance { red: 1500, ..apple }
                .validate_for(RawEngine::Apple)
                .is_err()
        );
        assert!(
            RawWhiteBalance {
                apple_temperature: 0,
                ..apple
            }
            .validate()
            .is_err()
        );
    }
}
