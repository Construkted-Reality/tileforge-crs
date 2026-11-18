// ABOUTME: Selects the Helmert datum shift to WGS 84 that a catalogue CRS needs.
// ABOUTME: Refuses a CRS whose datum has no published static Helmert transformation.
//! Datum shifts for catalogue definitions that carry no datum.
//!
//! The proj4 catalogue (`crs-definitions`) describes many CRSs by projection
//! and ellipsoid only, and it carries no datum shift.
//!
//! - For an ellipsoid other than GRS80 or WGS 84 (Airy, Bessel,
//!   International 1924, Krassowsky and so on) the datum is offset from
//!   WGS 84 by tens to hundreds of metres.
//! - For a GRS80 or WGS 84 ellipsoid the datum is usually within about 2 m
//!   of WGS 84 (NAD83, ETRS89, GDA94). A few are not: GGRS87 (Greece) is
//!   326 m away, VN-2000 (Vietnam) 225 m, Israel 1993 78 m. Those are in
//!   the table. Known limit: EPSG:6988 to 6991 (IG05/12 and Israeli Grid
//!   05/12) are 78 m away and are not in the table, because PROJ states
//!   their operation as an inverse Helmert step, which a `+towgs84` clause
//!   cannot express exactly.
//!
//! This module supplies the missing shift from a generated table of Helmert
//! transformations (`datum_shift_table`). Each CRS in the table maps to the
//! first static Helmert operation in the ranking that PROJ makes, without
//! grid files, for the EPSG area of use of that CRS. PROJ also ranks an
//! operation that covers only a part of that area.
//!
//! A CRS on a foreign ellipsoid that has no verified operation is refused:
//! its position on WGS 84 cannot be derived. A CRS on GRS80 or WGS 84 is in
//! the table only when the first operation in the PROJ ranking is a Helmert
//! shift that moves the horizontal position by more than 5 m, and it is
//! never refused.
//! A definition on a sphere has no geodetic datum; its coordinates are read
//! as WGS 84 values, as PROJ does.

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
    /// joined with `+`, for example `"EPSG:5826+EPSG:1149"`. A few operations
    /// come from another registry in the PROJ database, for example `IGNF:`.
    pub operation_id: &'static str,
    /// Name of the coordinate operation, for example
    /// `"OSGB36 to WGS 84 (6)"`.
    pub operation_name: &'static str,
    /// Accuracy in metres that the registry publishes for the operation, or
    /// `None` when it states none. For two combined operations it is the sum
    /// of both. It holds inside the area of the operation, which can be
    /// smaller than the area of use of the CRS.
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
    /// The catalogue definition is used as it is: it names its own datum, it
    /// is on a sphere, or it is on GRS80 or WGS 84 with no first-ranked
    /// Helmert shift that moves the horizontal position by more than 5 m.
    NotNeeded,
    /// The definition needs this Helmert shift.
    Helmert(&'static DatumShift),
    /// The definition needs a shift and no verified static Helmert
    /// transformation to WGS 84 is available for it.
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

    #[derive(Debug, PartialEq)]
    enum ShiftClass {
        /// proj4rs builds the definition with its own datum, it has no
        /// projection, or it is on a sphere.
        None,
        /// No datum, GRS80 or WGS 84 ellipsoid.
        Wgs84,
        /// No datum, any other ellipsoid.
        Foreign,
    }

    /// Mirrors `shift_class` in `tools/gen_datum_shifts.py`.
    fn shift_class(proj4: &str) -> ShiftClass {
        let value = |key: &str| {
            proj4
                .split_whitespace()
                .find_map(|t| t.strip_prefix('+')?.strip_prefix(key)?.strip_prefix('='))
        };
        let number = |key: &str| value(key).and_then(|v| v.parse::<f64>().ok());
        if value("proj").is_none()
            || ["towgs84", "datum", "nadgrids"]
                .iter()
                .any(|k| value(k).is_some())
        {
            return ShiftClass::None;
        }
        let sphere = value("R").is_some()
            || value("ellps") == Some("sphere")
            || matches!((number("a"), number("b")), (Some(a), Some(b)) if a == b);
        if sphere {
            return ShiftClass::None;
        }
        let wgs84 = match value("ellps") {
            Some("GRS80" | "WGS84") => true,
            Some(_) => false,
            None => match (number("a"), number("rf"), number("b")) {
                (Some(a), Some(rf), _) => {
                    let f = 1.0 / rf;
                    is_wgs84(a, f * (2.0 - f))
                }
                (Some(a), None, Some(b)) => is_wgs84(a, 1.0 - (b * b) / (a * a)),
                _ => false,
            },
        };
        if wgs84 {
            ShiftClass::Wgs84
        } else {
            ShiftClass::Foreign
        }
    }

    /// The test that proj4rs uses to treat an ellipsoid as WGS 84.
    fn is_wgs84(a: f64, es: f64) -> bool {
        a == 6_378_137.0 && (es - 0.006_694_379_990_141_316_5).abs() < 0.000_000_000_050
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
    fn every_catalogue_definition_has_the_rule_of_its_class() {
        // Guards the generated table against a catalogue upgrade. A definition
        // on a foreign ellipsoid must be in exactly one of the two lists. A
        // definition on GRS80 or WGS 84 can be in the table. Every other
        // definition must be in neither list.
        let mut shifted = 0;
        let mut refused = 0;
        for code in 0..=u16::MAX {
            let rule = datum_shift_rule(code);
            let class = crs_definitions::from_code(code)
                .map_or(ShiftClass::None, |def| shift_class(def.proj4));
            match rule {
                DatumShiftRule::Helmert(_) => shifted += 1,
                DatumShiftRule::Unavailable => refused += 1,
                DatumShiftRule::NotNeeded => {}
            }
            match class {
                ShiftClass::Foreign => assert_ne!(
                    rule,
                    DatumShiftRule::NotNeeded,
                    "EPSG:{code} is on a foreign ellipsoid and has no rule"
                ),
                ShiftClass::Wgs84 => assert_ne!(
                    rule,
                    DatumShiftRule::Unavailable,
                    "EPSG:{code} is on GRS80 or WGS 84 and must not be refused"
                ),
                ShiftClass::None => assert_eq!(
                    rule,
                    DatumShiftRule::NotNeeded,
                    "EPSG:{code} needs no datum shift but has a rule"
                ),
            }
        }
        assert_eq!(shifted, CRS_OPERATIONS.len());
        assert_eq!(refused, CRS_WITHOUT_SHIFT.len());
    }

    #[test]
    fn reference_fixture_covers_every_crs_in_the_table() {
        // `tests/datum_shift_parity.rs` proves a CRS only if the fixture has
        // rows for it.
        let fixture = include_str!("../tests/fixtures/datum-shift-reference.txt");
        let mut in_fixture: Vec<u16> = fixture
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .map(|line| line.split_whitespace().next().unwrap().parse().unwrap())
            .collect();
        in_fixture.dedup();
        let in_table: Vec<u16> = CRS_OPERATIONS.iter().map(|&(code, _)| code).collect();
        assert_eq!(in_fixture, in_table);
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
