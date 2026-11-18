#!/usr/bin/env python3
# ABOUTME: Generates the Helmert datum shift table and its PROJ reference fixture.
# ABOUTME: Reads the crs-definitions catalogue and asks PROJ (through pyproj) for each shift.
"""Generate `src/datum_shift_table.rs` and `tests/fixtures/datum-shift-reference.txt`.

The catalogue that proj4rs reads (crs-definitions) carries no datum shift. A
definition that names only an ellipsoid, and whose ellipsoid is not GRS80 or
WGS 84, therefore needs a shift from another source. This tool asks PROJ which
coordinate operation it uses for each such CRS over the whole EPSG area of use
of that CRS, and keeps the first operation that is a static Helmert
transformation (3 or 7 parameters) or a plain change of ellipsoid.

A CRS for which PROJ has no such operation goes on the refusal list. So does
every CRS in `tools/datum-shift-exclusions.txt`: PROJ has an operation for
those, but proj4rs cannot reproduce the PROJ position from the catalogue
definition.

Usage:
    python tools/gen_datum_shifts.py <crs-definitions>/src/defs.rs <repo root>

Requirements: pyproj (the wheel bundles PROJ and its EPSG database). The grid
files of PROJ must NOT be installed and the PROJ network must stay off, so that
PROJ ranks only operations that need no grid.
"""

import math
import multiprocessing
import re
import sys
import warnings
from pathlib import Path

import pyproj
from pyproj import CRS
from pyproj.enums import TransformDirection
from pyproj.transformer import AreaOfInterest, TransformerGroup

WGS84_A = 6378137.0
WGS84_ES = 0.0066943799901413165
# proj4rs treats an ellipsoid as WGS 84 when `a` is equal and `es` is this close.
ES_TOLERANCE = 0.000000000050

DEF_LINE = re.compile(r'^EPSG_(\d+)\|(\d+)\|"([^"]*)"\|')


def read_catalogue(defs_path):
    """Return {code: proj4 string} for every catalogue definition."""
    defs = {}
    for line in Path(defs_path).read_text(encoding="utf-8").splitlines():
        m = DEF_LINE.match(line)
        if m:
            defs[int(m.group(2))] = m.group(3)
    return defs


def proj_params(proj4):
    params = {}
    for token in proj4.split():
        if not token.startswith("+"):
            continue
        key, _, value = token[1:].partition("=")
        params[key] = value
    return params


def is_wgs84_ellipsoid(params):
    ellps = params.get("ellps")
    if ellps in ("GRS80", "WGS84"):
        return True
    if ellps is not None or "a" not in params:
        return False
    a = float(params["a"])
    if "rf" in params:
        f = 1.0 / float(params["rf"])
        es = f * (2.0 - f)
    elif "b" in params:
        b = float(params["b"])
        es = 1.0 - (b * b) / (a * a)
    else:
        return False
    return a == WGS84_A and abs(es - WGS84_ES) < ES_TOLERANCE


def needs_datum_shift(proj4):
    """True when proj4rs builds this definition with no datum and a foreign ellipsoid."""
    params = proj_params(proj4)
    if "proj" not in params:
        return False
    if "towgs84" in params or "datum" in params or "nadgrids" in params:
        return False
    return not is_wgs84_ellipsoid(params)


def split_steps(definition):
    steps = []
    for raw in definition.split(" step ")[1:]:
        tokens = raw.split()
        step = {"inv": "inv" in tokens}
        for token in tokens:
            if token == "inv":
                continue
            key, _, value = token.partition("=")
            step[key] = value
        steps.append(step)
    return steps


TRANSFORM_PROJ = {
    "cart", "helmert", "push", "pop", "hgridshift", "vgridshift", "gridshift",
    "xyzgridshift", "molobadekas", "molodensky", "geogoffset", "deformation",
    "affine", "defmodel", "tinshift", "set", "noop",
}
HELMERT_REJECT = {"dx", "dy", "dz", "drx", "dry", "drz", "ds", "t_epoch", "exact", "transpose"}


def helmert_parameters(definition):
    """Return the 7 Position Vector parameters of a usable operation, else None.

    Usable means one of:
    - a pipeline of any conversion steps, then exactly
      push v_3, cart (source ellipsoid), [one static helmert], inv cart WGS84,
      pop v_3, cart WGS84. With no helmert step the operation is a pure change
      of ellipsoid and yields seven zeros;
    - a single static helmert step (a geocentric source CRS).
    """
    if definition.startswith("proj=helmert "):
        helmert = split_steps(" step " + definition)[0]
    else:
        steps = split_steps(definition)
        start = next((i for i, s in enumerate(steps) if s.get("proj") == "push"), None)
        if start is None:
            return None
        if any(s.get("proj") in TRANSFORM_PROJ for s in steps[:start]):
            return None
        core = steps[start:]
        kinds = [(s.get("proj"), s["inv"]) for s in core]
        with_helmert = [("push", False), ("cart", False), ("helmert", False),
                        ("cart", True), ("pop", False), ("cart", False)]
        without_helmert = with_helmert[:2] + with_helmert[3:]
        if kinds == without_helmert:
            helmert = {}
        elif kinds == with_helmert:
            helmert = core[2]
        else:
            return None
        if core[0].get("v_3") is None or core[-2].get("v_3") is None:
            return None
        if core[-1].get("ellps") != "WGS84" or core[-3].get("ellps") != "WGS84":
            return None
    if HELMERT_REJECT & set(helmert):
        return None
    values = [float(helmert.get(k, 0.0) or 0.0) for k in ("x", "y", "z", "rx", "ry", "rz", "s")]
    convention = helmert.get("convention")
    if convention == "coordinate_frame":
        values[3:6] = [-v for v in values[3:6]]
    elif convention not in (None, "position_vector"):
        return None
    return [v + 0.0 for v in values]


def transformation_id(transformer):
    """Return (name, identifier) of the datum transformation inside the operation.

    PROJ can merge two published transformations (for example a local datum to
    a national datum, then that datum to WGS 84) into one Helmert step. The
    name and identifier then list both.
    """
    found = []

    def walk(node):
        if isinstance(node, dict):
            if node.get("type") == "Transformation":
                method = (node.get("method") or {}).get("name", "")
                name = node.get("name") or ""
                if method != "Longitude rotation" and not name.startswith("Null geographic offset"):
                    found.append((name, node.get("id")))
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(transformer.to_json_dict())
    if not found or any(ident is None for _, ident in found):
        return None
    name = " + ".join(name for name, _ in found)
    ident = "+".join(f"{i['authority'].replace('DERIVED_FROM(EPSG)', 'EPSG')}:{i['code']}"
                     for _, i in found)
    return name, ident


def sample_lonlat(bounds):
    west, south, east, north = bounds
    if east < west:
        east += 360.0
    points = []
    for fx, fy, height in ((0.5, 0.5, 123.456), (0.25, 0.3, 1850.0)):
        lon = west + fx * (east - west)
        if lon > 180.0:
            lon -= 360.0
        points.append((lon, south + fy * (north - south), height))
    return points


def geodetic_to_ecef(lon_deg, lat_deg, h):
    lon, lat = math.radians(lon_deg), math.radians(lat_deg)
    n = WGS84_A / math.sqrt(1.0 - WGS84_ES * math.sin(lat) ** 2)
    return ((n + h) * math.cos(lat) * math.cos(lon),
            (n + h) * math.cos(lat) * math.sin(lon),
            (n * (1.0 - WGS84_ES) + h) * math.sin(lat))


def select_operation(code):
    """Return (reason, None) for a refusal or (None, record) for a usable shift."""
    warnings.simplefilter("ignore")
    try:
        crs = CRS.from_epsg(code)
    except Exception:
        return "not in the PROJ database", None
    if crs.area_of_use is None:
        return "no area of use", None
    aoi = AreaOfInterest(*crs.area_of_use.bounds)
    # The crate drops the vertical component of a compound CRS, so the shift
    # of a compound CRS is the shift of its horizontal component.
    horizontal = crs.sub_crs_list[0] if crs.is_compound else crs
    group = TransformerGroup(horizontal, CRS.from_epsg(4978), always_xy=True, area_of_interest=aoi)
    for transformer in group.transformers:
        if "Ballpark" in transformer.description:
            continue
        params = helmert_parameters(transformer.definition)
        if params is None:
            continue
        ident = transformation_id(transformer)
        if ident is None:
            continue
        samples = []
        for lon, lat, height in sample_lonlat(crs.area_of_use.bounds):
            if horizontal.is_geocentric:
                # A geocentric source has no separate height: every axis is shifted.
                src = transformer.transform(*geodetic_to_ecef(lon, lat, height),
                                            direction=TransformDirection.INVERSE)
            else:
                x, y, _ = transformer.transform(*geodetic_to_ecef(lon, lat, 0.0),
                                                direction=TransformDirection.INVERSE)
                src = (x, y, height)
            ecef = transformer.transform(*src)
            if not all(math.isfinite(v) for v in (*src, *ecef)):
                samples = []
                break
            samples.append((src, ecef))
        if not samples:
            continue
        accuracy = transformer.accuracy if transformer.accuracy >= 0 else None
        return None, {"params": params, "name": ident[0], "op": ident[1],
                      "accuracy": accuracy, "samples": samples}
    return "PROJ has no static Helmert transformation to WGS 84 for the area of use", None


def read_exclusions(path):
    """Return {code: reason} from the exclusion list beside this script."""
    exclusions = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip() and not line.startswith("#"):
            code, _, reason = line.partition("\t")
            exclusions[int(code)] = reason
    return exclusions


def rust_f64(value):
    text = repr(float(value))
    return text if any(c in text for c in ".einf") else text + ".0"


def main():
    defs_path, repo = sys.argv[1], Path(sys.argv[2])
    warnings.simplefilter("ignore")
    if pyproj.network.is_network_enabled():
        sys.exit("PROJ network is on; turn it off so that no grid is used")
    catalogue = read_catalogue(defs_path)
    exclusions = read_exclusions(Path(__file__).with_name("datum-shift-exclusions.txt"))
    affected = sorted(code for code, proj4 in catalogue.items() if needs_datum_shift(proj4))
    with multiprocessing.Pool(4) as pool:
        selected = pool.map(select_operation, affected, chunksize=16)

    operations = {}  # (op code, params) -> record
    crs_operation = []
    refused = []
    reasons = {}
    fixture = []
    for code, (reason, record) in zip(affected, selected):
        if record is not None and code in exclusions:
            reason, record = "excluded: " + exclusions[code], None
        if record is None:
            refused.append(code)
            reasons[reason] = reasons.get(reason, 0) + 1
            continue
        key = (record["op"], tuple(record["params"]))
        operations.setdefault(key, record)
        crs_operation.append((code, key))
        for (x, y, z), ecef in record["samples"]:
            fixture.append(f"{code} {x!r} {y!r} {z!r} {ecef[0]!r} {ecef[1]!r} {ecef[2]!r}")

    keys = sorted(operations)
    index = {key: i for i, key in enumerate(keys)}
    header = (f"pyproj {pyproj.__version__}, PROJ {pyproj.proj_version_str}, "
              f"crs-definitions catalogue with {len(catalogue)} definitions")

    out = []
    out.append("// ABOUTME: Helmert datum shifts to WGS 84 for catalogue CRSs that carry no datum.")
    out.append("// ABOUTME: Generated by tools/gen_datum_shifts.py; do not edit by hand.")
    out.append(f"//! Source: {header}.")
    out.append("//!")
    out.append("//! `OPERATIONS` holds each EPSG coordinate operation once. Parameters use the")
    out.append("//! Position Vector convention of a proj4 `+towgs84` clause: translations in")
    out.append("//! metres, rotations in arc-seconds, scale difference in parts per million.")
    out.append("//! `CRS_OPERATIONS` maps a CRS code to an index into `OPERATIONS` and is sorted")
    out.append("//! by CRS code. `CRS_WITHOUT_SHIFT` lists the CRS codes that need a datum shift")
    out.append("//! for which PROJ has no static Helmert transformation; it is sorted too.")
    out.append("")
    out.append("use crate::datum_shift::DatumShift;")
    out.append("")
    out.append("#[rustfmt::skip]")
    out.append(f"pub(crate) static OPERATIONS: [DatumShift; {len(keys)}] = [")
    for key in keys:
        record = operations[key]
        accuracy = "None" if record["accuracy"] is None else f"Some({rust_f64(record['accuracy'])})"
        params = ", ".join(rust_f64(v) for v in record["params"])
        name = record["name"].replace("\\", "\\\\").replace('"', '\\"')
        out.append(f'    DatumShift {{ operation_id: "{record["op"]}", operation_name: "{name}", '
                   f"accuracy_m: {accuracy}, parameters: [{params}] }},")
    out.append("];")
    out.append("")
    out.append("#[rustfmt::skip]")
    out.append(f"pub(crate) static CRS_OPERATIONS: [(u16, u16); {len(crs_operation)}] = [")
    row = []
    for code, key in crs_operation:
        row.append(f"({code}, {index[key]})")
        if len(row) == 8:
            out.append("    " + ", ".join(row) + ",")
            row = []
    if row:
        out.append("    " + ", ".join(row) + ",")
    out.append("];")
    out.append("")
    out.append("#[rustfmt::skip]")
    out.append(f"pub(crate) static CRS_WITHOUT_SHIFT: [u16; {len(refused)}] = [")
    for i in range(0, len(refused), 16):
        out.append("    " + ", ".join(str(c) for c in refused[i:i + 16]) + ",")
    out.append("];")
    (repo / "src" / "datum_shift_table.rs").write_text("\n".join(out) + "\n", encoding="utf-8")

    fixture_header = [
        "# Reference positions for every CRS in the datum shift table.",
        f"# Generated by tools/gen_datum_shifts.py with {header}.",
        "# Each row is one source coordinate and the EPSG:4978 position that PROJ gives",
        "# for it with the coordinate operation recorded in src/datum_shift_table.rs.",
        "# Format: epsg src_x src_y src_z ecef_x ecef_y ecef_z",
    ]
    (repo / "tests" / "fixtures" / "datum-shift-reference.txt").write_text(
        "\n".join(fixture_header + fixture) + "\n", encoding="utf-8")

    print(header)
    print(f"definitions that need a datum shift: {len(affected)}")
    print(f"  with a Helmert shift: {len(crs_operation)} ({len(keys)} distinct operations)")
    print(f"  refused: {len(refused)}")
    for reason, count in sorted(reasons.items()):
        print(f"    {count:5d}  {reason}")


if __name__ == "__main__":
    main()
