//! Canonical source-CRS handle.
//!
//! A thin newtype around a `u16` EPSG code. Both VLR-extraction paths
//! (GeoTIFF GeoKey + OGC WKT) end here; everything downstream consumes
//! `SourceCrs` and never sees raw VLR bytes or WKT strings.

/// Canonical source-CRS handle. Carries an EPSG code only — keeping the
/// internal representation this narrow lets every downstream consumer
/// (the `Reprojector`, the parity oracle, future `--source-crs` flag
/// support) work uniformly without re-parsing VLR-format-specific
/// payloads.
///
/// The newtype shape is deliberate: a bare `u16` would compose silently
/// with unrelated counts/sizes; wrapping it in a struct keeps the
/// type-system word load-bearing at API boundaries while staying simple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceCrs {
    pub epsg: u16,
}

impl SourceCrs {
    /// Construct from a raw EPSG code. Validation (is this code in the
    /// `crs-definitions` catalogue?) happens when `Reprojector::new` is
    /// called — we don't pre-check here so callers that just want to
    /// remember the code can do so without paying for the lookup.
    pub const fn new(epsg: u16) -> Self {
        Self { epsg }
    }

    /// EPSG:4978 — ECEF metres. The fixed reprojection target for 3D
    /// Tiles 1.1 `box` bounding volumes.
    pub const ECEF: Self = Self { epsg: 4978 };
}
