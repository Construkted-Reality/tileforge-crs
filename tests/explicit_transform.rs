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
    for operation in ["TOWGS84[100,200,300]", "towgs84 [0,0,0,0,0,0,0]"] {
        let wkt =
            format!("GEOGCS[\"custom\",DATUM[\"datum\",{operation}],AUTHORITY[\"EPSG\",\"4269\"]]");
        let err = extract_epsg_from_wkt(&wkt).unwrap_err();
        assert!(err.to_string().contains("TOWGS84"));
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
