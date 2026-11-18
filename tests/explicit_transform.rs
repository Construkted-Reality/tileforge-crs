use tileforge_crs::{extract_epsg_from_wkt, parse_crs_string};

#[test]
fn bound_transform_is_not_silently_discarded() {
    let wkt = include_str!("fixtures/bound-shift.wkt");
    let err = extract_epsg_from_wkt(wkt).unwrap_err();
    assert!(err.to_string().contains("BOUNDCRS"));
    assert!(parse_crs_string(wkt).is_err());
}

#[test]
fn towgs84_is_not_silently_discarded() {
    for operation in ["TOWGS84[100,200,300]", "towgs84 [0,0,0,0,0,0,1]"] {
        let wkt =
            format!("GEOGCS[\"custom\",DATUM[\"datum\",{operation}],AUTHORITY[\"EPSG\",\"4269\"]]");
        let err = extract_epsg_from_wkt(&wkt).unwrap_err();
        assert!(err.to_string().contains("TOWGS84"));
    }
}

/// A WKT1 projected CRS as GDAL 2 wrote it, with `datum_shift` as the
/// `TOWGS84` clause of its datum and `epsg` as its code.
fn projcs_with_towgs84(datum_shift: &str, epsg: u16) -> String {
    format!(
        "PROJCS[\"p\",GEOGCS[\"g\",DATUM[\"d\",SPHEROID[\"GRS 1980\",6378137,298.257222101,\
         AUTHORITY[\"EPSG\",\"7019\"]],{datum_shift},AUTHORITY[\"EPSG\",\"6152\"]],\
         PRIMEM[\"Greenwich\",0],UNIT[\"degree\",0.0174532925199433],\
         AUTHORITY[\"EPSG\",\"4152\"]],PROJECTION[\"Lambert_Conformal_Conic_2SP\"],\
         UNIT[\"foot\",0.3048],AUTHORITY[\"EPSG\",\"{epsg}\"]]"
    )
}

#[test]
fn zero_towgs84_is_accepted_for_a_datum_that_needs_no_shift() {
    // EPSG:2992 is NAD83(HARN) / Oregon Lambert (ft), on GRS80. The catalogue
    // applies no shift to it, which is what a zero TOWGS84 declares.
    for zero in [
        "TOWGS84[0,0,0,0,0,0,0]",
        "TOWGS84[0,0,0]",
        "towgs84 [ 0.0, 0.000, -0, 0e0, 0, 0, 0 ]",
    ] {
        let wkt = projcs_with_towgs84(zero, 2992);
        let extraction = extract_epsg_from_wkt(&wkt).unwrap_or_else(|e| panic!("{zero}: {e}"));
        assert_eq!(extraction.epsg, 2992, "{zero}");
        assert!(!extraction.vertical_stripped);
        assert_eq!(parse_crs_string(&wkt).unwrap().epsg, 2992, "{zero}");
    }
    // Codes that name their datum (NAD83, WGS 84), ETRS89 on GRS80, and
    // Web Mercator (null grid).
    for epsg in [4269_u16, 26917, 4326, 32617, 25832, 3857] {
        let wkt = projcs_with_towgs84("TOWGS84[0,0,0,0,0,0,0]", epsg);
        assert_eq!(extract_epsg_from_wkt(&wkt).unwrap().epsg, epsg);
    }
}

#[test]
fn zero_towgs84_in_the_horizontal_leg_of_a_compound_is_accepted() {
    let wkt = format!(
        "COMPD_CS[\"c\",{},VERT_CS[\"v\",AUTHORITY[\"EPSG\",\"5703\"]]]",
        projcs_with_towgs84("TOWGS84[0,0,0,0,0,0,0]", 2992)
    );
    let extraction = extract_epsg_from_wkt(&wkt).unwrap();
    assert_eq!(extraction.epsg, 2992);
    assert!(extraction.vertical_stripped);
}

#[test]
fn non_zero_or_malformed_towgs84_stays_rejected() {
    for operation in [
        "TOWGS84[0,0,0,0,0,0,0.001]",
        "TOWGS84[-0.991,1.9072,0.5129,0.0258,0.0097,0.0117,0]",
        "TOWGS84[0,0]",
        "TOWGS84[0,0,0,0]",
        "TOWGS84[]",
        "TOWGS84[0,0,zero]",
        "TOWGS84[0,0,0,0,0,0,0,0]",
        "TOWGS84[0,0,NaN]",
    ] {
        let err = extract_epsg_from_wkt(&projcs_with_towgs84(operation, 2992)).unwrap_err();
        assert!(
            err.to_string()
                .contains("TOWGS84 declares an explicit datum transformation"),
            "{operation}: {err}"
        );
    }
}

#[test]
fn zero_towgs84_is_rejected_for_a_code_that_is_not_on_grs80_or_wgs84() {
    // EPSG:27700 (OSGB36, Airy) and EPSG:2327 (Xian 1980, IAG 1975) are on
    // other ellipsoids, EPSG:26717 is NAD27 (grid based), EPSG:2163 is on a
    // sphere, and EPSG:60000 is not in the catalogue. A zero TOWGS84 on
    // these codes cannot be confirmed.
    for epsg in [27700_u16, 2327, 26717, 2163, 60000] {
        let wkt = projcs_with_towgs84("TOWGS84[0,0,0,0,0,0,0]", epsg);
        let message = extract_epsg_from_wkt(&wkt).unwrap_err().to_string();
        assert!(message.contains("TOWGS84"), "EPSG:{epsg}: {message}");
        assert!(
            message.contains("GRS80 or WGS 84"),
            "EPSG:{epsg}: {message}"
        );
        assert!(
            message.contains(&format!("EPSG:{epsg}")),
            "EPSG:{epsg}: {message}"
        );
        assert!(parse_crs_string(&wkt).is_err(), "EPSG:{epsg}");
    }
}

#[test]
fn quoted_operation_names_are_not_operations() {
    let wkt = r#"GEOGCS["BOUNDCRS and ""TOWGS84[1,2,3]""",AUTHORITY["EPSG","4326"]]"#;
    assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 4326);
}

#[test]
fn compound_crs_cannot_hide_a_bound_transform() {
    let wkt = format!(
        "COMPOUNDCRS[\"compound\",{},VERTCRS[\"height\"]]",
        include_str!("fixtures/bound-shift.wkt").trim()
    );
    let err = extract_epsg_from_wkt(&wkt).unwrap_err();
    assert!(err.to_string().contains("BOUNDCRS"));
}

fn projcs_with(inner: &str) -> String {
    format!(
        "PROJCS[\"utm\",GEOGCS[\"wgs\",DATUM[\"d\",SPHEROID[\"s\",6378137,298.257223563],{inner}]],AUTHORITY[\"EPSG\",\"32617\"]]"
    )
}

#[test]
fn towgs84_is_rejected_whatever_follows_the_keyword() {
    for inner in [
        "TOWGS84(-168,-60,320,0,0,0,0)",
        "TOWGS84\u{a0}[-168,-60,320,0,0,0,0]",
        "TOWGS84\u{b}[-168,-60,320,0,0,0,0]",
        "TOWGS84",
    ] {
        let wkt = projcs_with(inner);
        let err = extract_epsg_from_wkt(&wkt).unwrap_err();
        assert!(err.to_string().contains("TOWGS84"), "{inner:?}: {err}");
    }
}

#[test]
fn boundcrs_is_rejected_whatever_follows_the_keyword() {
    for wkt in [
        "BOUNDCRS(SOURCECRS(GEOGCRS(\"g\",ID(\"EPSG\",4326))))",
        "PROJCRS[\"p\",BOUNDCRS\u{a0}[SOURCECRS[]],ID[\"EPSG\",32617]]",
        "PROJCRS[\"p\",boundcrs\u{b}[SOURCECRS[]],ID[\"EPSG\",32617]]",
    ] {
        let err = extract_epsg_from_wkt(wkt).unwrap_err();
        assert!(err.to_string().contains("BOUNDCRS"), "{wkt:?}: {err}");
    }
}

#[test]
fn parenthesis_delimiters_are_rejected() {
    for wkt in [
        r#"PROJCS["x",GEOGCS("g",AUTHORITY["EPSG","4326"]),AUTHORITY["EPSG","32617"]]"#,
        r#"PROJCS("x",AUTHORITY("EPSG","32617"))"#,
        r#"GEOGCS["g",AUTHORITY["EPSG","4326"]])"#,
    ] {
        let err = extract_epsg_from_wkt(wkt).unwrap_err();
        assert!(
            err.to_string().contains("square-bracket delimiters only"),
            "{wkt:?}: {err}"
        );
    }
}

#[test]
fn quoted_parentheses_in_names_are_accepted() {
    let wkt = r#"PROJCS["WGS 84 / UTM zone 17N (""grid"")",GEOGCS["WGS 84 (G1762)",AUTHORITY["EPSG","4326"]],AUTHORITY["EPSG","32617"]]"#;
    assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
}

const HORIZONTAL_LEG: &str = r#"PROJCRS["NAD83 / UTM zone 17N",BASEGEOGCRS["NAD83",DATUM["North American Datum 1983",ELLIPSOID["GRS 1980",6378137,298.257222101]],ID["EPSG",4269]],CS[Cartesian,2],AXIS["e",east],AXIS["n",north],ID["EPSG",26917]]"#;

/// A `BOUNDCRS` vertical leg: source `vertical`, a geographic 3D target and
/// a geoid model operation.
fn bound_vertical_leg(vertical: &str) -> String {
    format!(
        r#"BOUNDCRS[SOURCECRS[{vertical}],TARGETCRS[GEOGCRS["WGS 84",DATUM["World Geodetic System 1984",ELLIPSOID["WGS 84",6378137,298.257223563]],CS[ellipsoidal,3],ID["EPSG",4979]]],ABRIDGEDTRANSFORMATION["geoid",METHOD["GravityRelatedHeight to Geographic3D"],PARAMETERFILE["Geoid (height correction) model file","us_noaa_g2018u0.tif"]]]"#
    )
}

const NAVD88: &str = r#"VERTCRS["NAVD88 height",VDATUM["North American Vertical Datum 1988"],CS[vertical,1],AXIS["up",up],ID["EPSG",5703]]"#;

#[test]
fn compound_with_geoid_bound_vertical_leg_keeps_horizontal_code() {
    let wkt = include_str!("fixtures/compound-geoid-vertical.wkt");
    let r = extract_epsg_from_wkt(wkt).unwrap();
    assert_eq!(r.epsg, 26917);
    assert!(r.vertical_stripped);
    let p = parse_crs_string(wkt.trim()).unwrap();
    assert_eq!(p.epsg, 26917);
    assert!(p.vertical_stripped);
}

#[test]
fn every_vertical_keyword_marks_a_bound_vertical_leg() {
    for vertical in [
        NAVD88.to_string(),
        NAVD88.replacen("VERTCRS", "VERTICALCRS", 1),
        NAVD88.replacen("VERTCRS", "vertcrs", 1),
        r#"VERT_CS["NAVD88 height",VERT_DATUM["NAVD88",2005],AUTHORITY["EPSG","5703"]]"#
            .to_string(),
        format!(" \n{NAVD88}"),
    ] {
        let wkt = format!(
            "COMPOUNDCRS[\"c\",{HORIZONTAL_LEG},{}]",
            bound_vertical_leg(&vertical)
        );
        let r = extract_epsg_from_wkt(&wkt).unwrap_or_else(|e| panic!("{vertical}: {e}"));
        assert_eq!(r.epsg, 26917, "{vertical}");
        assert!(r.vertical_stripped, "{vertical}");
    }
}

#[test]
fn bound_vertical_leg_contents_are_not_scanned_for_operations() {
    let target_with_shift = r#"BOUNDCRS[SOURCECRS[VERTCRS["v"]],TARGETCRS[GEOGCS["g",DATUM["d",TOWGS84[1,2,3]]]],ABRIDGEDTRANSFORMATION["t"]]"#;
    let wkt = format!("COMPOUNDCRS[\"c\",{HORIZONTAL_LEG},{target_with_shift}]");
    assert_eq!(extract_epsg_from_wkt(&wkt).unwrap().epsg, 26917);
}

#[test]
fn horizontal_leg_transform_is_rejected_beside_a_bound_vertical_leg() {
    let bound_horizontal = include_str!("fixtures/bound-shift.wkt").trim();
    let towgs84_horizontal =
        r#"PROJCS["p",GEOGCS["g",DATUM["d",TOWGS84[100,200,300]]],AUTHORITY["EPSG","26917"]]"#;
    for (horizontal, keyword) in [
        (bound_horizontal, "BOUNDCRS"),
        (towgs84_horizontal, "TOWGS84"),
    ] {
        for wkt in [
            format!(
                "COMPOUNDCRS[\"c\",{horizontal},{}]",
                bound_vertical_leg(NAVD88)
            ),
            format!(
                "COMPOUNDCRS[\"c\",{},{horizontal}]",
                bound_vertical_leg(NAVD88)
            ),
        ] {
            let err = extract_epsg_from_wkt(&wkt).unwrap_err();
            assert!(err.to_string().contains(keyword), "{wkt}: {err}");
            assert!(parse_crs_string(&wkt).is_err());
        }
    }
}

#[test]
fn bound_crs_without_a_vertical_source_is_rejected() {
    for leg in [
        // No SOURCECRS child.
        r#"BOUNDCRS[TARGETCRS[GEOGCRS["WGS 84",ID["EPSG",4979]]],ABRIDGEDTRANSFORMATION["t"]]"#
            .to_string(),
        // SOURCECRS holds no identifier.
        r#"BOUNDCRS[SOURCECRS["VERTCRS"],TARGETCRS[GEOGCRS["WGS 84"]]]"#.to_string(),
        // SOURCECRS holds a horizontal CRS.
        r#"BOUNDCRS[SOURCECRS[GEOGCRS["g",ID["EPSG",4269]]],TARGETCRS[GEOGCRS["WGS 84"]]]"#
            .to_string(),
        // A vertical identifier that is not the first child.
        r#"BOUNDCRS[SOURCECRS[GEOGCRS["g"],VERTCRS["v"]],TARGETCRS[GEOGCRS["WGS 84"]]]"#
            .to_string(),
        // Two SOURCECRS children.
        format!(
            r#"BOUNDCRS[SOURCECRS[{NAVD88}],SOURCECRS[GEOGCRS["g"]],TARGETCRS[GEOGCRS["WGS 84"]]]"#
        ),
        // SOURCECRS is not followed by a bracket.
        format!(r#"BOUNDCRS[SOURCECRS {NAVD88},TARGETCRS[GEOGCRS["WGS 84"]]]"#),
    ] {
        let wkt = format!("COMPOUNDCRS[\"c\",{HORIZONTAL_LEG},{leg}]");
        let err = extract_epsg_from_wkt(&wkt).unwrap_err();
        assert!(err.to_string().contains("BOUNDCRS"), "{leg}: {err}");
    }
}

#[test]
fn bound_vertical_leg_with_unbalanced_brackets_is_rejected() {
    let wkt = format!("COMPOUNDCRS[\"c\",{HORIZONTAL_LEG},BOUNDCRS[SOURCECRS[{NAVD88}]");
    let err = extract_epsg_from_wkt(&wkt).unwrap_err();
    assert!(err.to_string().contains("BOUNDCRS"), "{err}");
}

#[test]
fn bound_vertical_crs_at_the_root_is_an_error() {
    let wkt = bound_vertical_leg(NAVD88);
    let msg = extract_epsg_from_wkt(&wkt).unwrap_err().to_string();
    assert!(msg.contains("CRS WKT root"), "{msg}");
    assert!(msg.contains("vertical"), "{msg}");
    assert!(msg.contains("horizontal or compound"), "{msg}");
    assert!(parse_crs_string(&wkt).is_err());
}

#[test]
fn bracket_that_opens_no_identifier_block_is_rejected() {
    for wkt in [
        "PROJCS[\"x\",GEOGCS\u{b}[\"g\",AUTHORITY[\"EPSG\",\"4326\"]],UNIT[\"m\",1]]",
        "PROJCS[\"x\",GEOGCS\u{a0}[\"g\",AUTHORITY[\"EPSG\",\"4326\"]],UNIT[\"m\",1]]",
        r#"PROJCS["x",[AUTHORITY["EPSG","4326"]],UNIT["m",1]]"#,
        r#"[PROJCS["x",AUTHORITY["EPSG","4326"]]]"#,
    ] {
        let msg = extract_epsg_from_wkt(wkt).unwrap_err().to_string();
        assert!(
            msg.contains("'[' that does not open the block of an identifier"),
            "{wkt:?}: {msg}"
        );
        assert!(msg.contains("CRS WKT"), "{wkt:?}: {msg}");
        assert!(parse_crs_string(wkt).is_err());
    }
}

#[test]
fn bracket_that_closes_no_block_is_rejected() {
    for wkt in [
        r#"PROJCS["x",AUTHORITY["EPSG","4326"]]]"#,
        r#"PROJCS["x",UNIT["m",1]],AUTHORITY["EPSG","4326"]]"#,
        r#"]PROJCS["x",AUTHORITY["EPSG","4326"]]"#,
    ] {
        let msg = extract_epsg_from_wkt(wkt).unwrap_err().to_string();
        assert!(msg.contains("']' that closes no block"), "{wkt:?}: {msg}");
    }
}

#[test]
fn quoted_brackets_are_not_blocks() {
    let wkt = r#"PROJCS["[x], ""[y]"" ]",AUTHORITY["EPSG","32617"]]"#;
    assert_eq!(extract_epsg_from_wkt(wkt).unwrap().epsg, 32617);
}

#[test]
fn bound_vertical_block_is_exempt_only_as_a_leg_of_a_compound_root() {
    // The same block nested in a horizontal CRS is not a vertical leg, so
    // the operation inside it must be refused.
    let block = r#"BOUNDCRS[SOURCECRS[VERT_CS["v"]],TOWGS84[100,200,300]]"#;
    let in_projcs = format!(
        "PROJCS[\"p\",GEOGCS[\"g\",DATUM[\"d\",SPHEROID[\"s\",1,2],{block}],\
         AUTHORITY[\"EPSG\",\"4267\"]],AUTHORITY[\"EPSG\",\"26717\"]]"
    );
    let in_compound_leg = format!(
        "COMPOUNDCRS[\"c\",PROJCRS[\"p\",BASEGEOGCRS[\"g\",{block}],ID[\"EPSG\",26917]],\
         VERTCRS[\"v\",ID[\"EPSG\",5703]]]"
    );
    for wkt in [in_projcs, in_compound_leg] {
        let err = extract_epsg_from_wkt(&wkt).unwrap_err();
        assert!(
            err.to_string()
                .contains("declares an explicit datum transformation"),
            "{wkt}: {err}"
        );
    }
}

#[test]
fn delimiter_and_bracket_rules_apply_inside_a_bound_vertical_leg() {
    for inner in [r#"VERTCRS["v"],PARAMETER("x",1)"#, r#"VERTCRS["v"],["x"]"#] {
        let wkt = format!(
            "COMPOUNDCRS[\"c\",PROJCRS[\"p\",ID[\"EPSG\",26917]],\
             BOUNDCRS[SOURCECRS[{inner}],TARGETCRS[GEOGCRS[\"g\"]]]]"
        );
        assert!(extract_epsg_from_wkt(&wkt).is_err(), "{wkt}");
    }
}

#[test]
fn bound_vertical_block_after_the_root_block_is_not_exempt() {
    // Text after the root block is not part of the CRS. A second top-level
    // block must not get the exemption of a vertical leg.
    let root = r#"PROJCS["p",GEOGCS["g",AUTHORITY["EPSG","4267"]],AUTHORITY["EPSG","26717"]]"#;
    let block = r#"BOUNDCRS[SOURCECRS[VERT_CS["v"]],TOWGS84[100,200,300]]"#;
    for wkt in [
        format!("{root},{block}"),
        format!("{root},COMPOUNDCRS[\"c\",{block}]"),
    ] {
        let err = extract_epsg_from_wkt(&wkt).unwrap_err();
        assert!(
            err.to_string()
                .contains("declares an explicit datum transformation"),
            "{wkt}: {err}"
        );
    }
}

#[test]
fn zero_towgs84_inside_a_bound_vertical_leg_is_not_checked() {
    // The vertical leg is discarded unread, so a clause inside it does not
    // take part in the zero-shift rule of the horizontal code.
    let wkt = "COMPOUNDCRS[\"c\",PROJCRS[\"p\",ID[\"EPSG\",27700]],\
               BOUNDCRS[SOURCECRS[VERTCRS[\"v\"]],TARGETCRS[GEOGCRS[\"g\"]],\
               TOWGS84[0,0,0,0,0,0,0]]]";
    let extraction = extract_epsg_from_wkt(wkt).unwrap();
    assert_eq!(extraction.epsg, 27700);
    assert!(extraction.vertical_stripped);
}
