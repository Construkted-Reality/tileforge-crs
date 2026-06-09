//! Minimal OGC WKT scanner — extracts the outermost EPSG `AUTHORITY`
//! clause from a CRS WKT string.
//!
//! Not a full WKT parser. The strategy is bracket-balanced substring
//! scanning, which is sufficient for the Phase 1.1 deliverable: pull the
//! EPSG code out of a real-world LAS-1.4 WKT VLR (which always carries
//! an `AUTHORITY["EPSG","NNNN"]` clause when produced by PDAL, lastools,
//! QGIS, or GDAL).
//!
//! Supported root blocks — both **WKT1** and **WKT2** (LAS 1.4 producers
//! increasingly emit WKT2):
//! - WKT1: `PROJCS`, `GEOGCS`, `GEOCCS`, `COMPD_CS`.
//! - WKT2: `PROJCRS`, `GEOGCRS`, `GEODCRS`, `COMPOUNDCRS`, `BOUNDCRS`.
//!
//! The EPSG code is read from the outermost authority clause, which is
//! `AUTHORITY["EPSG","NNNN"]` in WKT1 and `ID["EPSG",NNNN]` (code often
//! **unquoted**) in WKT2. For a compound CRS the scanner descends into the
//! first horizontal subblock and reports the vertical component as stripped
//! via `WktExtraction::vertical_stripped` — the caller decides what to do
//! (PC ignores, mesh warns). For a `BOUNDCRS` (a CRS bundled with a datum
//! transform) it descends into `SOURCECRS` and recurses.

use crate::error::CrsError;

/// Result of a successful WKT scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WktExtraction {
    pub epsg: u16,
    /// `true` iff the input was a `COMPD_CS[...]` and the vertical
    /// component was discarded.
    pub vertical_stripped: bool,
}

/// Extract the EPSG code from a CRS WKT string.
pub fn extract_epsg_from_wkt(wkt: &str) -> Result<WktExtraction, CrsError> {
    let bytes = wkt.as_bytes();
    let after_ws = skip_whitespace(bytes, 0);
    let (id, after_id) = read_identifier(bytes, after_ws).ok_or_else(|| {
        CrsError::parse("WKT VLR is empty or has no leading identifier".to_string())
    })?;
    let id_upper = id.to_ascii_uppercase();
    let after_open = skip_whitespace(bytes, after_id);
    if after_open >= bytes.len() || bytes[after_open] != b'[' {
        return Err(CrsError::parse(format!(
            "WKT VLR identifier '{id}' is not followed by '['"
        )));
    }
    let close_idx = find_matching_bracket(bytes, after_open).ok_or_else(|| {
        CrsError::parse(format!("WKT VLR identifier '{id}' has unbalanced brackets"))
    })?;
    let body = &wkt[after_open + 1..close_idx];

    match id_upper.as_str() {
        // Compound CRS (WKT1 COMPD_CS / WKT2 COMPOUNDCRS): take the first
        // horizontal subblock, mark the vertical component stripped.
        "COMPD_CS" | "COMPOUNDCRS" => {
            let horizontal = find_child_block(body, HORIZONTAL_KEYWORDS).ok_or_else(|| {
                CrsError::parse(
                    "compound CRS contains no horizontal (PROJCS/GEOGCS/GEOCCS/\
                     PROJCRS/GEOGCRS/GEODCRS) subblock to extract EPSG from"
                        .to_string(),
                )
            })?;
            let inner = extract_epsg_from_wkt(horizontal)?;
            Ok(WktExtraction {
                epsg: inner.epsg,
                vertical_stripped: true,
            })
        }
        // WKT2 BOUNDCRS wraps the real CRS in SOURCECRS (plus a datum
        // transform to a target). The EPSG we want is the source CRS's.
        "BOUNDCRS" => {
            let source = find_child_block(body, &["SOURCECRS"])
                .ok_or_else(|| CrsError::parse("BOUNDCRS has no SOURCECRS subblock".to_string()))?;
            // `source` is `SOURCECRS[<crs>]`; its body is the wrapped CRS.
            let inner_crs = block_body(source)
                .ok_or_else(|| CrsError::parse("BOUNDCRS SOURCECRS is malformed".to_string()))?;
            extract_epsg_from_wkt(inner_crs.trim())
        }
        // Simple CRS that directly carries the authority/ID clause.
        // WKT1: PROJCS/GEOGCS/GEOCCS. WKT2: PROJCRS/GEOGCRS/GEODCRS.
        "PROJCS" | "GEOGCS" | "GEOCCS" | "PROJCRS" | "GEOGCRS" | "GEODCRS" => {
            let epsg = parse_epsg_authority_in_body(body)?;
            Ok(WktExtraction {
                epsg,
                vertical_stripped: false,
            })
        }
        _ => Err(CrsError::parse(format!(
            "WKT VLR root is unexpected '{id}' (want a WKT1 PROJCS/GEOGCS/GEOCCS/COMPD_CS \
             or WKT2 PROJCRS/GEOGCRS/GEODCRS/COMPOUNDCRS/BOUNDCRS)"
        ))),
    }
}

/// Horizontal-CRS root keywords, WKT1 and WKT2. Used to find the
/// horizontal component of a compound CRS.
const HORIZONTAL_KEYWORDS: &[&str] = &[
    "PROJCS", "GEOGCS", "GEOCCS", // WKT1
    "PROJCRS", "GEOGCRS", "GEODCRS", // WKT2
];

/// Given a `KEYWORD[...]` substring, return the slice between its outer
/// brackets (the block body), or `None` if malformed.
fn block_body(block: &str) -> Option<&str> {
    let bytes = block.as_bytes();
    let open = bytes.iter().position(|&b| b == b'[')?;
    let close = find_matching_bracket(bytes, open)?;
    Some(&block[open + 1..close])
}

fn skip_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn read_identifier(bytes: &[u8], start: usize) -> Option<(&str, usize)> {
    let mut end = start;
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_alphanumeric() || c == b'_' {
            end += 1;
        } else {
            break;
        }
    }
    if end == start {
        return None;
    }
    // Safe: identifier characters are all ASCII.
    Some((std::str::from_utf8(&bytes[start..end]).unwrap(), end))
}

/// Given `bytes[open_idx] == b'['`, return the index of the matching
/// closing `b']'`. Respects WKT-quoted strings (`"..."`) so brackets
/// inside CRS names don't mis-count.
fn find_matching_bracket(bytes: &[u8], open_idx: usize) -> Option<usize> {
    debug_assert_eq!(bytes[open_idx], b'[');
    let mut depth: i32 = 0;
    let mut i = open_idx;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'"' {
                // WKT escapes a literal quote as `""`. Peek ahead.
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    i += 2;
                    continue;
                }
                in_string = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Walk the depth-0 tokens of `body` (the contents of a block, excluding
/// its outer brackets) and return the substring of the first child block
/// whose identifier (case-insensitive) is in `keywords`.
fn find_child_block<'a>(body: &'a str, keywords: &[&str]) -> Option<&'a str> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            // Skip quoted string.
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'"' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphanumeric() || c == b'_' {
            let (id, after_id) = read_identifier(bytes, i)?;
            let after_ws = skip_whitespace(bytes, after_id);
            if after_ws < bytes.len() && bytes[after_ws] == b'[' {
                let close = find_matching_bracket(bytes, after_ws)?;
                if keywords.iter().any(|k| id.eq_ignore_ascii_case(k)) {
                    return Some(&body[i..=close]);
                }
                // Skip past this non-matching block and continue scanning.
                i = close + 1;
                continue;
            }
            i = after_id;
            continue;
        }
        i += 1;
    }
    None
}

/// Scan `body` (the contents of a PROJCS/GEOGCS/GEOCCS block, excluding
/// its outer brackets) for an `AUTHORITY[...]` at depth 0. Return the
/// EPSG u16 code from inside it.
fn parse_epsg_authority_in_body(body: &str) -> Result<u16, CrsError> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'"' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphanumeric() || c == b'_' {
            let Some((id, after_id)) = read_identifier(bytes, i) else {
                i += 1;
                continue;
            };
            let after_ws = skip_whitespace(bytes, after_id);
            if after_ws < bytes.len() && bytes[after_ws] == b'[' {
                let Some(close) = find_matching_bracket(bytes, after_ws) else {
                    return Err(CrsError::parse(
                        "unbalanced brackets while scanning for AUTHORITY".to_string(),
                    ));
                };
                // WKT1 `AUTHORITY["EPSG","NNNN"]` or WKT2 `ID["EPSG",NNNN]`.
                if id.eq_ignore_ascii_case("AUTHORITY") || id.eq_ignore_ascii_case("ID") {
                    let inner = &body[after_ws + 1..close];
                    return parse_authority_args(inner);
                }
                i = close + 1;
                continue;
            }
            i = after_id;
            continue;
        }
        i += 1;
    }
    Err(CrsError::parse(
        "WKT VLR has no EPSG AUTHORITY/ID clause; pre-process with `pdal translate` to inject one"
            .to_string(),
    ))
}

/// Parse the contents of an `AUTHORITY`/`ID` block. Handles both WKT1
/// `"EPSG","NNNN"` (code quoted) and WKT2 `"EPSG",NNNN` (code unquoted),
/// and tolerates extra WKT2 trailing args (e.g. `…,VERSION[...]` /
/// `…,URI[...]`) by reading only the first two.
fn parse_authority_args(inner: &str) -> Result<u16, CrsError> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() < 2 {
        return Err(CrsError::parse(format!(
            "AUTHORITY/ID clause must have at least two arguments, got {} ({inner:?})",
            parts.len()
        )));
    }
    let authority = strip_quotes(parts[0])?;
    // WKT1 quotes the code; WKT2 leaves it bare. Accept either.
    let code_str = strip_quotes_opt(parts[1]);
    if !authority.eq_ignore_ascii_case("EPSG") {
        return Err(CrsError::parse(format!(
            "WKT VLR uses non-EPSG authority '{authority}'; only EPSG is supported"
        )));
    }
    code_str.parse::<u16>().map_err(|e| {
        CrsError::parse(format!(
            "AUTHORITY/ID code is not a valid EPSG u16: {code_str:?}: {e}"
        ))
    })
}

/// Strip surrounding double-quotes if present; otherwise return the
/// trimmed input unchanged (WKT2 numeric codes are unquoted).
fn strip_quotes_opt(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn strip_quotes(s: &str) -> Result<&str, CrsError> {
    let s = s.trim();
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return Err(CrsError::parse(format!(
            "AUTHORITY argument is not quoted: {s:?}"
        )));
    }
    Ok(&s[1..s.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINGSTON_WKT: &str = include_str!("../tests/fixtures/kingston-rd.wkt");
    const FABRE_WKT: &str = include_str!("../tests/fixtures/fabregualta.wkt");
    const COMPD_WKT: &str = include_str!("../tests/fixtures/compd-cs-with-vert.wkt");
    const ECEF_WKT: &str = include_str!("../tests/fixtures/epsg-4978.wkt");

    #[test]
    fn projcs_kingston_yields_32617_no_vertical() {
        let r = extract_epsg_from_wkt(KINGSTON_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 32617,
                vertical_stripped: false
            }
        );
    }

    #[test]
    fn projcs_fabregualta_yields_32719() {
        let r = extract_epsg_from_wkt(FABRE_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 32719,
                vertical_stripped: false
            }
        );
    }

    #[test]
    fn geoccs_ecef_wkt_yields_4978() {
        let r = extract_epsg_from_wkt(ECEF_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 4978,
                vertical_stripped: false
            }
        );
    }

    #[test]
    fn compd_cs_returns_horizontal_epsg_and_strips_vertical() {
        let r = extract_epsg_from_wkt(COMPD_WKT).unwrap();
        assert_eq!(
            r,
            WktExtraction {
                epsg: 32617,
                vertical_stripped: true
            }
        );
    }

    #[test]
    fn projcs_with_no_authority_is_crs_error() {
        let wkt = r#"PROJCS["fake", GEOGCS["wgs",DATUM["d",SPHEROID["s",6378137,298.257]]], PROJECTION["tm"], UNIT["m",1]]"#;
        let err = extract_epsg_from_wkt(wkt).unwrap_err();
        let CrsError::Parse(msg) = &err else {
            panic!("want Parse got {err:?}");
        };
        assert!(msg.to_lowercase().contains("authority"), "msg: {msg}");
    }

    #[test]
    fn projcs_with_non_epsg_authority_is_crs_error() {
        let wkt = r#"PROJCS["esri thing", AUTHORITY["ESRI","102100"]]"#;
        let err = extract_epsg_from_wkt(wkt).unwrap_err();
        let CrsError::Parse(msg) = &err else {
            panic!("want Parse got {err:?}");
        };
        assert!(msg.to_lowercase().contains("esri"), "msg: {msg}");
    }

    #[test]
    fn empty_string_is_crs_error() {
        assert!(matches!(
            extract_epsg_from_wkt("").unwrap_err(),
            CrsError::Parse(_)
        ));
    }

    #[test]
    fn unbalanced_brackets_is_crs_error() {
        assert!(matches!(
            extract_epsg_from_wkt(r#"PROJCS["x", AUTHORITY["EPSG","32617""#).unwrap_err(),
            CrsError::Parse(_)
        ));
    }

    #[test]
    fn unknown_root_identifier_is_crs_error() {
        assert!(matches!(
            extract_epsg_from_wkt(r#"VERT_CS["only vertical", AUTHORITY["EPSG","5773"]]"#)
                .unwrap_err(),
            CrsError::Parse(_)
        ));
    }

    #[test]
    fn whitespace_and_lowercase_are_tolerated() {
        let wkt = r#"
            projcs [ "WGS 84 / UTM zone 17N",
                geogcs [ "WGS 84",
                    authority [ "EPSG", "4326" ] ],
                authority [ "EPSG" , "32617" ]
            ]
        "#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 32617);
    }

    // ---- WKT2 (ISO 19162) ----

    #[test]
    fn wkt2_geogcrs_unquoted_id_yields_4326() {
        // WKT2 geographic: root GEOGCRS, code unquoted in ID[].
        let wkt = r#"GEOGCRS["WGS 84",
            DATUM["World Geodetic System 1984",
                ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1]]],
            CS[ellipsoidal,2],
                AXIS["geodetic latitude (Lat)",north],
                AXIS["geodetic longitude (Lon)",east],
                ANGLEUNIT["degree",0.0174532925199433],
            ID["EPSG",4326]]"#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 4326);
        assert!(!r.vertical_stripped);
    }

    #[test]
    fn wkt2_geodcrs_nad83_yields_4269() {
        let wkt = r#"GEODCRS["NAD83",
            DATUM["North American Datum 1983",
                ELLIPSOID["GRS 1980",6378137,298.257222101]],
            CS[ellipsoidal,2],AXIS["lat",north],AXIS["lon",east],
            ID["EPSG",4269]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 4269);
    }

    #[test]
    fn wkt2_projcrs_takes_projected_id_not_base() {
        // The outermost ID (32617) is the projected code; the nested
        // BASEGEOGCRS ID (4326) must NOT shadow it.
        let wkt = r#"PROJCRS["WGS 84 / UTM zone 17N",
            BASEGEOGCRS["WGS 84",
                DATUM["World Geodetic System 1984",
                    ELLIPSOID["WGS 84",6378137,298.257223563]],
                ID["EPSG",4326]],
            CONVERSION["UTM zone 17N",METHOD["Transverse Mercator",ID["EPSG",9807]]],
            CS[Cartesian,2],AXIS["(E)",east],AXIS["(N)",north],
            LENGTHUNIT["metre",1],
            ID["EPSG",32617]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
    }

    #[test]
    fn wkt2_compoundcrs_strips_vertical() {
        let wkt = r#"COMPOUNDCRS["NAD83 / UTM 17N + NAVD88",
            PROJCRS["NAD83 / UTM zone 17N",
                BASEGEOGCRS["NAD83",DATUM["NAD83",ELLIPSOID["GRS 1980",6378137,298.257222101]],ID["EPSG",4269]],
                CONVERSION["x",METHOD["Transverse Mercator"]],
                CS[Cartesian,2],AXIS["e",east],AXIS["n",north],ID["EPSG",26917]],
            VERTCRS["NAVD88",VDATUM["North American Vertical Datum 1988"],
                CS[vertical,1],AXIS["up",up],ID["EPSG",5703]]]"#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 26917);
        assert!(r.vertical_stripped);
    }

    #[test]
    fn wkt2_boundcrs_descends_into_sourcecrs() {
        let wkt = r#"BOUNDCRS[
            SOURCECRS[GEOGCRS["unknown",DATUM["d",ELLIPSOID["GRS 1980",6378137,298.257222101]],
                CS[ellipsoidal,2],AXIS["lat",north],AXIS["lon",east],ID["EPSG",4269]]],
            TARGETCRS[GEOGCRS["WGS 84",DATUM["WGS84",ELLIPSOID["WGS 84",6378137,298.257223563]],ID["EPSG",4326]]],
            ABRIDGEDTRANSFORMATION["NAD83 to WGS84",METHOD["Geocentric translations"],
                PARAMETER["X",0,LENGTHUNIT["metre",1]]]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 4269);
    }

    #[test]
    fn wkt2_id_with_trailing_uri_arg_is_tolerated() {
        let wkt = r#"GEOGCRS["WGS 84",DATUM["d",ELLIPSOID["WGS 84",6378137,298.257223563]],
            CS[ellipsoidal,2],AXIS["lat",north],AXIS["lon",east],
            ID["EPSG",4326,URI["http://www.opengis.net/def/crs/EPSG/0/4326"]]]"#;
        assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 4326);
    }

    #[test]
    fn quoted_brackets_in_name_do_not_mis_count() {
        // CRS name happens to contain a `]` inside a quoted string;
        // bracket counting must not treat it as a closer.
        let wkt = r#"PROJCS["weird]name", AUTHORITY["EPSG","32617"]]"#;
        let r = extract_epsg_from_wkt(wkt).unwrap();
        assert_eq!(r.epsg, 32617);
    }
}
