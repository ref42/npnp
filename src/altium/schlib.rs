#[path = "schlib_common.rs"]
#[allow(dead_code)]
mod common;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::error::Result;
use crate::util::nested_string;

const WHITE_BGR: i32 = 0xFFFFFF;
/// The fill colour that means "no colour at all": the symbol keeps its outline
/// and the body comes out transparent, whatever colour the source shape asked
/// for.
pub const NO_FILL_BGR: i32 = -1;
const BODY_LINE_WIDTH_INDEX: i32 = 1;
const GRAPHIC_LINE_WIDTH_INDEX: i32 = 1;
const PIN_LENGTH_UNITS: f64 = 20.0;
const MIN_INDUCTOR_PIN_LENGTH_UNITS: f64 = 10.0;

#[derive(Debug, Clone, Default)]
pub struct SchlibMetadata {
    pub description: Option<String>,
    pub designator: Option<String>,
    pub comment: Option<String>,
    pub parameters: Vec<SchlibParameter>,
    pub footprint_model_name: Option<String>,
    pub footprint_library_file: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SchlibParameter {
    pub name: String,
    pub value: String,
}

pub fn write_schlib_from_payload(
    payload: &Value,
    component_name: &str,
    output_path: &Path,
) -> Result<()> {
    write_schlib_from_payload_with_metadata(
        payload,
        component_name,
        &SchlibMetadata::default(),
        output_path,
    )
}

pub fn write_schlib_from_payload_with_metadata(
    payload: &Value,
    component_name: &str,
    metadata: &SchlibMetadata,
    output_path: &Path,
) -> Result<()> {
    let component = build_component_from_payload_with_metadata(payload, component_name, metadata)?;
    write_schlib_library(std::slice::from_ref(&component), output_path)
}

pub fn build_component_from_payload(payload: &Value, component_name: &str) -> Result<Component> {
    build_component_from_payload_with_metadata(payload, component_name, &SchlibMetadata::default())
}

pub fn build_component_from_payload_with_metadata(
    payload: &Value,
    component_name: &str,
    metadata: &SchlibMetadata,
) -> Result<Component> {
    build_component(
        payload,
        component_name,
        metadata,
        common::SYMBOL_BGR,
        common::FILL_BGR,
    )
}

pub fn build_component_from_payload_with_metadata_and_color(
    payload: &Value,
    component_name: &str,
    metadata: &SchlibMetadata,
    color_bgr: i32,
    fill_color_bgr: i32,
) -> Result<Component> {
    build_component(payload, component_name, metadata, color_bgr, fill_color_bgr)
}

fn build_component(
    payload: &Value,
    component_name: &str,
    metadata: &SchlibMetadata,
    color_bgr: i32,
    fill_color_bgr: i32,
) -> Result<Component> {
    // A caller can ask for no fill colour at all. That sentinel wins over
    // whatever fill the source shapes carried: they keep their outlines and
    // their areas come out transparent.
    let no_fill = fill_color_bgr == NO_FILL_BGR;
    let fill_color_bgr = if no_fill { WHITE_BGR } else { fill_color_bgr };
    let rows = common::parse_easyeda_rows(payload)?;
    let mut parts: Vec<PartRaw> = Vec::new();
    let mut current_part_index = None;
    let mut has_part_rows = false;
    let mut attr_by_parent: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut root_attrs: HashMap<String, String> = HashMap::new();
    let mut global_bounds = common::OptionalBounds::default();

    for row in &rows {
        let Some(row_type) = row.first().and_then(Value::as_str) else {
            continue;
        };
        match row_type.trim().to_ascii_uppercase().as_str() {
            "PART" => {
                has_part_rows = true;
                let bounds = common::part_bounds_from_row(row);
                if let Some(bounds) = bounds {
                    global_bounds.update_x(bounds.min_x_units, bounds.max_x_units);
                    global_bounds.update_y(bounds.min_y_units, bounds.max_y_units);
                }
                let owner_part_id = parts.len() as i32 + 1;
                parts.push(PartRaw::new(owner_part_id, bounds));
                current_part_index = Some(parts.len() - 1);
            }
            "ATTR" => {
                let parent = common::row_string(row, 2);
                let key = common::row_string(row, 3);
                if key.trim().is_empty() {
                    continue;
                }
                let key_upper = key.trim().to_ascii_uppercase();
                let value = common::row_string(row, 4);
                if parent.trim().is_empty() {
                    root_attrs.insert(key_upper, value);
                    continue;
                }
                let attrs = attr_by_parent.entry(parent.trim().to_string()).or_default();
                attrs.insert(key_upper.clone(), value);
                attrs.insert(
                    format!("{key_upper}__VISIBLE"),
                    common::row_bool(row, 6, true).to_string(),
                );
            }
            "PIN" => {
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                let pin = PinRaw {
                    id: common::row_string(row, 1),
                    x_units: common::row_f64(row, 4, 0.0),
                    y_units: common::row_f64(row, 5, 0.0),
                    length_units: common::row_f64(row, 6, PIN_LENGTH_UNITS),
                    rotation_degrees: common::row_f64(row, 7, 0.0),
                    owner_part_id,
                };
                if pin.id.trim().is_empty() {
                    continue;
                }
                let (body_x_units, body_y_units) = pin_body_location_from_easyeda(
                    pin.x_units,
                    pin.y_units,
                    pin.length_units,
                    pin.rotation_degrees,
                );
                let min_x = pin.x_units.min(body_x_units);
                let max_x = pin.x_units.max(body_x_units);
                let min_y = pin.y_units.min(body_y_units);
                let max_y = pin.y_units.max(body_y_units);
                parts[part_index].bounds.update_x(min_x, max_x);
                parts[part_index].bounds.update_y(min_y, max_y);
                parts[part_index].pins.push(pin);
                global_bounds.update_x(min_x, max_x);
                global_bounds.update_y(min_y, max_y);
            }
            "RECT" => {
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                let rect = RectRaw {
                    x1_units: common::row_f64(row, 2, 0.0),
                    y1_units: common::row_f64(row, 3, 0.0),
                    x2_units: common::row_f64(row, 4, 0.0),
                    y2_units: common::row_f64(row, 5, 0.0),
                    owner_part_id,
                };
                let min_x = rect.x1_units.min(rect.x2_units);
                let max_x = rect.x1_units.max(rect.x2_units);
                let min_y = rect.y1_units.min(rect.y2_units);
                let max_y = rect.y1_units.max(rect.y2_units);
                parts[part_index].bounds.update_x(min_x, max_x);
                parts[part_index].bounds.update_y(min_y, max_y);
                parts[part_index].rectangles.push(rect);
                global_bounds.update_x(min_x, max_x);
                global_bounds.update_y(min_y, max_y);
            }
            "POLY" | "POLYGON" | "PATH" => {
                let Some(shape) = row.get(2) else {
                    continue;
                };
                let points = parse_path_raw_points(shape);
                if points.len() < 2 {
                    continue;
                }
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                for point in &points {
                    parts[part_index]
                        .bounds
                        .update_x(point.x_units, point.x_units);
                    parts[part_index]
                        .bounds
                        .update_y(point.y_units, point.y_units);
                    global_bounds.update_x(point.x_units, point.x_units);
                    global_bounds.update_y(point.y_units, point.y_units);
                }
                parts[part_index].polylines.push(PolylineRaw {
                    points,
                    owner_part_id,
                });
            }
            "LINE" => {
                let points = vec![
                    PointUnits {
                        x_units: common::row_f64(row, 2, 0.0),
                        y_units: common::row_f64(row, 3, 0.0),
                    },
                    PointUnits {
                        x_units: common::row_f64(row, 4, 0.0),
                        y_units: common::row_f64(row, 5, 0.0),
                    },
                ];
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                for point in &points {
                    parts[part_index]
                        .bounds
                        .update_x(point.x_units, point.x_units);
                    parts[part_index]
                        .bounds
                        .update_y(point.y_units, point.y_units);
                    global_bounds.update_x(point.x_units, point.x_units);
                    global_bounds.update_y(point.y_units, point.y_units);
                }
                parts[part_index].polylines.push(PolylineRaw {
                    points,
                    owner_part_id,
                });
            }
            "ARC" => {
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                let arc = ArcRaw {
                    start: PointUnits {
                        x_units: common::row_f64(row, 2, 0.0),
                        y_units: common::row_f64(row, 3, 0.0),
                    },
                    mid: PointUnits {
                        x_units: common::row_f64(row, 4, 0.0),
                        y_units: common::row_f64(row, 5, 0.0),
                    },
                    end: PointUnits {
                        x_units: common::row_f64(row, 6, 0.0),
                        y_units: common::row_f64(row, 7, 0.0),
                    },
                    owner_part_id,
                };
                for point in [arc.start, arc.mid, arc.end] {
                    parts[part_index]
                        .bounds
                        .update_x(point.x_units, point.x_units);
                    parts[part_index]
                        .bounds
                        .update_y(point.y_units, point.y_units);
                    global_bounds.update_x(point.x_units, point.x_units);
                    global_bounds.update_y(point.y_units, point.y_units);
                }
                parts[part_index].arcs.push(arc);
            }
            "CIRCLE" => {
                let r = common::row_f64(row, 4, 0.0).abs();
                if r <= 0.000001 {
                    continue;
                }
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                let filled = r <= 1.0 + f64::EPSILON;
                let ellipse = EllipseRaw {
                    center_x_units: common::row_f64(row, 2, 0.0),
                    center_y_units: common::row_f64(row, 3, 0.0),
                    radius_x_units: r,
                    radius_y_units: r,
                    owner_part_id,
                    is_filled: filled,
                    is_transparent: !filled,
                };
                update_ellipse_bounds(&mut parts[part_index].bounds, &ellipse);
                update_ellipse_bounds(&mut global_bounds, &ellipse);
                parts[part_index].ellipses.push(ellipse);
            }
            "ELLIPSE" => {
                let rx = common::row_f64(row, 4, 0.0).abs();
                let ry = common::row_f64(row, 5, 0.0).abs();
                if rx <= 0.000001 || ry <= 0.000001 {
                    continue;
                }
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                let filled = rx.max(ry) <= 1.0 + f64::EPSILON;
                let ellipse = EllipseRaw {
                    center_x_units: common::row_f64(row, 2, 0.0),
                    center_y_units: common::row_f64(row, 3, 0.0),
                    radius_x_units: rx,
                    radius_y_units: ry,
                    owner_part_id,
                    is_filled: filled,
                    is_transparent: !filled,
                };
                update_ellipse_bounds(&mut parts[part_index].bounds, &ellipse);
                update_ellipse_bounds(&mut global_bounds, &ellipse);
                parts[part_index].ellipses.push(ellipse);
            }
            "TEXT" => {
                let text = normalize_text_value(&common::row_string(row, 5));
                if text.is_empty() {
                    continue;
                }
                let part_index = ensure_current_part_index(&mut parts, &mut current_part_index);
                let owner_part_id = parts[part_index].owner_part_id;
                let label = TextRaw {
                    text,
                    x_units: common::row_f64(row, 2, 0.0),
                    y_units: common::row_f64(row, 3, 0.0),
                    rotation_degrees: common::row_f64(row, 4, 0.0),
                    owner_part_id,
                };
                parts[part_index]
                    .bounds
                    .update_x(label.x_units, label.x_units);
                parts[part_index]
                    .bounds
                    .update_y(label.y_units, label.y_units);
                global_bounds.update_x(label.x_units, label.x_units);
                global_bounds.update_y(label.y_units, label.y_units);
                parts[part_index].labels.push(label);
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        parts.push(PartRaw::new(1, None));
    }

    let description = metadata
        .description
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| nested_string(payload, &["result", "description"]))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Generated from EasyEDA symbol".to_string());
    let designator_text = metadata
        .designator
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| root_attrs.get("DESIGNATOR").cloned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "U?".to_string());
    let comment_text = metadata
        .comment
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| root_attrs.get("NAME").cloned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "*".to_string());
    let parameters = metadata
        .parameters
        .iter()
        .filter(|parameter| !parameter.name.trim().is_empty() && !parameter.value.trim().is_empty())
        .cloned()
        .collect();

    let mut component = Component {
        name: normalize_component_name(component_name),
        description,
        designator_text,
        comment_text,
        parameters,
        implementations: Vec::new(),
        part_count: parts.len().max(1),
        pins: Vec::new(),
        rectangles: Vec::new(),
        polylines: Vec::new(),
        arcs: Vec::new(),
        ellipses: Vec::new(),
        labels: Vec::new(),
    };
    let has_original_graphics = parts.iter().any(PartRaw::has_graphics);

    if !has_original_graphics && !has_part_rows {
        let layout_pins: Vec<common::PinRaw> = parts[0]
            .pins
            .iter()
            .map(|pin| common::PinRaw {
                id: pin.id.clone(),
                x_units: pin.x_units,
                y_units: pin.y_units,
                length_units: pin.length_units,
                rotation_degrees: pin.rotation_degrees,
            })
            .collect();
        let (placed_pins, laid_out_rect) = common::layout_pins(&layout_pins, &attr_by_parent);
        if !placed_pins.is_empty() {
            component.rectangles.push(Rectangle {
                corner1: common::CoordPoint::from_symbol_units(
                    laid_out_rect.x1_units,
                    laid_out_rect.height_units() - laid_out_rect.y1_units,
                ),
                corner2: common::CoordPoint::from_symbol_units(
                    laid_out_rect.x2_units,
                    laid_out_rect.height_units() - laid_out_rect.y2_units,
                ),
                color_bgr,
                fill_color_bgr,
                is_filled: !no_fill,
                is_transparent: no_fill,
                line_width_index: BODY_LINE_WIDTH_INDEX,
                owner_part_id: 1,
            });
            for pin in placed_pins {
                component.pins.push(Pin {
                    designator: pin.designator.clone(),
                    display_designator: pin.designator,
                    name: pin.name,
                    location: common::CoordPoint::from_symbol_units(
                        pin.x_units,
                        laid_out_rect.height_units() - pin.y_units,
                    ),
                    length_raw: common::raw_from_symbol_units(pin.length_units),
                    orientation: pin_orientation_from_easyeda_rotation(pin.rotation_degrees),
                    show_name: pin.show_name,
                    show_designator: pin.show_designator,
                    color_bgr,
                    owner_part_id: 1,
                    owner_part_display_mode: 0,
                });
            }
            add_metadata_implementation(&mut component, metadata);
            return Ok(component);
        }
    }

    let mut has_any_body = false;
    for part in &parts {
        let complex = part.has_complex_body();
        if !part.rectangles.is_empty() {
            for rect in &part.rectangles {
                component.rectangles.push(Rectangle {
                    corner1: common::CoordPoint::from_symbol_units(rect.x1_units, rect.y1_units),
                    corner2: common::CoordPoint::from_symbol_units(rect.x2_units, rect.y2_units),
                    color_bgr,
                    fill_color_bgr: if complex { WHITE_BGR } else { fill_color_bgr },
                    is_filled: !complex && !no_fill,
                    is_transparent: complex || no_fill,
                    line_width_index: BODY_LINE_WIDTH_INDEX,
                    owner_part_id: rect.owner_part_id,
                });
                has_any_body = true;
            }
        } else if let Some(bounds) = part.bounds.finish() {
            if !part.pins.is_empty() && !part.has_graphics() {
                component.rectangles.push(Rectangle {
                    corner1: common::CoordPoint::from_symbol_units(
                        bounds.min_x_units,
                        bounds.max_y_units,
                    ),
                    corner2: common::CoordPoint::from_symbol_units(
                        bounds.max_x_units,
                        bounds.min_y_units,
                    ),
                    color_bgr,
                    fill_color_bgr: if complex { WHITE_BGR } else { fill_color_bgr },
                    is_filled: !complex && !no_fill,
                    is_transparent: complex || no_fill,
                    line_width_index: BODY_LINE_WIDTH_INDEX,
                    owner_part_id: part.owner_part_id,
                });
                has_any_body = true;
            }
        }
        for polyline in &part.polylines {
            component.polylines.push(Polyline {
                points: polyline
                    .points
                    .iter()
                    .map(|point| {
                        common::CoordPoint::from_symbol_units(point.x_units, point.y_units)
                    })
                    .collect(),
                color_bgr,
                line_width_index: GRAPHIC_LINE_WIDTH_INDEX,
                owner_part_id: polyline.owner_part_id,
            });
            has_any_body = true;
        }
        for arc in &part.arcs {
            if let Some(converted) = arc_from_raw(arc) {
                component.arcs.push(converted);
            } else {
                component.polylines.push(Polyline {
                    points: vec![
                        common::CoordPoint::from_symbol_units(arc.start.x_units, arc.start.y_units),
                        common::CoordPoint::from_symbol_units(arc.mid.x_units, arc.mid.y_units),
                        common::CoordPoint::from_symbol_units(arc.end.x_units, arc.end.y_units),
                    ],
                    color_bgr,
                    line_width_index: GRAPHIC_LINE_WIDTH_INDEX,
                    owner_part_id: arc.owner_part_id,
                });
            }
            has_any_body = true;
        }
        for ellipse in &part.ellipses {
            component.ellipses.push(Ellipse {
                center: common::CoordPoint::from_symbol_units(
                    ellipse.center_x_units,
                    ellipse.center_y_units,
                ),
                radius_x_raw: common::raw_from_symbol_units(ellipse.radius_x_units),
                radius_y_raw: common::raw_from_symbol_units(ellipse.radius_y_units),
                color_bgr,
                fill_color_bgr: if ellipse.is_filled && !no_fill {
                    fill_color_bgr
                } else {
                    WHITE_BGR
                },
                is_filled: ellipse.is_filled && !no_fill,
                is_transparent: ellipse.is_transparent || no_fill,
                line_width_index: GRAPHIC_LINE_WIDTH_INDEX,
                owner_part_id: ellipse.owner_part_id,
            });
            has_any_body = true;
        }
        for label in &part.labels {
            component.labels.push(Label {
                text: label.text.clone(),
                location: common::CoordPoint::from_symbol_units(label.x_units, label.y_units),
                orientation: text_orientation_from_rotation(label.rotation_degrees),
                color_bgr,
                owner_part_id: label.owner_part_id,
            });
            has_any_body = true;
        }
        let graphics_bounds = part.pin_attachment_bounds();
        let min_attached_pin_length_units =
            if part.is_inductor_coil_symbol(&component.designator_text) {
                MIN_INDUCTOR_PIN_LENGTH_UNITS
            } else {
                0.0
            };
        for (pin_index, pin) in part.pins.iter().enumerate() {
            let attrs = attr_by_parent.get(&pin.id);
            let designator = common::safe_attr(attrs, "NUMBER")
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| (pin_index + 1).to_string());
            let name = common::safe_attr(attrs, "NAME")
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| designator.clone());
            let source_length_units = if pin.length_units > 0.000001 {
                pin.length_units
            } else {
                10.0
            };
            let pin_geometry = source_pin_geometry(
                pin,
                source_length_units,
                graphics_bounds,
                min_attached_pin_length_units,
            );
            let show_name = common::safe_attr_flag(attrs, "NAME", !name.trim().is_empty());
            let show_designator = common::safe_attr_flag(attrs, "NUMBER", true);
            let location = common::CoordPoint::from_symbol_units(
                pin_geometry.location_x_units,
                pin_geometry.location_y_units,
            );
            component.pins.push(Pin {
                designator: designator.clone(),
                display_designator: designator,
                name,
                location,
                length_raw: common::raw_from_symbol_units(pin_geometry.length_units),
                orientation: pin_geometry.orientation,
                show_name,
                show_designator,
                color_bgr,
                owner_part_id: pin.owner_part_id,
                owner_part_display_mode: 0,
            });
        }
    }

    if !has_any_body {
        if let Some(bounds) = global_bounds.finish() {
            component.rectangles.push(Rectangle {
                corner1: common::CoordPoint::from_symbol_units(
                    bounds.min_x_units,
                    bounds.max_y_units,
                ),
                corner2: common::CoordPoint::from_symbol_units(
                    bounds.max_x_units,
                    bounds.min_y_units,
                ),
                color_bgr,
                fill_color_bgr: WHITE_BGR,
                is_filled: false,
                is_transparent: true,
                line_width_index: BODY_LINE_WIDTH_INDEX,
                owner_part_id: 1,
            });
        }
    }

    add_metadata_implementation(&mut component, metadata);
    Ok(component)
}

fn add_metadata_implementation(component: &mut Component, metadata: &SchlibMetadata) {
    let Some(model_name) = metadata
        .footprint_model_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };

    let data_file_entities = metadata
        .footprint_library_file
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default();

    let mut seen_designators = HashSet::new();
    let mut map_definers = Vec::new();
    for pin in &component.pins {
        let designator = pin.designator.trim();
        if designator.is_empty() || !seen_designators.insert(designator.to_ascii_lowercase()) {
            continue;
        }
        map_definers.push(MapDefiner {
            designator_interface: designator.to_string(),
            designator_implementations: vec![designator.to_string()],
            is_trivial: true,
        });
    }

    component.implementations.push(Implementation {
        description: Some("PCB footprint".to_string()),
        model_name: model_name.to_string(),
        model_type: "PCBLIB".to_string(),
        is_current: true,
        data_file_kinds: vec!["PCBLib".to_string()],
        data_file_entities,
        map_definers,
    });
}

fn update_ellipse_bounds(bounds: &mut common::OptionalBounds, ellipse: &EllipseRaw) {
    bounds.update_x(
        ellipse.center_x_units - ellipse.radius_x_units,
        ellipse.center_x_units + ellipse.radius_x_units,
    );
    bounds.update_y(
        ellipse.center_y_units - ellipse.radius_y_units,
        ellipse.center_y_units + ellipse.radius_y_units,
    );
}

fn normalize_text_value(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn ensure_current_part_index(parts: &mut Vec<PartRaw>, current: &mut Option<usize>) -> usize {
    if current.is_none() {
        parts.push(PartRaw::new(1, None));
        *current = Some(0);
    }
    current.expect("part index")
}

fn parse_path_raw_points(shape: &Value) -> Vec<PointUnits> {
    match shape {
        Value::Array(values) => parse_path_array_points(values),
        Value::String(text) => parse_svg_path_points(text),
        _ => Vec::new(),
    }
}

fn parse_path_array_points(values: &[Value]) -> Vec<PointUnits> {
    let mut points = Vec::new();
    let mut index = 0usize;
    let mut start = None;
    let mut current = None;
    while index < values.len() {
        match &values[index] {
            Value::String(command) => {
                let cmd = command.trim().to_ascii_uppercase();
                index += 1;
                match cmd.as_str() {
                    "Z" | "CLOSE" => {
                        if let (Some(first), Some(last)) = (start, current) {
                            if !same_point(first, last) {
                                add_path_point(&mut points, first.x_units, first.y_units);
                                current = Some(first);
                            }
                        }
                    }
                    "M" | "L" => {
                        while index + 1 < values.len() {
                            let Some(x) = values.get(index).and_then(common::value_as_f64) else {
                                break;
                            };
                            let Some(y) = values.get(index + 1).and_then(common::value_as_f64)
                            else {
                                break;
                            };
                            add_path_point(&mut points, x, y);
                            current = Some(PointUnits {
                                x_units: x,
                                y_units: y,
                            });
                            if start.is_none() {
                                start = current;
                            }
                            index += 2;
                        }
                    }
                    "H" => {
                        while index < values.len() {
                            let Some(x) = values.get(index).and_then(common::value_as_f64) else {
                                break;
                            };
                            let y = current.map_or(0.0, |point| point.y_units);
                            add_path_point(&mut points, x, y);
                            current = Some(PointUnits {
                                x_units: x,
                                y_units: y,
                            });
                            if start.is_none() {
                                start = current;
                            }
                            index += 1;
                        }
                    }
                    "V" => {
                        while index < values.len() {
                            let Some(y) = values.get(index).and_then(common::value_as_f64) else {
                                break;
                            };
                            let x = current.map_or(0.0, |point| point.x_units);
                            add_path_point(&mut points, x, y);
                            current = Some(PointUnits {
                                x_units: x,
                                y_units: y,
                            });
                            if start.is_none() {
                                start = current;
                            }
                            index += 1;
                        }
                    }
                    "ARC" | "A" => {
                        if index + 2 < values.len() {
                            if let (Some(x), Some(y)) = (
                                values.get(index + 1).and_then(common::value_as_f64),
                                values.get(index + 2).and_then(common::value_as_f64),
                            ) {
                                add_path_point(&mut points, x, y);
                                current = Some(PointUnits {
                                    x_units: x,
                                    y_units: y,
                                });
                                if start.is_none() {
                                    start = current;
                                }
                                index += 3;
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                if index + 1 < values.len() {
                    if let (Some(x), Some(y)) = (
                        values.get(index).and_then(common::value_as_f64),
                        values.get(index + 1).and_then(common::value_as_f64),
                    ) {
                        add_path_point(&mut points, x, y);
                        current = Some(PointUnits {
                            x_units: x,
                            y_units: y,
                        });
                        if start.is_none() {
                            start = current;
                        }
                        index += 2;
                        continue;
                    }
                }
                index += 1;
            }
        }
    }
    points
}
fn parse_svg_path_points(text: &str) -> Vec<PointUnits> {
    let tokens = tokenize_svg_path(text);
    let mut points = Vec::new();
    let mut index = 0usize;
    let mut command = 'M';
    let mut current = PointUnits {
        x_units: 0.0,
        y_units: 0.0,
    };
    let mut start = None;
    while index < tokens.len() {
        if let Some(letter) = tokens[index]
            .chars()
            .next()
            .filter(|ch| tokens[index].len() == 1 && ch.is_ascii_alphabetic())
        {
            command = letter;
            index += 1;
            if matches!(command, 'Z' | 'z') {
                if let Some(first) = start {
                    if !same_point(first, current) {
                        add_path_point(&mut points, first.x_units, first.y_units);
                        current = first;
                    }
                }
                continue;
            }
        }
        match command {
            'M' | 'm' => {
                if index + 1 >= tokens.len() {
                    break;
                }
                let Some(mut x) = tokens[index].parse::<f64>().ok() else {
                    break;
                };
                let Some(mut y) = tokens[index + 1].parse::<f64>().ok() else {
                    break;
                };
                if command == 'm' {
                    x += current.x_units;
                    y += current.y_units;
                }
                add_path_point(&mut points, x, y);
                current = PointUnits {
                    x_units: x,
                    y_units: y,
                };
                if start.is_none() {
                    start = Some(current);
                }
                command = if command == 'm' { 'l' } else { 'L' };
                index += 2;
            }
            'L' | 'l' => {
                if index + 1 >= tokens.len() {
                    break;
                }
                let Some(mut x) = tokens[index].parse::<f64>().ok() else {
                    break;
                };
                let Some(mut y) = tokens[index + 1].parse::<f64>().ok() else {
                    break;
                };
                if command == 'l' {
                    x += current.x_units;
                    y += current.y_units;
                }
                add_path_point(&mut points, x, y);
                current = PointUnits {
                    x_units: x,
                    y_units: y,
                };
                if start.is_none() {
                    start = Some(current);
                }
                index += 2;
            }
            'H' | 'h' => {
                let Some(mut x) = tokens
                    .get(index)
                    .and_then(|token| token.parse::<f64>().ok())
                else {
                    break;
                };
                if command == 'h' {
                    x += current.x_units;
                }
                add_path_point(&mut points, x, current.y_units);
                current = PointUnits {
                    x_units: x,
                    y_units: current.y_units,
                };
                if start.is_none() {
                    start = Some(current);
                }
                index += 1;
            }
            'V' | 'v' => {
                let Some(mut y) = tokens
                    .get(index)
                    .and_then(|token| token.parse::<f64>().ok())
                else {
                    break;
                };
                if command == 'v' {
                    y += current.y_units;
                }
                add_path_point(&mut points, current.x_units, y);
                current = PointUnits {
                    x_units: current.x_units,
                    y_units: y,
                };
                if start.is_none() {
                    start = Some(current);
                }
                index += 1;
            }
            'A' | 'a' => {
                if index + 6 >= tokens.len() {
                    break;
                }
                let Some(mut x) = tokens[index + 5].parse::<f64>().ok() else {
                    break;
                };
                let Some(mut y) = tokens[index + 6].parse::<f64>().ok() else {
                    break;
                };
                if command == 'a' {
                    x += current.x_units;
                    y += current.y_units;
                }
                add_path_point(&mut points, x, y);
                current = PointUnits {
                    x_units: x,
                    y_units: y,
                };
                if start.is_none() {
                    start = Some(current);
                }
                index += 7;
            }
            'C' | 'c' => {
                if index + 5 >= tokens.len() {
                    break;
                }
                let Some(mut x) = tokens[index + 4].parse::<f64>().ok() else {
                    break;
                };
                let Some(mut y) = tokens[index + 5].parse::<f64>().ok() else {
                    break;
                };
                if command == 'c' {
                    x += current.x_units;
                    y += current.y_units;
                }
                add_path_point(&mut points, x, y);
                current = PointUnits {
                    x_units: x,
                    y_units: y,
                };
                if start.is_none() {
                    start = Some(current);
                }
                index += 6;
            }
            'Q' | 'q' | 'S' | 's' => {
                if index + 3 >= tokens.len() {
                    break;
                }
                let Some(mut x) = tokens[index + 2].parse::<f64>().ok() else {
                    break;
                };
                let Some(mut y) = tokens[index + 3].parse::<f64>().ok() else {
                    break;
                };
                if command.is_ascii_lowercase() {
                    x += current.x_units;
                    y += current.y_units;
                }
                add_path_point(&mut points, x, y);
                current = PointUnits {
                    x_units: x,
                    y_units: y,
                };
                if start.is_none() {
                    start = Some(current);
                }
                index += 4;
            }
            _ => index += 1,
        }
    }
    points
}

fn tokenize_svg_path(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_alphabetic() {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current.clear();
            }
            tokens.push(ch.to_string());
            continue;
        }
        if ch == ',' || ch.is_whitespace() {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current.clear();
            }
            continue;
        }
        if (ch == '-' || ch == '+') && !current.is_empty() && current != "e" && current != "E" {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current.clear();
            }
        }
        current.push(ch);
        if matches!(chars.peek(), Some(next) if next.is_ascii_alphabetic() || *next == ',' || next.is_whitespace())
        {
            if !current.trim().is_empty() {
                tokens.push(current.trim().to_string());
                current.clear();
            }
        }
    }
    if !current.trim().is_empty() {
        tokens.push(current.trim().to_string());
    }
    tokens
}

fn add_path_point(points: &mut Vec<PointUnits>, x_units: f64, y_units: f64) {
    let point = PointUnits { x_units, y_units };
    if points
        .last()
        .copied()
        .is_some_and(|last| same_point(last, point))
    {
        return;
    }
    points.push(point);
}
fn same_point(left: PointUnits, right: PointUnits) -> bool {
    (left.x_units - right.x_units).abs() < 1e-9 && (left.y_units - right.y_units).abs() < 1e-9
}

pub fn write_schlib(component: &Component, output_path: &Path) -> Result<()> {
    write_schlib_library(std::slice::from_ref(component), output_path)
}

pub fn write_schlib_library(components: &[Component], output_path: &Path) -> Result<()> {
    if components.is_empty() {
        return Err(crate::error::AppError::Other(
            "cannot write empty SchLib library".to_string(),
        ));
    }

    let sections = collect_sections(components);
    let file = File::create(output_path)?;
    let mut compound = cfb::CompoundFile::create(file)?;

    write_stream(&mut compound, "/FileHeader", &file_header_bytes(components))?;
    let section_keys = collect_section_key_pairs(&sections);
    if !section_keys.is_empty() {
        write_stream(
            &mut compound,
            "/SectionKeys",
            &section_keys_bytes(&section_keys),
        )?;
    }
    for (component, section_key) in &sections {
        compound.create_storage(&format!("/{section_key}/"))?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/Data"),
            &component_data_bytes(component),
        )?;
    }
    write_stream(&mut compound, "/Storage", &storage_bytes())?;
    compound.flush()?;
    Ok(())
}

impl Component {
    pub(crate) fn schlib_name(&self) -> &str {
        &self.name
    }

    pub(crate) fn schlib_data_bytes(&self) -> Vec<u8> {
        component_data_bytes(self)
    }
}

fn collect_sections<'a>(components: &'a [Component]) -> Vec<(&'a Component, String)> {
    let mut used = HashSet::new();
    components
        .iter()
        .map(|component| {
            let section_key = unique_section_key(&component.name, &mut used);
            (component, section_key)
        })
        .collect()
}

fn collect_section_key_pairs(sections: &[(&Component, String)]) -> Vec<(String, String)> {
    sections
        .iter()
        .filter_map(|(component, section_key)| {
            (section_key.as_str() != component.name.as_str())
                .then(|| (component.name.clone(), section_key.clone()))
        })
        .collect()
}

fn unique_section_key(name: &str, used: &mut HashSet<String>) -> String {
    let base = common::section_key_from_name(name);
    if used.insert(base.clone()) {
        return base;
    }

    let mut index = 2usize;
    loop {
        let suffix = format!("_{index}");
        let max_len = 31usize.saturating_sub(suffix.len());
        let prefix: String = base.chars().take(max_len.max(1)).collect();
        let candidate = format!("{prefix}{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn write_stream(
    compound: &mut cfb::CompoundFile<File>,
    stream_path: &str,
    data: &[u8],
) -> std::io::Result<()> {
    let mut stream = compound.create_stream(stream_path)?;
    stream.write_all(data)
}

fn file_header_bytes(components: &[Component]) -> Vec<u8> {
    let mut writer = common::BinaryWriter::default();
    let mut params = common::Params::default();
    params.push(
        "HEADER",
        "Protel for Windows - Schematic Library Editor Binary File Version 5.0",
    );
    params.push("WEIGHT", schlib_weight(components).to_string());
    params.push("MINORVERSION", "2");
    params.push("FONTIDCOUNT", "1");
    params.push("SIZE1", "4");
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
    params.push("COMPCOUNT", components.len().to_string());
    for (index, component) in components.iter().enumerate() {
        params.push(format!("LIBREF{index}"), &component.name);
        params.push(format!("COMPDESCR{index}"), &component.description);
        params.push(
            format!("PARTCOUNT{index}"),
            (component.part_count + 1).to_string(),
        );
    }
    writer.write_cstring_param_block(&params);
    writer.write_i32(components.len() as i32);
    for component in components {
        writer.write_string_block(&component.name);
    }
    writer.into_inner()
}

fn section_keys_bytes(section_keys: &[(String, String)]) -> Vec<u8> {
    let mut writer = common::BinaryWriter::default();
    let mut params = common::Params::default();
    params.push("KeyCount", section_keys.len().to_string());
    for (index, (component_name, section_key)) in section_keys.iter().enumerate() {
        params.push(format!("LibRef{index}"), component_name);
        params.push(format!("SectionKey{index}"), section_key);
    }
    writer.write_cstring_param_block(&params);
    writer.into_inner()
}
fn storage_bytes() -> Vec<u8> {
    let mut writer = common::BinaryWriter::default();
    let mut params = common::Params::default();
    params.push("HEADER", "Icon storage");
    writer.write_cstring_param_block(&params);
    writer.into_inner()
}
fn schlib_weight(components: &[Component]) -> usize {
    components.iter().map(component_weight).sum()
}

fn component_weight(component: &Component) -> usize {
    1 + component.rectangles.len()
        + component.labels.len()
        + component.polylines.len()
        + component.arcs.len()
        + component.ellipses.len()
        + component.pins.len()
        + component.parameters.len()
        + 2
        + implementation_record_count(component)
}

fn component_data_bytes(component: &Component) -> Vec<u8> {
    let mut writer = common::BinaryWriter::default();
    let mut params = common::Params::default();
    params.push("RECORD", "1");
    params.push("LIBREFERENCE", &component.name);
    params.push("COMPONENTDESCRIPTION", &component.description);
    params.push("PARTCOUNT", (component.part_count + 1).to_string());
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
        let mut p = common::Params::default();
        p.push("RECORD", "14");
        push_owned_part(&mut p, rect.owner_part_id);
        p.push_coord("LOCATION.X", rect.corner1.x_raw);
        p.push_coord("LOCATION.Y", rect.corner1.y_raw);
        p.push_coord("CORNER.X", rect.corner2.x_raw);
        p.push_coord("CORNER.Y", rect.corner2.y_raw);
        p.push("LINEWIDTH", rect.line_width_index.to_string());
        p.push_non_zero("COLOR", rect.color_bgr);
        p.push("AREACOLOR", rect.fill_color_bgr.to_string());
        p.push_bool("ISSOLID", rect.is_filled);
        p.push_bool("TRANSPARENT", rect.is_transparent);
        p.push(
            "UNIQUEID",
            stable_unique_id(
                &component.name,
                &format!("RECT{}_{index}", rect.owner_part_id),
            ),
        );
        writer.write_cstring_param_block(&p);
    }
    for (index, label) in component.labels.iter().enumerate() {
        let mut p = common::Params::default();
        p.push("RECORD", "4");
        push_owned_part(&mut p, label.owner_part_id);
        p.push_coord("LOCATION.X", label.location.x_raw);
        p.push_coord("LOCATION.Y", label.location.y_raw);
        p.push("FONTID", "1");
        p.push("TEXT", &label.text);
        p.push_non_zero("COLOR", label.color_bgr);
        p.push_non_zero("ORIENTATION", label.orientation as i32);
        p.push(
            "UNIQUEID",
            stable_unique_id(
                &component.name,
                &format!("TEXT{}_{index}", label.owner_part_id),
            ),
        );
        writer.write_cstring_param_block(&p);
    }
    for (index, polyline) in component.polylines.iter().enumerate() {
        let mut p = common::Params::default();
        p.push("RECORD", "6");
        push_owned_part(&mut p, polyline.owner_part_id);
        p.push("LINEWIDTH", polyline.line_width_index.to_string());
        p.push_non_zero("COLOR", polyline.color_bgr);
        p.push("LOCATIONCOUNT", polyline.points.len().to_string());
        for (point_index, point) in polyline.points.iter().enumerate() {
            p.push_coord(&format!("X{}", point_index + 1), point.x_raw);
            p.push_coord(&format!("Y{}", point_index + 1), point.y_raw);
        }
        p.push(
            "UNIQUEID",
            stable_unique_id(
                &component.name,
                &format!("POLY{}_{index}", polyline.owner_part_id),
            ),
        );
        writer.write_cstring_param_block(&p);
    }
    for (index, arc) in component.arcs.iter().enumerate() {
        let mut p = common::Params::default();
        p.push("RECORD", "12");
        push_owned_part(&mut p, arc.owner_part_id);
        p.push_coord("LOCATION.X", arc.center.x_raw);
        p.push_coord("LOCATION.Y", arc.center.y_raw);
        p.push_coord("RADIUS", arc.radius_raw);
        p.push("LINEWIDTH", arc.line_width_index.to_string());
        if arc.start_angle.abs() > f64::EPSILON {
            p.push("STARTANGLE", format_angle(arc.start_angle));
        }
        p.push("ENDANGLE", format_angle(arc.end_angle));
        p.push_non_zero("COLOR", arc.color_bgr);
        p.push(
            "UNIQUEID",
            stable_unique_id(
                &component.name,
                &format!("ARC{}_{index}", arc.owner_part_id),
            ),
        );
        writer.write_cstring_param_block(&p);
    }
    for (index, ellipse) in component.ellipses.iter().enumerate() {
        let mut p = common::Params::default();
        p.push("RECORD", "8");
        push_owned_part(&mut p, ellipse.owner_part_id);
        p.push_coord("LOCATION.X", ellipse.center.x_raw);
        p.push_coord("LOCATION.Y", ellipse.center.y_raw);
        p.push_coord("RADIUS", ellipse.radius_x_raw);
        p.push_coord("SECONDARYRADIUS", ellipse.radius_y_raw);
        p.push("LINEWIDTH", ellipse.line_width_index.to_string());
        p.push_non_zero("COLOR", ellipse.color_bgr);
        p.push("AREACOLOR", ellipse.fill_color_bgr.to_string());
        p.push_bool("ISSOLID", ellipse.is_filled);
        p.push_bool("TRANSPARENT", ellipse.is_transparent);
        p.push(
            "UNIQUEID",
            stable_unique_id(
                &component.name,
                &format!("ELLIPSE{}_{index}", ellipse.owner_part_id),
            ),
        );
        writer.write_cstring_param_block(&p);
    }
    for pin in &component.pins {
        writer.write_block(0x01, |w| {
            w.write_i32(2);
            w.write_u8(0);
            w.write_i16(pin.owner_part_id.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
            w.write_u8(pin.owner_part_display_mode as u8);
            w.write_u8(0);
            w.write_u8(0);
            w.write_u8(0);
            w.write_u8(0);
            w.write_pascal_short_string("");
            w.write_u8(0);
            w.write_u8(4);
            w.write_u8(pin_conglomerate(pin));
            w.write_i16(common::dxp_i16(pin.length_raw));
            w.write_i16(common::dxp_i16(pin.location.x_raw));
            w.write_i16(common::dxp_i16(pin.location.y_raw));
            w.write_i32(pin.color_bgr);
            w.write_pascal_short_string(&pin.name);
            w.write_pascal_short_string(&pin.display_designator);
            w.write_pascal_short_string("");
            w.write_pascal_short_string("");
            w.write_pascal_short_string("");
        });
    }
    for (index, parameter) in component.parameters.iter().enumerate() {
        let mut p = common::Params::default();
        p.push("RECORD", "41");
        p.push("OWNERPARTID", "-1");
        p.push("LOCATION.X_FRAC", "-5");
        p.push("LOCATION.Y_FRAC", "-15");
        p.push("COLOR", "8388608");
        p.push("FONTID", "1");
        p.push("ISHIDDEN", "T");
        p.push("TEXT", &parameter.value);
        p.push("NAME", &parameter.name);
        p.push(
            "UNIQUEID",
            stable_unique_id(&component.name, &format!("PARAM{index}_{}", parameter.name)),
        );
        writer.write_cstring_param_block(&p);
    }

    let mut d = common::Params::default();
    d.push("RECORD", "34");
    d.push("OWNERPARTID", "-1");
    d.push("LOCATION.X_FRAC", "-5");
    d.push("LOCATION.Y_FRAC", "5");
    d.push("COLOR", "8388608");
    d.push("FONTID", "1");
    d.push("TEXT", &component.designator_text);
    d.push("NAME", "Designator");
    d.push("READONLYSTATE", "1");
    d.push("UNIQUEID", stable_unique_id(&component.name, "DESIGNATOR"));
    writer.write_cstring_param_block(&d);
    let mut c = common::Params::default();
    c.push("RECORD", "41");
    c.push("OWNERPARTID", "-1");
    c.push("LOCATION.X_FRAC", "-5");
    c.push("LOCATION.Y_FRAC", "-15");
    c.push("COLOR", "8388608");
    c.push("FONTID", "1");
    c.push("TEXT", &component.comment_text);
    c.push("NAME", "Comment");
    c.push("UNIQUEID", stable_unique_id(&component.name, "COMMENT"));
    writer.write_cstring_param_block(&c);
    write_implementation_records(&mut writer, component);
    writer.into_inner()
}

fn implementation_record_count(component: &Component) -> usize {
    if component.implementations.is_empty() {
        1
    } else {
        1 + component
            .implementations
            .iter()
            .map(|implementation| 3 + implementation.map_definers.len())
            .sum::<usize>()
    }
}

fn write_implementation_records(writer: &mut common::BinaryWriter, component: &Component) {
    let mut list = common::Params::default();
    list.push("RECORD", "44");
    writer.write_cstring_param_block(&list);

    for (implementation_index, implementation) in component.implementations.iter().enumerate() {
        let mut implementation_params = common::Params::default();
        implementation_params.push("RECORD", "45");
        if let Some(description) = implementation.description.as_deref() {
            implementation_params.push("DESCRIPTION", description);
        }
        implementation_params.push("MODELNAME", &implementation.model_name);
        implementation_params.push("MODELTYPE", &implementation.model_type);
        implementation_params.push(
            "DATAFILECOUNT",
            implementation.data_file_kinds.len().to_string(),
        );
        for (data_file_index, kind) in implementation.data_file_kinds.iter().enumerate() {
            implementation_params.push(format!("MODELDATAFILEKIND{}", data_file_index + 1), kind);
            if let Some(entity) = implementation.data_file_entities.get(data_file_index) {
                implementation_params.push(
                    format!("MODELDATAFILEENTITY{}", data_file_index + 1),
                    entity,
                );
            }
        }
        implementation_params.push_bool("ISCURRENT", implementation.is_current);
        implementation_params.push(
            "UNIQUEID",
            stable_unique_id(
                &component.name,
                &format!("IMPL{implementation_index}_{}", implementation.model_name),
            ),
        );
        writer.write_cstring_param_block(&implementation_params);

        let mut map_definer_list = common::Params::default();
        map_definer_list.push("RECORD", "46");
        writer.write_cstring_param_block(&map_definer_list);

        for (map_index, map_definer) in implementation.map_definers.iter().enumerate() {
            let mut map_params = common::Params::default();
            map_params.push("RECORD", "47");
            map_params.push("DESINTF", &map_definer.designator_interface);
            map_params.push(
                "DESIMPCOUNT",
                map_definer.designator_implementations.len().to_string(),
            );
            for (designator_index, designator) in
                map_definer.designator_implementations.iter().enumerate()
            {
                map_params.push(format!("DESIMP{designator_index}"), designator);
            }
            map_params.push_bool("ISTRIVIAL", map_definer.is_trivial);
            map_params.push(
                "UNIQUEID",
                stable_unique_id(
                    &component.name,
                    &format!(
                        "MAP{implementation_index}_{map_index}_{}",
                        map_definer.designator_interface
                    ),
                ),
            );
            writer.write_cstring_param_block(&map_params);
        }

        let mut implementation_parameters = common::Params::default();
        implementation_parameters.push("RECORD", "48");
        writer.write_cstring_param_block(&implementation_parameters);
    }
}

fn push_owned_part(params: &mut common::Params, owner_part_id: i32) {
    params.push_bool("ISNOTACCESIBLE", true);
    params.push("OWNERPARTID", owner_part_id.to_string());
}
fn format_angle(angle: f64) -> String {
    format!("{:.3}", common::normalize_angle(angle))
}
fn text_orientation_from_rotation(rotation: f64) -> u8 {
    ((common::normalize_angle(rotation) / 90.0).round() as i32).rem_euclid(4) as u8
}
fn pin_orientation_from_easyeda_rotation(rotation: f64) -> u8 {
    ((common::normalize_angle(rotation) / 90.0).round() as i32).rem_euclid(4) as u8
}
fn source_pin_geometry(
    pin: &PinRaw,
    source_length_units: f64,
    graphics_bounds: Option<common::Bounds>,
    min_attached_length_units: f64,
) -> PinGeometry {
    let mut orientation = pin_orientation_from_easyeda_rotation(pin.rotation_degrees);
    let mut length_units = source_length_units;
    let mut location_x_units = pin.x_units;
    let mut location_y_units = pin.y_units;
    let mut attached_to_body = false;

    if let Some(bounds) = graphics_bounds {
        let horizontal = matches!(orientation, 0 | 2);
        if horizontal {
            if pin.x_units <= bounds.min_x_units {
                orientation = 2;
                length_units = bounds.min_x_units - pin.x_units;
                location_x_units = bounds.min_x_units;
                attached_to_body = true;
            } else if pin.x_units >= bounds.max_x_units {
                orientation = 0;
                length_units = pin.x_units - bounds.max_x_units;
                location_x_units = bounds.max_x_units;
                attached_to_body = true;
            }
        } else if pin.y_units <= bounds.min_y_units {
            orientation = 3;
            length_units = bounds.min_y_units - pin.y_units;
            location_y_units = bounds.min_y_units;
            attached_to_body = true;
        } else if pin.y_units >= bounds.max_y_units {
            orientation = 1;
            length_units = pin.y_units - bounds.max_y_units;
            location_y_units = bounds.max_y_units;
            attached_to_body = true;
        }
    }

    if !length_units.is_finite() || length_units <= 0.000001 {
        length_units = source_length_units;
        location_x_units = pin.x_units;
        location_y_units = pin.y_units;
        attached_to_body = false;
    }

    if attached_to_body && min_attached_length_units > 0.0 {
        length_units = length_units.max(min_attached_length_units);
    }

    PinGeometry {
        location_x_units,
        location_y_units,
        length_units,
        orientation,
    }
}
fn pin_body_location_from_easyeda(
    x_units: f64,
    y_units: f64,
    length_units: f64,
    rotation: f64,
) -> (f64, f64) {
    let angle = common::normalize_angle(rotation);
    let (dx_units, dy_units) = if !(45.0..315.0).contains(&angle) {
        (length_units, 0.0)
    } else if angle < 135.0 {
        (0.0, length_units)
    } else if angle < 225.0 {
        (-length_units, 0.0)
    } else {
        (0.0, -length_units)
    };
    (x_units - dx_units, y_units - dy_units)
}

fn arc_from_raw(raw: &ArcRaw) -> Option<Arc> {
    let (x1, y1, x2, y2, x3, y3) = (
        raw.start.x_units,
        raw.start.y_units,
        raw.mid.x_units,
        raw.mid.y_units,
        raw.end.x_units,
        raw.end.y_units,
    );
    let divisor = 2.0 * (x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2));
    if divisor.abs() <= 1e-9 {
        return None;
    }
    let x1_sq = x1 * x1 + y1 * y1;
    let x2_sq = x2 * x2 + y2 * y2;
    let x3_sq = x3 * x3 + y3 * y3;
    let cx = (x1_sq * (y2 - y3) + x2_sq * (y3 - y1) + x3_sq * (y1 - y2)) / divisor;
    let cy = (x1_sq * (x3 - x2) + x2_sq * (x1 - x3) + x3_sq * (x2 - x1)) / divisor;
    let radius = ((x1 - cx).powi(2) + (y1 - cy).powi(2)).sqrt();
    if !radius.is_finite() || radius <= 1e-9 {
        return None;
    }
    let mut start_angle = point_angle_degrees(cx, cy, x1, y1);
    let mid_angle = point_angle_degrees(cx, cy, x2, y2);
    let mut end_angle = point_angle_degrees(cx, cy, x3, y3);
    if !angle_lies_on_ccw_path(start_angle, mid_angle, end_angle) {
        std::mem::swap(&mut start_angle, &mut end_angle);
    }
    Some(Arc {
        center: common::CoordPoint::from_symbol_units(cx, cy),
        radius_raw: common::raw_from_symbol_units(radius),
        start_angle,
        end_angle,
        color_bgr: common::SYMBOL_BGR,
        line_width_index: GRAPHIC_LINE_WIDTH_INDEX,
        owner_part_id: raw.owner_part_id,
    })
}

fn point_angle_degrees(cx: f64, cy: f64, x: f64, y: f64) -> f64 {
    common::normalize_angle((y - cy).atan2(x - cx).to_degrees())
}
fn angle_lies_on_ccw_path(start: f64, mid: f64, end: f64) -> bool {
    angle_delta_ccw(start, mid) <= angle_delta_ccw(start, end) + 1e-6
}
fn angle_delta_ccw(start: f64, end: f64) -> f64 {
    let mut delta = common::normalize_angle(end) - common::normalize_angle(start);
    if delta < 0.0 {
        delta += 360.0;
    }
    delta
}
fn stable_unique_id(name: &str, salt: &str) -> String {
    common::stable_unique_id(name, salt)
}
fn normalize_component_name(name: &str) -> String {
    common::normalize_component_name(name)
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

#[derive(Debug, Clone, Copy)]
struct PointUnits {
    x_units: f64,
    y_units: f64,
}
#[derive(Debug, Clone)]
struct PartRaw {
    owner_part_id: i32,
    bounds: common::OptionalBounds,
    pins: Vec<PinRaw>,
    rectangles: Vec<RectRaw>,
    polylines: Vec<PolylineRaw>,
    arcs: Vec<ArcRaw>,
    ellipses: Vec<EllipseRaw>,
    labels: Vec<TextRaw>,
}
impl PartRaw {
    fn new(owner_part_id: i32, declared_bounds: Option<common::Bounds>) -> Self {
        let mut bounds = common::OptionalBounds::default();
        if let Some(bounds_decl) = declared_bounds {
            bounds.update_x(bounds_decl.min_x_units, bounds_decl.max_x_units);
            bounds.update_y(bounds_decl.min_y_units, bounds_decl.max_y_units);
        }
        Self {
            owner_part_id,
            bounds,
            pins: Vec::new(),
            rectangles: Vec::new(),
            polylines: Vec::new(),
            arcs: Vec::new(),
            ellipses: Vec::new(),
            labels: Vec::new(),
        }
    }
    fn has_graphics(&self) -> bool {
        !self.rectangles.is_empty()
            || !self.polylines.is_empty()
            || !self.arcs.is_empty()
            || !self.ellipses.is_empty()
            || !self.labels.is_empty()
    }
    fn has_complex_body(&self) -> bool {
        !self.polylines.is_empty() || !self.arcs.is_empty()
    }
    fn graphics_bounds(&self) -> Option<common::Bounds> {
        let mut bounds = common::OptionalBounds::default();
        for rect in &self.rectangles {
            bounds.update_x(
                rect.x1_units.min(rect.x2_units),
                rect.x1_units.max(rect.x2_units),
            );
            bounds.update_y(
                rect.y1_units.min(rect.y2_units),
                rect.y1_units.max(rect.y2_units),
            );
        }
        for polyline in &self.polylines {
            for point in &polyline.points {
                bounds.update_x(point.x_units, point.x_units);
                bounds.update_y(point.y_units, point.y_units);
            }
        }
        for arc in &self.arcs {
            bounds.update_x(
                arc.start.x_units.min(arc.mid.x_units).min(arc.end.x_units),
                arc.start.x_units.max(arc.mid.x_units).max(arc.end.x_units),
            );
            bounds.update_y(
                arc.start.y_units.min(arc.mid.y_units).min(arc.end.y_units),
                arc.start.y_units.max(arc.mid.y_units).max(arc.end.y_units),
            );
        }
        for ellipse in &self.ellipses {
            bounds.update_x(
                ellipse.center_x_units - ellipse.radius_x_units,
                ellipse.center_x_units + ellipse.radius_x_units,
            );
            bounds.update_y(
                ellipse.center_y_units - ellipse.radius_y_units,
                ellipse.center_y_units + ellipse.radius_y_units,
            );
        }
        bounds.finish()
    }

    fn pin_attachment_bounds(&self) -> Option<common::Bounds> {
        let mut rectangular_bounds = common::OptionalBounds::default();
        for rect in &self.rectangles {
            rectangular_bounds.update_x(
                rect.x1_units.min(rect.x2_units),
                rect.x1_units.max(rect.x2_units),
            );
            rectangular_bounds.update_y(
                rect.y1_units.min(rect.y2_units),
                rect.y1_units.max(rect.y2_units),
            );
        }
        if let Some(bounds) = rectangular_bounds.finish() {
            return Some(bounds);
        }

        let closed_polylines: Vec<(common::Bounds, f64)> = self
            .polylines
            .iter()
            .filter_map(closed_polyline_bounds)
            .collect();
        let largest_area = closed_polylines
            .iter()
            .map(|(_, area)| *area)
            .fold(0.0_f64, f64::max);
        if largest_area > 0.0 {
            let mut body_bounds = common::OptionalBounds::default();
            for (bounds, area) in closed_polylines {
                if area + 1e-9 < largest_area * 0.5 {
                    continue;
                }
                body_bounds.update_x(bounds.min_x_units, bounds.max_x_units);
                body_bounds.update_y(bounds.min_y_units, bounds.max_y_units);
            }
            if let Some(bounds) = body_bounds.finish() {
                return Some(bounds);
            }
        }

        self.graphics_bounds()
    }

    fn is_inductor_coil_symbol(&self, designator_text: &str) -> bool {
        if !designator_text.trim().to_ascii_uppercase().starts_with('L') {
            return false;
        }
        if self.pins.len() != 2
            || self.polylines.len() < 2
            || !self.rectangles.is_empty()
            || !self.arcs.is_empty()
            || !self.ellipses.is_empty()
        {
            return false;
        }
        if !self.pins.iter().all(|pin| {
            matches!(
                pin_orientation_from_easyeda_rotation(pin.rotation_degrees),
                0 | 2
            )
        }) {
            return false;
        }

        let Some(bounds) = self.graphics_bounds() else {
            return false;
        };
        let min_pin_x = self
            .pins
            .iter()
            .map(|pin| pin.x_units)
            .fold(f64::INFINITY, f64::min);
        let max_pin_x = self
            .pins
            .iter()
            .map(|pin| pin.x_units)
            .fold(f64::NEG_INFINITY, f64::max);
        if min_pin_x >= bounds.min_x_units || max_pin_x <= bounds.max_x_units {
            return false;
        }

        self.polylines
            .iter()
            .filter(|polyline| is_open_coil_polyline(polyline))
            .count()
            >= 2
    }
}

fn is_open_coil_polyline(polyline: &PolylineRaw) -> bool {
    if polyline.points.len() < 6 {
        return false;
    }
    let Some(first) = polyline.points.first() else {
        return false;
    };
    let Some(last) = polyline.points.last() else {
        return false;
    };
    if (first.x_units - last.x_units).abs() <= 1e-6 && (first.y_units - last.y_units).abs() <= 1e-6
    {
        return false;
    }

    let mut bounds = common::OptionalBounds::default();
    for point in &polyline.points {
        bounds.update_x(point.x_units, point.x_units);
        bounds.update_y(point.y_units, point.y_units);
    }
    let Some(bounds) = bounds.finish() else {
        return false;
    };
    let width = bounds.max_x_units - bounds.min_x_units;
    let height = bounds.max_y_units - bounds.min_y_units;
    if width <= 0.0 || height <= 0.5 || width < height {
        return false;
    }

    let low_band = bounds.min_y_units + height * 0.35;
    let endpoints_start_near_low = first.y_units <= low_band && last.y_units <= low_band;
    let high_peak = bounds.min_y_units + height * 0.65;
    endpoints_start_near_low
        && polyline
            .points
            .iter()
            .any(|point| point.y_units >= high_peak)
}

fn closed_polyline_bounds(polyline: &PolylineRaw) -> Option<(common::Bounds, f64)> {
    let first = polyline.points.first()?;
    let last = polyline.points.last()?;
    if polyline.points.len() < 4
        || (first.x_units - last.x_units).abs() > 1e-6
        || (first.y_units - last.y_units).abs() > 1e-6
    {
        return None;
    }

    let mut bounds = common::OptionalBounds::default();
    for point in &polyline.points {
        bounds.update_x(point.x_units, point.x_units);
        bounds.update_y(point.y_units, point.y_units);
    }
    let bounds = bounds.finish()?;
    let area = (bounds.max_x_units - bounds.min_x_units).abs()
        * (bounds.max_y_units - bounds.min_y_units).abs();
    (area > 1e-9).then_some((bounds, area))
}
#[derive(Debug, Clone)]
struct PinRaw {
    id: String,
    x_units: f64,
    y_units: f64,
    length_units: f64,
    rotation_degrees: f64,
    owner_part_id: i32,
}
#[derive(Debug, Clone, Copy)]
struct RectRaw {
    x1_units: f64,
    y1_units: f64,
    x2_units: f64,
    y2_units: f64,
    owner_part_id: i32,
}
#[derive(Debug, Clone)]
struct PolylineRaw {
    points: Vec<PointUnits>,
    owner_part_id: i32,
}
#[derive(Debug, Clone, Copy)]
struct ArcRaw {
    start: PointUnits,
    mid: PointUnits,
    end: PointUnits,
    owner_part_id: i32,
}
#[derive(Debug, Clone, Copy)]
struct EllipseRaw {
    center_x_units: f64,
    center_y_units: f64,
    radius_x_units: f64,
    radius_y_units: f64,
    owner_part_id: i32,
    is_filled: bool,
    is_transparent: bool,
}
#[derive(Debug, Clone)]
struct TextRaw {
    text: String,
    x_units: f64,
    y_units: f64,
    rotation_degrees: f64,
    owner_part_id: i32,
}
#[derive(Debug)]
struct Pin {
    designator: String,
    display_designator: String,
    name: String,
    location: common::CoordPoint,
    length_raw: i64,
    orientation: u8,
    show_name: bool,
    show_designator: bool,
    color_bgr: i32,
    owner_part_id: i32,
    owner_part_display_mode: i32,
}
#[derive(Debug, Clone, Copy)]
struct PinGeometry {
    location_x_units: f64,
    location_y_units: f64,
    length_units: f64,
    orientation: u8,
}
#[derive(Debug)]
struct Rectangle {
    corner1: common::CoordPoint,
    corner2: common::CoordPoint,
    color_bgr: i32,
    fill_color_bgr: i32,
    is_filled: bool,
    is_transparent: bool,
    line_width_index: i32,
    owner_part_id: i32,
}
#[derive(Debug)]
struct Polyline {
    points: Vec<common::CoordPoint>,
    color_bgr: i32,
    line_width_index: i32,
    owner_part_id: i32,
}
#[derive(Debug)]
struct Arc {
    center: common::CoordPoint,
    radius_raw: i64,
    start_angle: f64,
    end_angle: f64,
    color_bgr: i32,
    line_width_index: i32,
    owner_part_id: i32,
}
#[derive(Debug)]
struct Ellipse {
    center: common::CoordPoint,
    radius_x_raw: i64,
    radius_y_raw: i64,
    color_bgr: i32,
    fill_color_bgr: i32,
    is_filled: bool,
    is_transparent: bool,
    line_width_index: i32,
    owner_part_id: i32,
}
#[derive(Debug)]
struct Label {
    text: String,
    location: common::CoordPoint,
    orientation: u8,
    color_bgr: i32,
    owner_part_id: i32,
}
#[derive(Debug)]
pub struct Component {
    name: String,
    description: String,
    designator_text: String,
    comment_text: String,
    parameters: Vec<SchlibParameter>,
    implementations: Vec<Implementation>,
    part_count: usize,
    pins: Vec<Pin>,
    rectangles: Vec<Rectangle>,
    polylines: Vec<Polyline>,
    arcs: Vec<Arc>,
    ellipses: Vec<Ellipse>,
    labels: Vec<Label>,
}

#[derive(Debug)]
struct Implementation {
    description: Option<String>,
    model_name: String,
    model_type: String,
    is_current: bool,
    data_file_kinds: Vec<String>,
    data_file_entities: Vec<String>,
    map_definers: Vec<MapDefiner>,
}

#[derive(Debug)]
struct MapDefiner {
    designator_interface: String,
    designator_implementations: Vec<String>,
    is_trivial: bool,
}

#[cfg(test)]
mod tests {
    use super::common;
    use super::{
        SchlibMetadata, SchlibParameter, build_component_from_payload,
        write_schlib_from_payload_with_metadata, write_schlib_library,
    };
    use serde_json::json;
    use std::fs::File;
    use std::io::Read;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_payload() -> serde_json::Value {
        json!({"result": {"dataStr": r#"["DOCTYPE","SYMBOL","1.1"]
["PART","U.1",{"BBOX":[-10,-10,10,10]}]
["RECT","body",-10,-10,10,10,0,0,0,"st1",0]
["ATTR","root1","","Symbol","TEST",false,false,null,null,0,"st3",0]
["ATTR","root2","","Designator","U?",false,false,null,null,0,"st3",0]
["PIN","p1",1,null,-20,0,10,0,null,0,0,1]
["ATTR","p1n","p1","NAME","A",false,true,-5,0,0,"st3",0]
["ATTR","p1d","p1","NUMBER","1",false,true,-10,0,0,"st4",0]
["PIN","p2",1,null,20,0,10,180,null,0,0,1]
["ATTR","p2n","p2","NAME","B",false,true,5,0,0,"st3",0]
["ATTR","p2d","p2","NUMBER","2",false,true,10,0,0,"st4",0]"#}})
    }

    #[test]
    fn writes_metadata_records_into_schlib() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("export_schlib_meta_{timestamp}.SchLib"));
        let metadata = SchlibMetadata {
            description: Some("CPU Core: -; CPU Maximum Speed: 133MHz;".to_string()),
            designator: Some("U?".to_string()),
            comment: Some("={Manufacturer Part}".to_string()),
            parameters: vec![
                SchlibParameter {
                    name: "Footprint".to_string(),
                    value: "LQFN-56_L7.0-W7.0-P0.4-EP".to_string(),
                },
                SchlibParameter {
                    name: "Manufacturer".to_string(),
                    value: "Raspberry Pi".to_string(),
                },
            ],
            footprint_model_name: Some("LQFN-56_L7.0-W7.0-P0.4-EP".to_string()),
            footprint_library_file: Some("MyLib.PcbLib".to_string()),
        };

        write_schlib_from_payload_with_metadata(&sample_payload(), "TEST/COMP", &metadata, &path)
            .unwrap();

        let file = File::open(&path).unwrap();
        let mut compound = cfb::CompoundFile::open(file).unwrap();
        let mut data_stream = compound.open_stream("/TEST_COMP/Data").unwrap();
        let mut data = Vec::new();
        data_stream.read_to_end(&mut data).unwrap();
        let data_text = String::from_utf8_lossy(&data);

        assert!(
            data_text.contains("|COMPONENTDESCRIPTION=CPU Core: -; CPU Maximum Speed: 133MHz;|")
        );
        assert!(data_text.contains("|RECORD=34|"));
        assert!(data_text.contains("|NAME=Designator|"));
        assert!(data_text.contains("|TEXT=U?|"));
        assert!(data_text.contains("|NAME=Comment|"));
        assert!(data_text.contains("|TEXT=={Manufacturer Part}|"));
        assert!(!data_text.contains("|ISHIDDEN=T|TEXT=={Manufacturer Part}|NAME=Comment|"));
        assert!(data_text.contains("|NAME=Footprint|"));
        assert!(data_text.contains("|TEXT=LQFN-56_L7.0-W7.0-P0.4-EP|"));
        assert!(data_text.contains("|NAME=Manufacturer|"));
        assert!(data_text.contains("|TEXT=Raspberry Pi|"));
        assert!(data_text.contains("|RECORD=45|"));
        assert!(data_text.contains("|MODELNAME=LQFN-56_L7.0-W7.0-P0.4-EP|"));
        assert!(data_text.contains("|MODELTYPE=PCBLIB|"));
        assert!(data_text.contains("|MODELDATAFILEKIND1=PCBLib|"));
        assert!(data_text.contains("|MODELDATAFILEENTITY1=MyLib.PcbLib|"));
        assert!(data_text.contains("|RECORD=46"));
        assert!(data_text.contains("|RECORD=47|"));
        assert!(data_text.contains("|DESINTF=1|"));
        assert!(data_text.contains("|DESIMP0=1|"));
        assert!(data_text.contains("|RECORD=48"));
    }

    #[test]
    fn writes_multi_component_schlib_with_unique_section_keys() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("export_schlib_multi_{timestamp}.SchLib"));
        let name_a = format!("{}1", "A".repeat(31));
        let name_b = format!("{}2", "A".repeat(31));
        let component_a = build_component_from_payload(&sample_payload(), &name_a).unwrap();
        let component_b = build_component_from_payload(&sample_payload(), &name_b).unwrap();

        write_schlib_library(&[component_a, component_b], &path).unwrap();

        let file = File::open(&path).unwrap();
        let mut compound = cfb::CompoundFile::open(file).unwrap();
        let mut header_stream = compound.open_stream("/FileHeader").unwrap();
        let mut header = Vec::new();
        header_stream.read_to_end(&mut header).unwrap();
        let header_text = String::from_utf8_lossy(&header);
        assert!(header_text.contains("|COMPCOUNT=2|"));

        let mut section_keys_stream = compound.open_stream("/SectionKeys").unwrap();
        let mut section_keys = Vec::new();
        section_keys_stream.read_to_end(&mut section_keys).unwrap();
        let section_keys_text = String::from_utf8_lossy(&section_keys);
        assert!(section_keys_text.contains("|KeyCount=2|"));

        let first_key = "A".repeat(31);
        let second_key = format!("{}{}", "A".repeat(29), "_2");
        assert!(compound.open_stream(&format!("/{first_key}/Data")).is_ok());
        assert!(compound.open_stream(&format!("/{second_key}/Data")).is_ok());

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn schematic_graphics_use_black_outline_colors() {
        let component = build_component_from_payload(&sample_payload(), "TEST/COMP").unwrap();

        assert!(
            component
                .rectangles
                .iter()
                .all(|item| item.color_bgr == common::SYMBOL_BGR)
        );
        assert!(
            component
                .polylines
                .iter()
                .all(|item| item.color_bgr == common::SYMBOL_BGR)
        );
        assert!(
            component
                .labels
                .iter()
                .all(|item| item.color_bgr == common::SYMBOL_BGR)
        );
        assert!(
            component
                .pins
                .iter()
                .all(|item| item.color_bgr == common::SYMBOL_BGR)
        );
    }

    #[test]
    fn easyeda_pin_orientation_points_outward_from_body() {
        let component = build_component_from_payload(&sample_payload(), "TEST/COMP").unwrap();
        let pin_1 = component
            .pins
            .iter()
            .find(|pin| pin.designator == "1")
            .unwrap();
        let pin_2 = component
            .pins
            .iter()
            .find(|pin| pin.designator == "2")
            .unwrap();

        assert_eq!(pin_1.orientation, 2);
        assert_eq!(pin_2.orientation, 0);
        assert_eq!(pin_1.location.x_raw, common::raw_from_symbol_units(-10.0));
        assert_eq!(pin_2.location.x_raw, common::raw_from_symbol_units(10.0));
        assert_eq!(pin_1.length_raw, common::raw_from_symbol_units(10.0));
        assert_eq!(pin_2.length_raw, common::raw_from_symbol_units(10.0));
        assert!(pin_1.show_name);
        assert!(pin_1.show_designator);
        assert_eq!(pin_1.name, "A");
        assert_eq!(pin_1.display_designator, "1");
    }

    #[test]
    fn source_symbol_graphics_keep_y_coordinates_and_pins_point_outward() {
        let payload = json!({"result": {"dataStr": r#"["DOCTYPE","SYMBOL","1.1"]
["PART","U.1",{"BBOX":[0,0,100,100]}]
["RECT","body",10,20,30,40,0,0,0,"st1",0]
["PIN","top",1,null,50,10,10,90,null,0,0,1]
["ATTR","topn","top","NAME","TOP",false,true,50,0,0,"st3",0]
["ATTR","topd","top","NUMBER","1",false,true,50,0,0,"st4",0]
["PIN","bottom",1,null,50,90,10,270,null,0,0,1]
["ATTR","bottomn","bottom","NAME","BOTTOM",false,true,50,0,0,"st3",0]
["ATTR","bottomd","bottom","NUMBER","2",false,true,50,0,0,"st4",0]"#}});

        let component = build_component_from_payload(&payload, "TEST/COMP").unwrap();
        let rect = component.rectangles.first().unwrap();
        let top_pin = component
            .pins
            .iter()
            .find(|pin| pin.designator == "1")
            .unwrap();
        let bottom_pin = component
            .pins
            .iter()
            .find(|pin| pin.designator == "2")
            .unwrap();

        assert_eq!(rect.corner1.y_raw, common::raw_from_symbol_units(20.0));
        assert_eq!(rect.corner2.y_raw, common::raw_from_symbol_units(40.0));
        assert_eq!(top_pin.location.y_raw, common::raw_from_symbol_units(20.0));
        assert_eq!(top_pin.length_raw, common::raw_from_symbol_units(10.0));
        assert_eq!(top_pin.orientation, 3);
        assert_eq!(
            bottom_pin.location.y_raw,
            common::raw_from_symbol_units(40.0)
        );
        assert_eq!(bottom_pin.length_raw, common::raw_from_symbol_units(50.0));
        assert_eq!(bottom_pin.orientation, 1);
    }

    #[test]
    fn inductor_coil_pins_point_outward_and_use_altium_pin_length() {
        let payload = json!({"result": {"dataStr": r#"["DOCTYPE","SYMBOL","1.1"]
["PART","L.1",{"BBOX":[-17.4,-0.45,17.2,4.86]}]
["ATTR","designator","","Designator","L?",false,false,null,null,0,"st3",0]
["PIN","right",1,null,20,0,3,180,null,0,0,1]
["ATTR","rightn","right","NAME","2",false,false,13,-1.9,0,"st4",0]
["ATTR","rightd","right","NUMBER","2",false,false,21,2.1,0,"st3",0]
["PIN","left",1,null,-20,0,3,0,null,0,0,1]
["ATTR","leftn","left","NAME","1",false,false,-13,-1.9,0,"st3",0]
["ATTR","leftd","left","NUMBER","1",false,false,-21,2.1,0,"st4",0]
["POLY","coil1",[-16.88,0.06,-16.68,1.70,-15.82,3.10,-14.48,4.03,-12.89,4.35,-11.31,4.03,-9.96,3.10,-9.10,1.70,-8.92,0.05],0,"st1",0]
["POLY","coil2",[-8.40,0.06,-8.20,1.70,-7.34,3.10,-6.00,4.04,-4.42,4.36,-2.83,4.04,-1.49,3.10,-0.63,1.70,-0.44,0.06],0,"st1",0]
["POLY","coil3",[0.07,0.06,0.26,1.70,1.12,3.10,2.46,4.04,4.05,4.36,5.64,4.04,6.98,3.10,7.84,1.70,8.03,0.06],0,"st1",0]
["POLY","coil4",[8.70,0.06,8.91,1.69,9.78,3.08,11.11,3.99,12.69,4.30,14.27,3.98,15.60,3.07,16.46,1.68,16.67,0.05],0,"st1",0]"#}});

        let component = build_component_from_payload(&payload, "TEST/INDUCTOR").unwrap();
        let left = component
            .pins
            .iter()
            .find(|pin| pin.designator == "1")
            .unwrap();
        let right = component
            .pins
            .iter()
            .find(|pin| pin.designator == "2")
            .unwrap();

        assert_eq!(left.orientation, 2);
        assert_eq!(right.orientation, 0);
        assert_eq!(left.location.x_raw, common::raw_from_symbol_units(-16.88));
        assert_eq!(right.location.x_raw, common::raw_from_symbol_units(16.67));
        assert_eq!(
            left.length_raw,
            common::raw_from_symbol_units(super::MIN_INDUCTOR_PIN_LENGTH_UNITS)
        );
        assert_eq!(
            right.length_raw,
            common::raw_from_symbol_units(super::MIN_INDUCTOR_PIN_LENGTH_UNITS)
        );
    }

    #[test]
    fn decorative_led_arrows_do_not_move_pin_attachment_edges() {
        let payload = json!({"result": {"dataStr": r#"["DOCTYPE","SYMBOL","1.1"]
["PART","LED.1",{"BBOX":[-20,-10,20,10]}]
["PIN","right",1,null,20,0,15,180,null,0,0,1]
["ATTR","rightn","right","NAME","K",false,true,20,0,0,"st3",0]
["ATTR","rightd","right","NUMBER","1",false,true,20,0,0,"st4",0]
["PIN","left",1,null,-20,0,15,0,null,0,0,1]
["ATTR","leftn","left","NAME","A",false,true,-20,0,0,"st3",0]
["ATTR","leftd","left","NUMBER","2",false,true,-20,0,0,"st4",0]
["POLY","body",[5,-6,-5,0,5,7,5,-6],0,0,0,1]
["POLY","bar",[-5,8,-5,-8],0,0,0,1]
["POLY","arrow",[-15,-8,-10,-5,-13,-4,-15,-8],0,0,0,1]"#}});

        let component = build_component_from_payload(&payload, "TEST/LED").unwrap();
        let right = component
            .pins
            .iter()
            .find(|pin| pin.designator == "1")
            .unwrap();
        let left = component
            .pins
            .iter()
            .find(|pin| pin.designator == "2")
            .unwrap();

        assert_eq!(right.location.x_raw, common::raw_from_symbol_units(5.0));
        assert_eq!(left.location.x_raw, common::raw_from_symbol_units(-5.0));
        assert_eq!(right.length_raw, common::raw_from_symbol_units(15.0));
        assert_eq!(left.length_raw, common::raw_from_symbol_units(15.0));
        assert_eq!(right.orientation, 0);
        assert_eq!(left.orientation, 2);
    }

    #[test]
    fn a_sentinel_fill_colour_exports_a_transparent_area() {
        let build = |fill: i32| {
            super::build_component_from_payload_with_metadata_and_color(
                &sample_payload(),
                "TEST/COMP",
                &SchlibMetadata::default(),
                0x000000,
                fill,
            )
            .unwrap()
        };
        let colored = build(0xFFFFE0);
        let no_fill = build(super::NO_FILL_BGR);

        assert!(!colored.rectangles.is_empty(), "the fixture draws a body");
        assert!(
            colored
                .rectangles
                .iter()
                .all(|rect| rect.is_filled && !rect.is_transparent)
        );
        assert!(
            no_fill
                .rectangles
                .iter()
                .all(|rect| !rect.is_filled && rect.is_transparent),
            "no colour must leave the body unfilled, whatever the shape asked for"
        );

        // Altium writes a boolean only when it is set, so "not filled" is the
        // absence of `ISSOLID` rather than an `ISSOLID=F`.
        let filled_text = schlib_text(&colored, "filled");
        let none_text = schlib_text(&no_fill, "none");
        assert!(filled_text.contains("|ISSOLID=T|"));
        assert!(none_text.contains("|TRANSPARENT=T|"));
        assert!(!none_text.contains("|ISSOLID=T|"));
    }

    /// Writes `component` to a scratch SchLib and returns its record text.
    fn schlib_text(component: &super::Component, tag: &str) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("export_schlib_{tag}_{timestamp}.SchLib"));
        super::write_schlib(component, &path).unwrap();

        let file = File::open(&path).unwrap();
        let mut compound = cfb::CompoundFile::open(file).unwrap();
        let mut data_stream = compound.open_stream("/TEST_COMP/Data").unwrap();
        let mut data = Vec::new();
        data_stream.read_to_end(&mut data).unwrap();
        String::from_utf8_lossy(&data).into_owned()
    }
}
