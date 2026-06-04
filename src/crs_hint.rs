//! `CrsHint` — caller-supplied CRS policy passed to format readers.
//!
//! Two modes:
//!
//! - **`Detected`** — the reader walks the cascade defined by the
//!   consumer (PC's ADR-013): in-band metadata (LAS VLR; E57
//!   `coordinateMetadata`) → sidecar `.prj` / `.qpj` → local-frame
//!   fallback. The default for every workflow.
//! - **`Override(u16)`** — caller forces a specific EPSG. Wins
//!   silently over any in-band / sidecar value; the call site emits a
//!   `tracing::info` breadcrumb when the override displaced a detected
//!   EPSG, so support can trace the decision after the fact.
//!
//! There is no `LocalCartesian` variant. Local-frame ingest is a
//! resolution outcome (`CrsResolution::LocalFrame`), not a user-facing
//! knob.

/// Caller-supplied CRS policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsHint {
    /// Walk the auto-detect cascade.
    Detected,
    /// Force this EPSG; wins silently over in-band / sidecar values.
    Override(u16),
}

/// Outcome of resolving a [`CrsHint`] against detected source CRS data.
///
/// Records *how* the resolved EPSG (or local-frame fallback) was
/// produced so downstream consumers can emit accurate provenance
/// metadata — in particular `asset.extras.tileforge.originallyGeoreferenced`
/// in the tileset.json construkted-extras block.
///
/// **Phase 1 (this enum):** distinguishes user-supplied `--crs`,
/// successful auto-detection, and local-frame fallback. A separate
/// `InvalidMetadata` variant (input declared a CRS but it failed to
/// parse) is deferred until the readers track parse failure
/// separately from absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsResolution {
    /// User supplied `--crs <EPSG>` and the value was used. Wins
    /// silently over in-band/sidecar values.
    Override(u16),
    /// Reader auto-detected a CRS from the input (in-band metadata or
    /// sidecar `.prj`/`.qpj`) and resolved cleanly.
    Detected(u16),
    /// No CRS metadata present; ingest fell back to local-frame
    /// (stored downstream as EPSG:4978 so the reprojector
    /// short-circuits to identity).
    LocalFrame,
}

impl CrsResolution {
    /// EPSG of the resolved CRS, or `None` for local-frame ingest.
    pub fn epsg(self) -> Option<u16> {
        match self {
            CrsResolution::Override(e) | CrsResolution::Detected(e) => Some(e),
            CrsResolution::LocalFrame => None,
        }
    }

    /// Was the input originally georeferenced — either by detection
    /// from the file or via a user-supplied `--crs` override?
    ///
    /// Drives `asset.extras.tileforge.originallyGeoreferenced` in the
    /// tileset.json construkted-extras block.
    pub fn originally_georeferenced(self) -> bool {
        !matches!(self, CrsResolution::LocalFrame)
    }
}

impl CrsHint {
    /// Reconcile the hint against the EPSG detected from the source
    /// (in-band metadata folded together with sidecar lookup at the
    /// call site — pass `in_band.or(sidecar)`).
    ///
    /// Behaviour matrix:
    /// - `Override(o)` → `Override(o)` regardless of `detected`.
    /// - `Detected` + `Some(d)` → `Detected(d)`.
    /// - `Detected` + `None` → `LocalFrame`.
    ///
    /// Infallible: every input combination has a defined outcome.
    /// Mismatch between `Override(o)` and `detected = Some(d)` does
    /// not error; the call site logs the displacement at
    /// `tracing::info` level and proceeds with `o`.
    pub fn resolve(self, detected: Option<u16>) -> CrsResolution {
        match self {
            CrsHint::Override(o) => CrsResolution::Override(o),
            CrsHint::Detected => match detected {
                Some(d) => CrsResolution::Detected(d),
                None => CrsResolution::LocalFrame,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_with_in_band_passes_through() {
        assert_eq!(
            CrsHint::Detected.resolve(Some(32617)),
            CrsResolution::Detected(32617),
        );
    }

    #[test]
    fn detected_without_in_band_falls_back_to_local_frame() {
        // Absence of detected EPSG resolves to local-frame, not an error.
        assert_eq!(CrsHint::Detected.resolve(None), CrsResolution::LocalFrame);
    }

    #[test]
    fn override_matching_detected_returns_override() {
        assert_eq!(
            CrsHint::Override(32617).resolve(Some(32617)),
            CrsResolution::Override(32617),
        );
    }

    #[test]
    fn override_mismatching_detected_silently_wins() {
        // --crs override wins silently over detected; the displacement
        // is logged at the call site, not errored here.
        assert_eq!(
            CrsHint::Override(32617).resolve(Some(32619)),
            CrsResolution::Override(32617),
        );
    }

    #[test]
    fn override_without_detected_used_directly() {
        assert_eq!(
            CrsHint::Override(32617).resolve(None),
            CrsResolution::Override(32617),
        );
    }

    #[test]
    fn epsg_returns_none_for_local_frame_else_inner() {
        assert_eq!(CrsResolution::Override(32617).epsg(), Some(32617));
        assert_eq!(CrsResolution::Detected(32617).epsg(), Some(32617));
        assert_eq!(CrsResolution::LocalFrame.epsg(), None);
    }

    #[test]
    fn originally_georeferenced_is_true_iff_not_local_frame() {
        assert!(CrsResolution::Override(32617).originally_georeferenced());
        assert!(CrsResolution::Detected(32617).originally_georeferenced());
        assert!(!CrsResolution::LocalFrame.originally_georeferenced());
    }
}
