// ABOUTME: Selects the Helmert datum shift to WGS 84 that a catalogue CRS needs.
// ABOUTME: Refuses a CRS whose datum has no published static Helmert transformation.
//! Datum shifts for catalogue definitions that carry no datum.
//!
//! The proj4 catalogue (`crs-definitions`) describes many CRSs by projection
//! and ellipsoid only. For a GRS80 or WGS 84 ellipsoid that is correct to the
//! metre. For any other ellipsoid (Airy, Bessel, International 1924,
//! Krassowsky and so on) the datum is offset from WGS 84 by tens to hundreds
//! of metres, and the catalogue does not say by how much.
//!
//! This module supplies the missing shift from a generated table of EPSG
//! Helmert transformations (`datum_shift_table`). Each affected CRS code maps
//! to the operation that PROJ ranks first, without grid files, over the whole
//! EPSG area of use of that CRS. A CRS that needs a shift and has no such
//! operation is refused: its position on WGS 84 cannot be derived.

use crate::datum_shift_table::{CRS_OPERATIONS, CRS_WITHOUT_SHIFT, OPERATIONS};

/// A Helmert datum shift from the datum of a source CRS to WGS 84.
///
/// [`Reprojector::datum_shift`](crate::Reprojector::datum_shift) returns the
/// shift that a reprojector applies. A Helmert transformation is an
/// approximation of the relation between two datums: `accuracy_m` is the
/// accuracy that EPSG publishes for it. A grid-based transformation, which
/// this crate does not support, is more accurate where one exists. A consumer
/// should tell the user which operation placed the data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumShift {
    /// Identifier of the coordinate operation, for example `"EPSG:1314"`.
    /// Two published operations that combine into one Helmert shift are
    /// joined with `+`, for example `"EPSG:5826+EPSG:1149"`.
    pub operation_id: &'static str,
    /// Name of the coordinate operation, for example
    /// `"OSGB36 to WGS 84 (6)"`.
    pub operation_name: &'static str,
    /// Accuracy in metres that EPSG publishes for the operation, or `None`
    /// when EPSG states none.
    pub accuracy_m: Option<f64>,
    /// The seven parameters in the Position Vector convention of a proj4
    /// `+towgs84` clause: `[tx, ty, tz, rx, ry, rz, ds]` with translations in
    /// metres, rotations in arc-seconds and the scale difference in parts per
    /// million.
    pub parameters: [f64; 7],
}

impl DatumShift {
    /// The proj4 `+towgs84=` clause for this shift.
    fn towgs84_clause(&self) -> String {
        let p = &self.parameters;
        format!(
            "+towgs84={},{},{},{},{},{},{}",
            p[0], p[1], p[2], p[3], p[4], p[5], p[6]
        )
    }

    /// The catalogue definition `proj4` with this shift added, in the form
    /// that proj4rs 0.1.10 reads correctly.
    ///
    /// proj4rs 0.1.10 misreads three things that catalogue definitions use,
    /// and each one moves a position by metres to thousands of kilometres:
    ///
    /// - it ignores the scale factor `+k_0=` (it reads `+k=` only);
    /// - it derives a wrong flattening for an ellipsoid given by its minor
    ///   axis (`+b=`, and the named ellipsoids that it stores that way), which
    ///   the Transverse Mercator series then use;
    /// - it adds a named or numeric `+pm=` to the longitude in radians
    ///   although the value is in degrees.
    ///
    /// The returned definition states the same CRS with `+k=`, with
    /// `+a=`/`+rf=`, and with the prime meridian in radians.
    pub(crate) fn definition_with_shift(&self, proj4: &str) -> String {
        let value = |key: &str| {
            proj4
                .split_whitespace()
                .find_map(|t| t.strip_prefix('+')?.strip_prefix(key)?.strip_prefix('='))
        };
        let semi_major = value("a").and_then(|v| v.parse::<f64>().ok());
        let has_k = value("k").is_some();
        let mut out = String::with_capacity(proj4.len() + 96);
        for token in proj4.split_whitespace() {
            let (key, val) = token
                .strip_prefix('+')
                .and_then(|t| t.split_once('='))
                .unwrap_or(("", ""));
            let rewritten = match key {
                "k_0" if !has_k => Some(format!("+k={val}")),
                "ellps" => MINOR_AXIS_ELLIPSOIDS
                    .iter()
                    .find(|(name, _, _)| *name == val)
                    .map(|&(_, a, b)| format!("+a={a} +rf={}", a / (a - b))),
                "b" => match (semi_major, val.parse::<f64>()) {
                    (Some(a), Ok(b)) if b < a => Some(format!("+rf={}", a / (a - b))),
                    _ => None,
                },
                "pm" => prime_meridian_deg(val).map(|deg| format!("+pm={}", deg.to_radians())),
                _ => None,
            };
            out.push_str(rewritten.as_deref().unwrap_or(token));
            out.push(' ');
        }
        out.push_str(&self.towgs84_clause());
        out
    }
}

/// Ellipsoids that proj4rs stores by semi-major and semi-minor axis, in
/// metres: `(proj4 name, a, b)`.
const MINOR_AXIS_ELLIPSOIDS: [(&str, f64, f64); 6] = [
    ("mod_airy", 6_377_340.189, 6_356_034.446),
    ("clrk66", 6_378_206.4, 6_356_583.8),
    ("new_intl", 6_378_157.5, 6_356_772.2),
    ("plessis", 6_376_523.0, 6_355_863.0),
    ("SEasia", 6_378_155.0, 6_356_773.320_5),
    ("walbeck", 6_376_896.0, 6_355_834.846_7),
];

/// Longitude of a proj4 prime meridian east of Greenwich, in degrees. The
/// value is a name from the proj4 table or a number of degrees.
fn prime_meridian_deg(pm: &str) -> Option<f64> {
    const NAMED: [(&str, f64); 13] = [
        ("lisbon", -9.131_906_111_111),
        ("paris", 2.337_229_166_667),
        ("bogota", -74.080_916_666_667),
        ("madrid", -3.687_938_888_889),
        ("rome", 12.452_333_333_333),
        ("bern", 7.439_583_333_333),
        ("jakarta", 106.807_719_444_444),
        ("ferro", -17.666_666_666_667),
        ("brussels", 4.367_975),
        ("stockholm", 18.058_277_777_778),
        ("athens", 23.716_337_5),
        ("oslo", 10.722_916_666_667),
        ("copenhagen", 12.577_88),
    ];
    NAMED
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(pm))
        .map(|&(_, deg)| deg)
        .or_else(|| pm.parse::<f64>().ok())
}

/// What the catalogue definition of a CRS needs to reach WGS 84.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DatumShiftRule {
    /// The catalogue definition is complete: it is on GRS80 or WGS 84, or it
    /// names its own datum.
    NotNeeded,
    /// The definition needs this Helmert shift.
    Helmert(&'static DatumShift),
    /// The definition needs a shift and no static Helmert transformation to
    /// WGS 84 is published for it.
    Unavailable,
}

/// Look up the datum shift rule for an EPSG CRS code.
pub(crate) fn datum_shift_rule(epsg: u16) -> DatumShiftRule {
    if let Ok(i) = CRS_OPERATIONS.binary_search_by_key(&epsg, |&(code, _)| code) {
        return DatumShiftRule::Helmert(&OPERATIONS[usize::from(CRS_OPERATIONS[i].1)]);
    }
    if CRS_WITHOUT_SHIFT.binary_search(&epsg).is_ok() {
        return DatumShiftRule::Unavailable;
    }
    DatumShiftRule::NotNeeded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `true` when proj4rs builds `proj4` with no datum on an ellipsoid that
    /// is not GRS80 or WGS 84. Mirrors `needs_datum_shift` in
    /// `tools/gen_datum_shifts.py`.
    fn needs_datum_shift(proj4: &str) -> bool {
        let value = |key: &str| {
            proj4
                .split_whitespace()
                .find_map(|t| t.strip_prefix('+')?.strip_prefix(key)?.strip_prefix('='))
        };
        if value("proj").is_none() {
            return false;
        }
        if ["towgs84", "datum", "nadgrids"]
            .iter()
            .any(|k| value(k).is_some())
        {
            return false;
        }
        match value("ellps") {
            Some("GRS80" | "WGS84") => false,
            Some(_) => true,
            None => {
                let Some(a) = value("a").and_then(|v| v.parse::<f64>().ok()) else {
                    return true;
                };
                let es = if let Some(rf) = value("rf").and_then(|v| v.parse::<f64>().ok()) {
                    let f = 1.0 / rf;
                    f * (2.0 - f)
                } else if let Some(b) = value("b").and_then(|v| v.parse::<f64>().ok()) {
                    1.0 - (b * b) / (a * a)
                } else {
                    return true;
                };
                !(a == 6_378_137.0 && (es - 0.006_694_379_990_141_316_5).abs() < 0.000_000_000_050)
            }
        }
    }

    #[test]
    fn tables_are_sorted_for_binary_search() {
        assert!(CRS_OPERATIONS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(CRS_WITHOUT_SHIFT.windows(2).all(|w| w[0] < w[1]));
        assert!(
            CRS_OPERATIONS
                .iter()
                .all(|&(_, i)| usize::from(i) < OPERATIONS.len())
        );
    }

    #[test]
    fn every_catalogue_definition_that_needs_a_shift_has_a_rule() {
        // Guards the generated table against a catalogue upgrade: a definition
        // that needs a shift must be in exactly one of the two lists, and a
        // definition that needs none must be in neither.
        let mut shifted = 0;
        let mut refused = 0;
        for code in 0..=u16::MAX {
            let Some(def) = crs_definitions::from_code(code) else {
                assert_eq!(
                    datum_shift_rule(code),
                    DatumShiftRule::NotNeeded,
                    "EPSG:{code} is not in the catalogue but has a rule"
                );
                continue;
            };
            let rule = datum_shift_rule(code);
            if needs_datum_shift(def.proj4) {
                match rule {
                    DatumShiftRule::Helmert(_) => shifted += 1,
                    DatumShiftRule::Unavailable => refused += 1,
                    DatumShiftRule::NotNeeded => {
                        panic!(
                            "EPSG:{code} needs a datum shift and has no rule: {}",
                            def.proj4
                        )
                    }
                }
            } else {
                assert_eq!(
                    rule,
                    DatumShiftRule::NotNeeded,
                    "EPSG:{code} needs no datum shift but has a rule: {}",
                    def.proj4
                );
            }
        }
        assert_eq!(shifted, CRS_OPERATIONS.len());
        assert_eq!(refused, CRS_WITHOUT_SHIFT.len());
    }

    fn shift_for(epsg: u16) -> &'static DatumShift {
        match datum_shift_rule(epsg) {
            DatumShiftRule::Helmert(shift) => shift,
            other => panic!("EPSG:{epsg} must have a Helmert shift, got {other:?}"),
        }
    }

    #[test]
    fn definition_with_shift_appends_the_towgs84_clause() {
        let shift = shift_for(27700);
        assert_eq!(shift.operation_id, "EPSG:1314");
        assert_eq!(
            shift.definition_with_shift("+proj=tmerc +lat_0=49 +ellps=airy +units=m +no_defs"),
            "+proj=tmerc +lat_0=49 +ellps=airy +units=m +no_defs \
             +towgs84=446.448,-125.157,542.06,0.15,0.247,0.842,-20.489"
        );
    }

    #[test]
    fn definition_with_shift_restates_what_proj4rs_misreads() {
        let shift = shift_for(27700);
        let towgs84 = "+towgs84=446.448,-125.157,542.06,0.15,0.247,0.842,-20.489";
        // The scale factor moves from `+k_0=` to `+k=`.
        assert_eq!(
            shift.definition_with_shift("+proj=somerc +k_0=0.99993 +ellps=GRS67"),
            format!("+proj=somerc +k=0.99993 +ellps=GRS67 {towgs84}")
        );
        // A named minor-axis ellipsoid becomes `+a=` and `+rf=`.
        assert_eq!(
            shift.definition_with_shift("+proj=utm +zone=36 +south +ellps=clrk66"),
            format!("+proj=utm +zone=36 +south +a=6378206.4 +rf=294.9786982138982 {towgs84}")
        );
        // An explicit minor axis becomes `+rf=`.
        assert_eq!(
            shift.definition_with_shift("+proj=tmerc +a=6378300.789 +b=6356566.435"),
            format!("+proj=tmerc +a=6378300.789 +rf=293.4663155389811 {towgs84}")
        );
        // A prime meridian is restated in radians.
        assert_eq!(
            shift.definition_with_shift("+proj=lcc +ellps=clrk80ign +pm=paris"),
            format!("+proj=lcc +ellps=clrk80ign +pm=0.04079234433198245 {towgs84}")
        );
    }
}
