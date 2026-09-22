//! SDR presentation selection is independent of RAW development and CPU/GPU compute.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Precision {
    #[default]
    Compatible8,
    Sdr10,
    Sdr16Float,
}
impl Precision {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compatible8 => "SDR 8 bit · compatibile",
            Self::Sdr10 => "SDR 10 bit · sperimentale",
            Self::Sdr16Float => "SDR 16 float · sperimentale",
        }
    }
}
