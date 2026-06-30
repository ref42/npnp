use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use encoding_rs::WINDOWS_1252;
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::util::{nested_string, value_to_string};

const RAW_PER_DXP_UNIT: f64 = 100_000.0;
const GRID_UNITS: f64 = 10.0;
const PIN_LENGTH_UNITS: f64 = 20.0;
pub(super) const BORDER_BGR: i32 = SYMBOL_BGR;
pub(super) const FILL_BGR: i32 = 0xE0FFFF;
pub(super) const SYMBOL_BGR: i32 = 0x000000;
pub(super) const RED_BGR: i32 = 0x0000FF;
const BLUE_BGR: i32 = 0xFF0000;

pub fn write_schlib_from_payload(
    payload: &Value,
    component_name: &str,
    output_path: &Path,
) -> Result<()> {
    let component = build_component(payload, component_name)?;
    write_schlib(&component, output_path)
}

fn build_component(payload: &Value, component_name: &str) -> Result<Component> {
    let rows = parse_easyeda_rows(payload)?;
    let mut pins = Vec::new();
    let mut rectangles = Vec::new();
    let mut attr_by_parent: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut bounds = OptionalBounds::default();
    let mut part_bounds = None;

    for row in &rows {
        let Some(row_type) = row.first().and_then(Value::as_str) else {
            continue;
        };
        match row_type.trim().to_ascii_uppercase().as_str() {
            "PIN" => {
                let pin = PinRaw {
                    id: row_string(row, 1),
                    x_units: row_f64(row, 4, 0.0),
                    y_units: row_f64(row, 5, 0.0),
                    length_units: row_f64(row, 6, 20.0),
                    rotation_degrees: row_f64(row, 7, 0.0),
                };
                if !pin.id.trim().is_empty() {
                    pins.push(pin.clone());
                }
                let angle_degrees = normalize_angle(pin.rotation_degrees);
                let (delta_x_units, delta_y_units) = if !(45.0..315.0).contains(&angle_degrees) {
                    (pin.length_units, 0.0)
                } else if angle_degrees < 135.0 {
                    (0.0, pin.length_units)
                } else if angle_degrees < 225.0 {
                    (-pin.length_units, 0.0)
                } else {
                    (0.0, -pin.length_units)
                };
                bounds.update_x(
                    pin.x_units.min(pin.x_units + delta_x_units),
                    pin.x_units.max(pin.x_units + delta_x_units),
                );
                bounds.update_y(
                    pin.y_units.min(pin.y_units + delta_y_units),
                    pin.y_units.max(pin.y_units + delta_y_units),
                );
            }
            "PART" => {
                if let Some(row_bounds) = part_bounds_from_row(row) {
                    bounds.update_x(row_bounds.min_x_units, row_bounds.max_x_units);
                    bounds.update_y(row_bounds.min_y_units, row_bounds.max_y_units);
                    part_bounds = Some(row_bounds);
                }
            }
            "ATTR" => {
                let parent = row_string(row, 2);
                let key = row_string(row, 3);
                if parent.trim().is_empty() || key.trim().is_empty() {
                    continue;
                }
                let key_upper = key.trim().to_ascii_uppercase();
                let attrs = attr_by_parent.entry(parent.trim().to_string()).or_default();
                attrs.insert(key_upper.clone(), row_string(row, 4));
                attrs.insert(
                    format!("{key_upper}__VISIBLE"),
                    row_bool(row, 6, true).to_string(),
                );
            }
            "RECT" => {
                let rect = RectRaw {
                    x1_units: row_f64(row, 2, 0.0),
                    y1_units: row_f64(row, 3, 0.0),
                    x2_units: row_f64(row, 4, 0.0),
                    y2_units: row_f64(row, 5, 0.0),
                };
                bounds.update_x(
                    rect.x1_units.min(rect.x2_units),
                    rect.x1_units.max(rect.x2_units),
                );
                bounds.update_y(
                    rect.y1_units.min(rect.y2_units),
                    rect.y1_units.max(rect.y2_units),
                );
                rectangles.push(rect);
            }
            _ => {}
        }
    }

    let mut component = Component {
        name: normalize_component_name(component_name),
        description: "Generated from EasyEDA symbol".to_string(),
        pins: Vec::new(),
        rectangles: Vec::new(),
    };

    let (laid_out_pins, laid_out_rect) = layout_pins(&pins, &attr_by_parent);
    if !laid_out_pins.is_empty() {
        component.rectangles.push(Rectangle {
            corner1: CoordPoint::from_symbol_units(
                laid_out_rect.x1_units,
                laid_out_rect.height_units() - laid_out_rect.y1_units,
            ),
            corner2: CoordPoint::from_symbol_units(
                laid_out_rect.x2_units,
                laid_out_rect.height_units() - laid_out_rect.y2_units,
            ),
            color_bgr: BORDER_BGR,
            fill_color_bgr: FILL_BGR,
            is_filled: true,
            is_transparent: false,
        });
        for pin in laid_out_pins {
            component.pins.push(Pin {
                designator: pin.designator,
                name: pin.name,
                location: CoordPoint::from_symbol_units(
                    pin.x_units,
                    laid_out_rect.height_units() - pin.y_units,
                ),
                length_raw: raw_from_symbol_units(pin.length_units),
                orientation: pin_orientation_from_rotation(pin.rotation_degrees),
                show_name: pin.show_name,
                show_designator: true,
                color_bgr: SYMBOL_BGR,
            });
        }
        return Ok(component);
    }

    if !rectangles.is_empty() {
        for rect in rectangles {
            component.rectangles.push(Rectangle {
                corner1: CoordPoint::from_symbol_units(rect.x1_units, rect.y1_units),
                corner2: CoordPoint::from_symbol_units(rect.x2_units, rect.y2_units),
                color_bgr: BORDER_BGR,
                fill_color_bgr: FILL_BGR,
                is_filled: true,
                is_transparent: false,
            });
        }
    } else if let Some(fallback_bounds) = part_bounds.or_else(|| bounds.finish()) {
        component.rectangles.push(Rectangle {
            corner1: CoordPoint::from_symbol_units(
                fallback_bounds.min_x_units,
                fallback_bounds.max_y_units,
            ),
            corner2: CoordPoint::from_symbol_units(
                fallback_bounds.max_x_units,
                fallback_bounds.min_y_units,
            ),
            color_bgr: BLUE_BGR,
            fill_color_bgr: BLUE_BGR,
            is_filled: false,
            is_transparent: true,
        });
    }

    for (pin_index, pin) in pins.iter().enumerate() {
        let attrs = attr_by_parent.get(&pin.id);
        let designator = safe_attr(attrs, "NUMBER")
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| (pin_index + 1).to_string());
        let name = safe_attr(attrs, "NAME")
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| designator.clone());
        let show_name = safe_attr_flag(attrs, "NAME", !name.trim().is_empty());
        let show_designator = safe_attr_flag(attrs, "NUMBER", true);
        component.pins.push(Pin {
            designator,
            name,
            location: CoordPoint::from_symbol_units(pin.x_units, pin.y_units),
            length_raw: raw_from_symbol_units(if pin.length_units > 0.000001 {
                pin.length_units
            } else {
                10.0
            }),
            orientation: pin_orientation_from_rotation(pin.rotation_degrees),
            show_name,
            show_designator,
            color_bgr: SYMBOL_BGR,
        });
    }

    Ok(component)
}

pub(super) fn parse_easyeda_rows(payload: &Value) -> Result<Vec<Vec<Value>>> {
    let data_str = nested_string(payload, &["result", "dataStr"])
        .or_else(|| nested_string(payload, &["dataStr"]))
        .ok_or_else(|| AppError::InvalidResponse("symbol payload has no dataStr".to_string()))?;
    let mut rows = Vec::new();
    for (line_index, line) in data_str.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row_value: Value = serde_json::from_str(trimmed).map_err(|err| {
            AppError::InvalidResponse(format!(
                "invalid EasyEDA dataStr row {}: {err}",
                line_index + 1
            ))
        })?;
        if let Value::Array(row) = row_value {
            rows.push(row);
        }
    }
    Ok(rows)
}
pub(super) fn layout_pins(
    pins: &[PinRaw],
    attr_by_parent: &HashMap<String, HashMap<String, String>>,
) -> (Vec<PlacedPin>, PlacedRect) {
    let mut rect = PlacedRect::default();
    let mut placed = Vec::new();
    if pins.is_empty() {
        return (placed, rect);
    }

    let mut groups = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut uncategorized = Vec::new();
    for pin in pins {
        if let Some(side) = classify_pin_side(pin.rotation_degrees) {
            groups[side.index()].push(pin.clone());
        } else {
            uncategorized.push(pin.clone());
        }
    }
    for side in PinSide::all() {
        sort_group(&mut groups[side.index()], side);
    }

    let populated: Vec<usize> = PinSide::all()
        .into_iter()
        .map(PinSide::index)
        .filter(|index| !groups[*index].is_empty())
        .collect();
    if !uncategorized.is_empty() {
        if populated.is_empty() {
            groups[PinSide::Left.index()].extend(uncategorized);
        } else if populated.len() == 1 {
            groups[populated[0]].extend(uncategorized);
        } else {
            for pin in uncategorized {
                let mut target_index = PinSide::Top.index();
                for side in PinSide::all().into_iter().skip(1) {
                    if groups[side.index()].len() < groups[target_index].len() {
                        target_index = side.index();
                    }
                }
                groups[target_index].push(pin);
            }
        }
    }
    for side in PinSide::all() {
        sort_group(&mut groups[side.index()], side);
    }

    let width_pin_count = groups[PinSide::Top.index()]
        .len()
        .max(groups[PinSide::Bottom.index()].len());
    let height_pin_count = groups[PinSide::Left.index()]
        .len()
        .max(groups[PinSide::Right.index()].len());
    let mut width_margin_units = 8.0;
    let mut height_margin_units = 8.0;
    let mut half_width_margin_units = width_margin_units / 2.0;
    let mut half_height_margin_units = height_margin_units / 2.0;
    if groups[PinSide::Top.index()].is_empty() && groups[PinSide::Bottom.index()].is_empty() {
        height_margin_units = 0.0;
        half_height_margin_units = 0.0;
    } else if groups[PinSide::Left.index()].is_empty() && groups[PinSide::Right.index()].is_empty()
    {
        width_margin_units = 0.0;
        half_width_margin_units = 0.0;
    }

    rect = PlacedRect {
        x1_units: 0.0,
        y1_units: 0.0,
        x2_units: (width_pin_count as f64 + width_margin_units) * GRID_UNITS + GRID_UNITS,
        y2_units: (height_pin_count as f64 + height_margin_units) * GRID_UNITS + GRID_UNITS,
    };
    let offsets = [
        (half_width_margin_units * GRID_UNITS, 0.0),
        (0.0, half_height_margin_units * GRID_UNITS + GRID_UNITS),
        (
            rect.width_units(),
            half_height_margin_units * GRID_UNITS + GRID_UNITS,
        ),
        (half_width_margin_units * GRID_UNITS, rect.height_units()),
    ];

    for side in PinSide::all() {
        let (offset_x_units, offset_y_units) = offsets[side.index()];
        for (pin_index, pin) in groups[side.index()].iter().enumerate() {
            let mut x_units = offset_x_units;
            let mut y_units = offset_y_units;
            if matches!(side, PinSide::Top | PinSide::Bottom) {
                x_units += pin_index as f64 * GRID_UNITS;
            } else {
                y_units += pin_index as f64 * GRID_UNITS;
            }
            let attrs = attr_by_parent.get(&pin.id);
            let designator = safe_attr(attrs, "NUMBER")
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| (placed.len() + 1).to_string());
            let name = safe_attr(attrs, "NAME")
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| designator.clone());
            let show_name = safe_attr_flag(attrs, "NAME", !name.trim().is_empty());
            let show_designator = safe_attr_flag(attrs, "NUMBER", true);
            placed.push(PlacedPin {
                designator,
                name: name.clone(),
                x_units,
                y_units,
                length_units: PIN_LENGTH_UNITS,
                rotation_degrees: side_rotation(side),
                show_name,
                show_designator,
            });
        }
    }

    (placed, rect)
}

fn write_schlib(component: &Component, output_path: &Path) -> Result<()> {
    let file = File::create(output_path)?;
    let mut compound = cfb::CompoundFile::create(file)?;
    let section_key = section_key_from_name(&component.name);
    write_stream(&mut compound, "/FileHeader", &file_header_bytes(component))?;
    if section_key != component.name {
        write_stream(
            &mut compound,
            "/SectionKeys",
            &section_keys_bytes(&component.name, &section_key),
        )?;
    }
    compound.create_storage(&format!("/{section_key}/"))?;
    write_stream(
        &mut compound,
        &format!("/{section_key}/Data"),
        &component_data_bytes(component),
    )?;
    write_stream(&mut compound, "/Storage", &storage_bytes())?;
    compound.flush()?;
    Ok(())
}

fn write_stream(
    compound: &mut cfb::CompoundFile<File>,
    stream_path: &str,
    data: &[u8],
) -> std::io::Result<()> {
    let mut stream = compound.create_stream(stream_path)?;
    stream.write_all(data)
}

fn file_header_bytes(component: &Component) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    let mut params = Params::default();
    params.push(
        "HEADER",
        "Protel for Windows - Schematic Library Editor Binary File Version 5.0",
    );
    params.push("WEIGHT", schlib_weight(component).to_string());
    params.push("MINORVERSION", "2");
    params.push("FONTIDCOUNT", "1");
    params.push("SIZE1", "10");
    params.push("FONTNAME1", "Times New Roman");
    params.push("USEMBCS", "T");
    params.push("ISBOC", "T");
    params.push("SHEETSTYLE", "9");
    params.push("SYSTEMFONT", "1");
    params.push("BORDERON", "T");
    params.push("SHEETNUMBERSPACESIZE", "12");
    params.push("AREACOLOR", "16317695");
    params.push("SNAPGRIDON", "T");
    params.push("SNAPGRIDSIZE", "10");
    params.push("VISIBLEGRIDON", "T");
    params.push("VISIBLEGRIDSIZE", "10");
    params.push("CUSTOMX", "18000");
    params.push("CUSTOMY", "18000");
    params.push("USECUSTOMSHEET", "T");
    params.push("REFERENCEZONESON", "T");
    params.push("DISPLAY_UNIT", "0");
    params.push("COMPCOUNT", "1");
    params.push("LIBREF0", &component.name);
    params.push("COMPDESCR0", &component.description);
    params.push("PARTCOUNT0", "2");
    writer.write_cstring_param_block(&params);
    writer.write_i32(1);
    writer.write_string_block(&component.name);
    writer.into_inner()
}

fn section_keys_bytes(component_name: &str, section_key: &str) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    let mut params = Params::default();
    params.push("KeyCount", "1");
    params.push("LibRef0", component_name);
    params.push("SectionKey0", section_key);
    writer.write_cstring_param_block(&params);
    writer.into_inner()
}

fn storage_bytes() -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    let mut params = Params::default();
    params.push("HEADER", "Icon storage");
    writer.write_cstring_param_block(&params);
    writer.into_inner()
}

fn schlib_weight(component: &Component) -> usize {
    1 + 1 + component.rectangles.len() + component.pins.len() + 3
}

fn component_data_bytes(component: &Component) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    let mut params = Params::default();
    params.push("RECORD", "1");
    params.push("LIBREFERENCE", &component.name);
    params.push("COMPONENTDESCRIPTION", &component.description);
    params.push("PARTCOUNT", "2");
    params.push("DISPLAYMODECOUNT", "1");
    params.push("OWNERPARTID", "-1");
    params.push("CURRENTPARTID", "1");
    params.push("LIBRARYPATH", "*");
    params.push("SOURCELIBRARYNAME", "*");
    params.push("SHEETPARTFILENAME", "*");
    params.push("TARGETFILENAME", "*");
    params.push("UNIQUEID", stable_unique_id(&component.name, "COMP"));
    params.push("AREACOLOR", "11599871");
    params.push("COLOR", "128");
    params.push("PARTIDLOCKED", "T");
    params.push("DESIGNITEMID", &component.name);
    if !component.pins.is_empty() {
        params.push("ALLPINCOUNT", component.pins.len().to_string());
    }
    writer.write_cstring_param_block(&params);
    for (index, rect) in component.rectangles.iter().enumerate() {
        let mut rect_params = Params::default();
        rect_params.push("RECORD", "14");
        rect_params.push_bool("ISNOTACCESIBLE", true);
        rect_params.push("OWNERPARTID", "1");
        rect_params.push_coord("LOCATION.X", rect.corner1.x_raw);
        rect_params.push_coord("LOCATION.Y", rect.corner1.y_raw);
        rect_params.push_coord("CORNER.X", rect.corner2.x_raw);
        rect_params.push_coord("CORNER.Y", rect.corner2.y_raw);
        rect_params.push("LINEWIDTH", "1");
        rect_params.push_non_zero("COLOR", rect.color_bgr);
        rect_params.push("AREACOLOR", rect.fill_color_bgr.to_string());
        rect_params.push_bool("ISSOLID", rect.is_filled);
        rect_params.push(
            "UNIQUEID",
            stable_unique_id(&component.name, &format!("RECT{index}")),
        );
        writer.write_cstring_param_block(&rect_params);
    }
    for pin in &component.pins {
        writer.write_block(0x01, |pin_writer| {
            pin_writer.write_i32(2);
            pin_writer.write_u8(0);
            pin_writer.write_i16(1);
            pin_writer.write_u8(0);
            pin_writer.write_u8(0);
            pin_writer.write_u8(0);
            pin_writer.write_u8(0);
            pin_writer.write_u8(0);
            pin_writer.write_pascal_short_string("");
            pin_writer.write_u8(0);
            pin_writer.write_u8(4);
            pin_writer.write_u8(pin_conglomerate(pin));
            pin_writer.write_i16(dxp_i16(pin.length_raw));
            pin_writer.write_i16(dxp_i16(pin.location.x_raw));
            pin_writer.write_i16(dxp_i16(pin.location.y_raw));
            pin_writer.write_i32(pin.color_bgr);
            pin_writer.write_pascal_short_string(&pin.name);
            pin_writer.write_pascal_short_string(&pin.designator);
            pin_writer.write_pascal_short_string("");
            pin_writer.write_pascal_short_string("");
            pin_writer.write_pascal_short_string("");
        });
    }

    let mut designator_params = Params::default();
    designator_params.push("RECORD", "34");
    designator_params.push("OWNERPARTID", "-1");
    designator_params.push("LOCATION.X_FRAC", "-5");
    designator_params.push("LOCATION.Y_FRAC", "5");
    designator_params.push("COLOR", "8388608");
    designator_params.push("FONTID", "1");
    designator_params.push("TEXT", "*");
    designator_params.push("NAME", "Designator");
    designator_params.push("READONLYSTATE", "1");
    designator_params.push("UNIQUEID", stable_unique_id(&component.name, "DESIGNATOR"));
    writer.write_cstring_param_block(&designator_params);

    let mut comment_params = Params::default();
    comment_params.push("RECORD", "41");
    comment_params.push("OWNERPARTID", "-1");
    comment_params.push("LOCATION.X_FRAC", "-5");
    comment_params.push("LOCATION.Y_FRAC", "-15");
    comment_params.push("COLOR", "8388608");
    comment_params.push("FONTID", "1");
    comment_params.push("ISHIDDEN", "T");
    comment_params.push("TEXT", "*");
    comment_params.push("NAME", "Comment");
    comment_params.push("UNIQUEID", stable_unique_id(&component.name, "COMMENT"));
    writer.write_cstring_param_block(&comment_params);

    let mut footer_params = Params::default();
    footer_params.push("RECORD", "44");
    writer.write_cstring_param_block(&footer_params);

    writer.into_inner()
}
pub(super) fn part_bounds_from_row(row: &[Value]) -> Option<Bounds> {
    let bbox = row.get(2)?.get("BBOX")?.as_array()?;
    if bbox.len() < 4 {
        return None;
    }
    let x1_units = value_as_f64(&bbox[0]).unwrap_or_default();
    let y1_units = value_as_f64(&bbox[1]).unwrap_or_default();
    let x2_units = value_as_f64(&bbox[2]).unwrap_or_default();
    let y2_units = value_as_f64(&bbox[3]).unwrap_or_default();
    Some(Bounds {
        min_x_units: x1_units.min(x2_units),
        max_x_units: x1_units.max(x2_units),
        min_y_units: y1_units.min(y2_units),
        max_y_units: y1_units.max(y2_units),
    })
}

pub(super) fn safe_attr<'a>(
    attrs: Option<&'a HashMap<String, String>>,
    key: &str,
) -> Option<&'a str> {
    attrs?
        .get(&key.to_ascii_uppercase())
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn safe_attr_flag(
    attrs: Option<&HashMap<String, String>>,
    key: &str,
    default: bool,
) -> bool {
    let Some(attrs) = attrs else {
        return default;
    };
    let visible_key = format!("{}__VISIBLE", key.to_ascii_uppercase());
    attrs
        .get(&visible_key)
        .and_then(|value| parse_boolish(value))
        .unwrap_or(default)
}

pub(super) fn row_string(row: &[Value], index: usize) -> String {
    row.get(index).and_then(value_to_string).unwrap_or_default()
}

pub(super) fn row_f64(row: &[Value], index: usize, default: f64) -> f64 {
    row.get(index).and_then(value_as_f64).unwrap_or(default)
}

pub(super) fn row_bool(row: &[Value], index: usize, default: bool) -> bool {
    row.get(index).and_then(value_as_bool).unwrap_or(default)
}

pub(super) fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

pub(super) fn value_as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::Number(number) => number.as_f64().map(|value| value.abs() > f64::EPSILON),
        Value::String(text) => parse_boolish(text),
        _ => None,
    }
}

fn parse_boolish(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "1" | "yes" | "y" => Some(true),
        "false" | "f" | "0" | "no" | "n" => Some(false),
        _ => None,
    }
}

pub(super) fn normalize_angle(rotation_degrees: f64) -> f64 {
    if !rotation_degrees.is_finite() {
        return 0.0;
    }
    let mut normalized = rotation_degrees % 360.0;
    if normalized < 0.0 {
        normalized += 360.0;
    }
    normalized
}

fn classify_pin_side(rotation_degrees: f64) -> Option<PinSide> {
    let normalized = normalize_angle(rotation_degrees);
    let targets = [0.0, 90.0, 180.0, 270.0];
    let mut best_index = None;
    let mut best_delta = f64::MAX;
    for (target_index, target_degrees) in targets.into_iter().enumerate() {
        let mut delta = (normalized - target_degrees).abs();
        if delta > 180.0 {
            delta = 360.0 - delta;
        }
        if delta < best_delta {
            best_delta = delta;
            best_index = Some(target_index);
        }
    }
    if best_delta > 15.0 {
        return None;
    }
    match best_index? {
        0 => Some(PinSide::Left),
        1 => Some(PinSide::Bottom),
        2 => Some(PinSide::Right),
        3 => Some(PinSide::Top),
        _ => None,
    }
}

fn sort_group(group: &mut [PinRaw], side: PinSide) {
    group.sort_by(|left, right| {
        let left_key = if matches!(side, PinSide::Top | PinSide::Bottom) {
            left.x_units
        } else {
            left.y_units
        };
        let right_key = if matches!(side, PinSide::Top | PinSide::Bottom) {
            right.x_units
        } else {
            right.y_units
        };
        left_key.partial_cmp(&right_key).unwrap_or(Ordering::Equal)
    });
}

fn side_rotation(side: PinSide) -> f64 {
    match side {
        PinSide::Left => 0.0,
        PinSide::Bottom => 90.0,
        PinSide::Right => 180.0,
        PinSide::Top => 270.0,
    }
}

pub(super) fn pin_orientation_from_rotation(rotation_degrees: f64) -> u8 {
    (((normalize_angle(rotation_degrees + 180.0) / 90.0).round() as i32).rem_euclid(4)) as u8
}

fn pin_conglomerate(pin: &Pin) -> u8 {
    let mut flags = pin.orientation & 0x03;
    if pin.show_name {
        flags |= 0x08;
    }
    if pin.show_designator {
        flags |= 0x10;
    }
    flags
}

pub(super) fn normalize_component_name(component_name: &str) -> String {
    let trimmed = component_name.trim();
    if trimmed.is_empty() {
        "component".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn section_key_from_name(name: &str) -> String {
    ascii_section_key_from_name(name)
}

pub(super) fn stable_unique_id(name: &str, salt: &str) -> String {
    const ALPHABET: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut hash: u64 = 0xCBF29CE484222325;
    for byte in name.bytes().chain([b'|']).chain(salt.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001B3);
    }
    let mut value = hash;
    let mut id = String::with_capacity(8);
    for _ in 0..8 {
        id.push(ALPHABET[(value % 26) as usize] as char);
        value /= 26;
    }
    id
}

pub(super) fn raw_from_symbol_units(value: f64) -> i64 {
    (value * RAW_PER_DXP_UNIT).round() as i64
}

fn dxp_parts(raw: i64) -> (i64, i64) {
    (raw / 100_000, raw % 100_000)
}

pub(super) fn dxp_i16(raw: i64) -> i16 {
    dxp_parts(raw).0.clamp(i16::MIN as i64, i16::MAX as i64) as i16
}

fn encode_ansi_lossy(text: &str) -> Vec<u8> {
    let sanitized = text.replace(' ', "?");
    let (bytes, _, _) = WINDOWS_1252.encode(&sanitized);
    bytes.into_owned()
}

fn requires_utf8_parameter(text: &str) -> bool {
    let (_, _, had_errors) = WINDOWS_1252.encode(text);
    had_errors
}

fn encode_utf8_parameter_value(text: &str) -> String {
    let bytes = text.as_bytes();
    let (value, _, _) = WINDOWS_1252.decode(bytes);
    value.into_owned()
}

fn ascii_section_key_from_name(name: &str) -> String {
    let mut key = String::new();
    for character in name.trim().chars() {
        if key.len() >= 31 {
            break;
        }
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
            key.push(character);
        } else {
            key.push('_');
        }
    }
    if key.is_empty() { "_".to_string() } else { key }
}

#[derive(Debug, Clone)]
pub(super) struct PinRaw {
    pub(super) id: String,
    pub(super) x_units: f64,
    pub(super) y_units: f64,
    pub(super) length_units: f64,
    pub(super) rotation_degrees: f64,
}
#[derive(Debug, Clone, Copy)]
struct RectRaw {
    x1_units: f64,
    y1_units: f64,
    x2_units: f64,
    y2_units: f64,
}
#[derive(Debug, Clone)]
pub(super) struct PlacedPin {
    pub(super) designator: String,
    pub(super) name: String,
    pub(super) x_units: f64,
    pub(super) y_units: f64,
    pub(super) length_units: f64,
    pub(super) rotation_degrees: f64,
    pub(super) show_name: bool,
    pub(super) show_designator: bool,
}
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct PlacedRect {
    pub(super) x1_units: f64,
    pub(super) y1_units: f64,
    pub(super) x2_units: f64,
    pub(super) y2_units: f64,
}
impl PlacedRect {
    pub(super) fn width_units(self) -> f64 {
        self.x2_units - self.x1_units
    }
    pub(super) fn height_units(self) -> f64 {
        self.y2_units - self.y1_units
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) struct Bounds {
    pub(super) min_x_units: f64,
    pub(super) max_x_units: f64,
    pub(super) min_y_units: f64,
    pub(super) max_y_units: f64,
}
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct OptionalBounds {
    pub(super) min_x_units: Option<f64>,
    pub(super) max_x_units: Option<f64>,
    pub(super) min_y_units: Option<f64>,
    pub(super) max_y_units: Option<f64>,
}
impl OptionalBounds {
    pub(super) fn update_x(&mut self, min_x_units: f64, max_x_units: f64) {
        self.min_x_units = Some(
            self.min_x_units
                .map_or(min_x_units, |value| value.min(min_x_units)),
        );
        self.max_x_units = Some(
            self.max_x_units
                .map_or(max_x_units, |value| value.max(max_x_units)),
        );
    }
    pub(super) fn update_y(&mut self, min_y_units: f64, max_y_units: f64) {
        self.min_y_units = Some(
            self.min_y_units
                .map_or(min_y_units, |value| value.min(min_y_units)),
        );
        self.max_y_units = Some(
            self.max_y_units
                .map_or(max_y_units, |value| value.max(max_y_units)),
        );
    }
    pub(super) fn finish(self) -> Option<Bounds> {
        Some(Bounds {
            min_x_units: self.min_x_units?,
            max_x_units: self.max_x_units?,
            min_y_units: self.min_y_units?,
            max_y_units: self.max_y_units?,
        })
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) struct CoordPoint {
    pub(super) x_raw: i64,
    pub(super) y_raw: i64,
}
impl CoordPoint {
    pub(super) fn from_symbol_units(x_units: f64, y_units: f64) -> Self {
        Self {
            x_raw: raw_from_symbol_units(x_units),
            y_raw: raw_from_symbol_units(y_units),
        }
    }
}
#[derive(Debug)]
struct Pin {
    designator: String,
    name: String,
    location: CoordPoint,
    length_raw: i64,
    orientation: u8,
    show_name: bool,
    show_designator: bool,
    color_bgr: i32,
}
#[derive(Debug)]
struct Rectangle {
    corner1: CoordPoint,
    corner2: CoordPoint,
    color_bgr: i32,
    fill_color_bgr: i32,
    is_filled: bool,
    is_transparent: bool,
}
#[derive(Debug)]
struct Component {
    name: String,
    description: String,
    pins: Vec<Pin>,
    rectangles: Vec<Rectangle>,
}
#[derive(Debug, Clone, Copy)]
enum PinSide {
    Top,
    Left,
    Right,
    Bottom,
}
impl PinSide {
    fn all() -> [Self; 4] {
        [Self::Top, Self::Left, Self::Right, Self::Bottom]
    }
    fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Left => 1,
            Self::Right => 2,
            Self::Bottom => 3,
        }
    }
}
#[derive(Debug, Default)]
pub(super) struct Params(Vec<(String, String)>);
impl Params {
    pub(super) fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.push((key.into(), value.into()));
    }
    pub(super) fn push_non_zero(&mut self, key: &str, value: i32) {
        if value != 0 {
            self.push(key, value.to_string());
        }
    }
    pub(super) fn push_bool(&mut self, key: &str, value: bool) {
        if value {
            self.push(key, "T");
        }
    }
    pub(super) fn push_coord(&mut self, key: &str, raw: i64) {
        let (dxp_value, frac_value) = dxp_parts(raw);
        if dxp_value != 0 {
            self.push(key, dxp_value.to_string());
        }
        if frac_value != 0 {
            self.push(format!("{key}_Frac"), frac_value.to_string());
        }
    }
    fn as_string(&self) -> String {
        let mut text = String::new();
        for (key, value) in &self.0 {
            text.push('|');
            text.push_str(key);
            text.push('=');
            text.push_str(value);
            if requires_utf8_parameter(value) {
                text.push('|');
                text.push_str("%UTF8%");
                text.push_str(key);
                text.push('=');
                text.push_str(&encode_utf8_parameter_value(value));
            }
        }
        text
    }
}
#[derive(Debug, Default)]
pub(super) struct BinaryWriter {
    data: Vec<u8>,
}
impl BinaryWriter {
    pub(super) fn into_inner(self) -> Vec<u8> {
        self.data
    }
    pub(super) fn write_u8(&mut self, value: u8) {
        self.data.push(value);
    }
    pub(super) fn write_i16(&mut self, value: i16) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    pub(super) fn write_i32(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    fn write_u32(&mut self, value: u32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    pub(super) fn write_pascal_short_string(&mut self, value: &str) {
        let bytes = encode_ansi_lossy(value);
        let byte_count = bytes.len().min(255);
        self.write_u8(byte_count as u8);
        self.data.extend_from_slice(&bytes[..byte_count]);
    }
    fn write_cstring(&mut self, value: &str) {
        self.data.extend_from_slice(&encode_ansi_lossy(value));
        self.write_u8(0);
    }
    pub(super) fn write_block(&mut self, flags: u8, serializer: impl FnOnce(&mut Self)) {
        let mut child = Self::default();
        serializer(&mut child);
        let child_data = child.into_inner();
        self.write_u32(((flags as u32) << 24) | child_data.len() as u32);
        self.data.extend_from_slice(&child_data);
    }
    pub(super) fn write_string_block(&mut self, value: &str) {
        self.write_block(0, |writer| writer.write_pascal_short_string(value));
    }
    pub(super) fn write_cstring_param_block(&mut self, params: &Params) {
        let text = params.as_string();
        self.write_block(0, |writer| writer.write_cstring(&text));
    }
}

#[cfg(test)]
mod tests {
    use super::{FILL_BGR, build_component, write_schlib_from_payload};
    use crate::schlib::{SchlibMetadata, SchlibParameter, write_schlib_from_payload_with_metadata};
    use encoding_rs::GBK;
    use serde_json::json;
    use std::fs::{self, File};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_payload() -> serde_json::Value {
        json!({"result": {"dataStr": r#"["DOCTYPE","SYMBOL","1.1"]
["PART","U.1",{"BBOX":[-10,-10,10,10]}]
["RECT","body",-10,-10,10,10,0,0,0,"st1",0]
["PIN","p1",1,null,-20,0,10,0,null,0,0,1]
["ATTR","p1n","p1","NAME","A",false,true,-5,0,0,"st3",0]
["ATTR","p1d","p1","NUMBER","1",false,true,-10,0,0,"st4",0]
["PIN","p2",1,null,20,0,10,180,null,0,0,1]
["ATTR","p2n","p2","NAME","B",false,true,5,0,0,"st3",0]
["ATTR","p2d","p2","NUMBER","2",false,true,10,0,0,"st4",0]"#}})
    }

    #[test]
    fn builds_filled_rectangular_symbol() {
        let component = build_component(&sample_payload(), "TEST").unwrap();
        assert_eq!(component.pins.len(), 2);
        assert_eq!(component.rectangles.len(), 1);
        assert_eq!(component.rectangles[0].fill_color_bgr, FILL_BGR);
        assert_eq!(super::pin_conglomerate(&component.pins[0]), 0x1A);
    }

    #[test]
    fn writes_minimal_schlib_streams() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("npnp_schlib_{timestamp}.SchLib"));
        write_schlib_from_payload(&sample_payload(), "TEST/COMP", &path).unwrap();
        let file = File::open(&path).unwrap();
        let mut compound = cfb::CompoundFile::open(file).unwrap();
        assert!(compound.open_stream("/FileHeader").is_ok());
        assert!(compound.open_stream("/Storage").is_ok());
        assert!(compound.open_stream("/SectionKeys").is_ok());

        let mut data_stream = compound.open_stream("/TEST_COMP/Data").unwrap();
        let mut data = Vec::new();
        use std::io::Read;
        data_stream.read_to_end(&mut data).unwrap();
        let data_text = String::from_utf8_lossy(&data);
        assert!(data_text.contains("|RECORD=1|"));
        assert!(data_text.contains("|LIBREFERENCE=TEST/COMP|"));
        assert!(data_text.contains("|PARTCOUNT=2|"));
        assert!(data_text.contains("|RECORD=14|"));
        assert!(data_text.contains("|ISNOTACCESIBLE=T|"));
        assert!(data_text.contains("|OWNERPARTID=1|"));
        assert!(data_text.contains("|LINEWIDTH=1|"));
        assert!(data_text.contains("|RECORD=34|"));
        assert!(data_text.contains("|NAME=Designator|"));
        assert!(data_text.contains("|RECORD=41|"));
        assert!(data_text.contains("|NAME=Comment|"));
        assert!(data_text.contains("|RECORD=44"));

        let rect_index = data
            .windows(b"|RECORD=14|".len())
            .position(|window| window == b"|RECORD=14|")
            .unwrap();
        let pin_text_index = data
            .windows([1u8, b'A', 1u8, b'1', 0u8, 0u8, 0u8].len())
            .position(|window| window == [1u8, b'A', 1u8, b'1', 0u8, 0u8, 0u8])
            .unwrap();
        assert!(rect_index < pin_text_index);

        fs::remove_file(path).ok();
    }

    #[test]
    fn writes_utf8_companion_for_chinese_parameters() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("npnp_schlib_utf8_{timestamp}.SchLib"));
        let metadata = SchlibMetadata {
            parameters: vec![SchlibParameter {
                name: "Manufacturer".to_string(),
                value: "DORABO(地博电气)".to_string(),
            }],
            ..SchlibMetadata::default()
        };

        write_schlib_from_payload_with_metadata(&sample_payload(), "TEST", &metadata, &path)
            .unwrap();
        let file = File::open(&path).unwrap();
        let mut compound = cfb::CompoundFile::open(file).unwrap();
        let mut data_stream = compound.open_stream("/TEST/Data").unwrap();
        let mut data = Vec::new();
        use std::io::Read;
        data_stream.read_to_end(&mut data).unwrap();

        assert!(
            data.windows(b"|TEXT=DORABO(&#22320;&#21338;&#30005;&#27668;)".len())
                .any(|window| window == b"|TEXT=DORABO(&#22320;&#21338;&#30005;&#27668;)")
        );

        let mut utf8_field = b"|%UTF8%TEXT=".to_vec();
        utf8_field.extend_from_slice("DORABO(地博电气)".as_bytes());
        assert!(
            data.windows(utf8_field.len())
                .any(|window| window == utf8_field.as_slice())
        );

        let (gbk_bytes, _, _) = GBK.encode("DORABO(地博电气)");
        assert!(
            !data
                .windows(gbk_bytes.len())
                .any(|window| window == gbk_bytes.as_ref())
        );

        fs::remove_file(path).ok();
    }
}
