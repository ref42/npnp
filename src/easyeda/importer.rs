use crate::easyeda::models::*;
use crate::error::{EasyedaError, Result};

pub struct SymbolImporter;

impl SymbolImporter {
    pub fn parse(data_str: &[String]) -> Result<EeSymbol> {
        log::debug!("Parsing symbol with {} shapes", data_str.len());

        let mut symbol = EeSymbol {
            name: String::new(),
            prefix: String::new(),
            pins: Vec::new(),
            rectangles: Vec::new(),
            circles: Vec::new(),
            ellipses: Vec::new(),
            arcs: Vec::new(),
            polylines: Vec::new(),
            polygons: Vec::new(),
            paths: Vec::new(),
            texts: Vec::new(),
        };

        for shape in data_str {
            if shape.is_empty() {
                continue;
            }

            // Split by "~" to get fields
            let fields: Vec<&str> = shape.split('~').collect();
            if fields.is_empty() {
                continue;
            }

            let designator = fields[0];

            log::debug!(
                "Shape designator: '{}', fields: {}",
                designator,
                fields.len()
            );

            match designator {
                "P" => {
                    // Pin: P~show~electric~x~y~rotation~gId~locked~spicePin~id
                    // Pins contain multiple segments separated by ^^
                    log::debug!("Parsing pin: {}", shape);
                    if let Ok(pin) = Self::parse_pin(shape) {
                        log::debug!(
                            "Successfully parsed pin: {} at ({}, {})",
                            pin.number,
                            pin.x,
                            pin.y
                        );
                        symbol.pins.push(pin);
                    } else {
                        log::warn!("Failed to parse pin from: {}", shape);
                    }
                }
                "R" => {
                    // Rectangle: R~x~y~width~height~id~locked~layerid~type~fill
                    if let Ok(rect) = Self::parse_rectangle(&fields) {
                        symbol.rectangles.push(rect);
                    }
                }
                "C" => {
                    // Circle: C~center_x~center_y~radius~stroke_color~stroke_width~stroke_style~fill_color~id~is_locked
                    log::debug!("Parsing circle with {} fields: {:?}", fields.len(), fields);
                    if let Ok(circle) = Self::parse_circle(&fields) {
                        log::debug!(
                            "Successfully parsed circle at ({}, {}), radius {}, fill {}",
                            circle.cx,
                            circle.cy,
                            circle.radius,
                            circle.fill
                        );
                        symbol.circles.push(circle);
                    } else {
                        log::warn!("Failed to parse circle from: {}", shape);
                    }
                }
                "E" => {
                    // Ellipse: E~center_x~center_y~radius_x~radius_y~stroke_color~stroke_width~stroke_style~fill_color~id~is_locked
                    log::debug!("Parsing ellipse with {} fields: {:?}", fields.len(), fields);
                    if let Ok(ellipse) = Self::parse_ellipse(&fields) {
                        log::debug!(
                            "Successfully parsed ellipse at ({}, {}), rx {}, ry {}, fill {}",
                            ellipse.cx,
                            ellipse.cy,
                            ellipse.rx,
                            ellipse.ry,
                            ellipse.fill
                        );
                        symbol.ellipses.push(ellipse);
                    } else {
                        log::warn!("Failed to parse ellipse from: {}", shape);
                    }
                }
                "A" => {
                    // Arc can be in two formats:
                    // 1. Traditional: A~x~y~radius~startAngle~endAngle~id~locked~layerid~type
                    // 2. SVG path: A~M x y A rx ry angle large sweep x y~~color~width~...
                    log::debug!("Parsing arc with {} fields", fields.len());

                    // Check if second field starts with "M" (SVG path format)
                    if fields.len() > 1 && fields[1].trim().starts_with("M") {
                        log::debug!("Detected SVG path arc: {}", fields[1]);
                        if let Ok(path_arcs) = Self::parse_svg_arc(&fields) {
                            symbol.arcs.extend(path_arcs);
                        } else {
                            log::warn!("Failed to parse SVG arc from: {}", shape);
                        }
                    } else {
                        // Traditional format
                        if let Ok(arc) = Self::parse_arc(&fields) {
                            symbol.arcs.push(arc);
                        } else {
                            log::warn!("Failed to parse traditional arc from: {}", shape);
                        }
                    }
                }
                "PL" => {
                    // Polyline: PL~x1 y1 x2 y2 ...~width~id~locked~layerid~type
                    if let Ok(polyline) = Self::parse_polyline(&fields) {
                        symbol.polylines.push(polyline);
                    }
                }
                "PG" => {
                    // Polygon: PG~x1 y1 x2 y2 ...~width~id~locked~layerid~type~fill
                    if let Ok(polygon) = Self::parse_polygon(&fields) {
                        symbol.polygons.push(polygon);
                    }
                }
                "PT" => {
                    // Path (SVG path): PT~M x y L x y Z~color~width~...~fill
                    log::debug!("Parsing PT path with {} fields", fields.len());
                    if let Ok(polygon) = Self::parse_pt_path(&fields) {
                        symbol.polygons.push(polygon);
                    } else {
                        log::warn!("Failed to parse PT path from: {}", shape);
                    }
                }
                "T" => {
                    // Text: T~x~y~rotation~text~id~locked~layerid~type~fontSize
                    if let Ok(text) = Self::parse_text(&fields) {
                        symbol.texts.push(text);
                    }
                }
                "PATH" => {
                    // Path: PATH~width~layer~path_data~gId~locked
                    if let Ok(path) = Self::parse_path(&fields) {
                        symbol.paths.push(path);
                    }
                }
                "LIB" if fields.len() > 3 => {
                    // Library info: LIB~x~y~package~id~locked
                    symbol.name = fields[3].to_string();
                }
                _ => {}
            }
        }

        // Set default prefix if not found
        if symbol.prefix.is_empty() {
            symbol.prefix = "U".to_string();
        }

        log::info!(
            "Parsed symbol: {} pins, {} rectangles, {} circles, {} ellipses, {} polylines",
            symbol.pins.len(),
            symbol.rectangles.len(),
            symbol.circles.len(),
            symbol.ellipses.len(),
            symbol.polylines.len()
        );

        Ok(symbol)
    }

    fn parse_pin(pin_data: &str) -> Result<EePin> {
        // Pin data contains multiple segments separated by ^^
        // Segment 0: P~is_displayed~type~spice_pin_number~pos_x~pos_y~rotation~id~is_locked
        // Segment 1: dot_x~dot_y
        // Segment 2: path~color
        // Segment 3: is_displayed~pos_x~pos_y~rotation~text~text_anchor~font~font_size
        // Segment 4: number fields
        // Segment 5: dot_bis (is_displayed~circle_x~circle_y)
        // Segment 6: clock (is_displayed~path)

        let segments: Vec<&str> = pin_data.split("^^").collect();
        if segments.is_empty() {
            return Err(EasyedaError::InvalidData("Empty pin data".to_string()).into());
        }

        // Parse first segment (pin settings)
        let fields: Vec<&str> = segments[0].split('~').collect();
        if fields.len() < 7 {
            return Err(EasyedaError::InvalidData(format!(
                "Invalid pin data, only {} fields in segment 0",
                fields.len()
            ))
            .into());
        }

        // Field indices: P~is_displayed~type~spice_pin_number~pos_x~pos_y~rotation~id~is_locked
        let x = fields[4]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid pin X coordinate".to_string()))?;
        let y = fields[5]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid pin Y coordinate".to_string()))?;
        let rotation = fields[6].parse::<i32>().unwrap_or(0);

        // Extract electric type from field 2
        let electric_type = fields[2].to_string();

        // Extract pin name from segment 3 if available
        let name = if segments.len() > 3 {
            let name_fields: Vec<&str> = segments[3].split('~').collect();
            if name_fields.len() > 4 {
                name_fields[4].to_string() // text field
            } else {
                "PIN".to_string()
            }
        } else {
            "PIN".to_string()
        };

        // Extract pin number from segment 4 if available
        let number = if segments.len() > 4 {
            let num_fields: Vec<&str> = segments[4].split('~').collect();
            if num_fields.len() > 4 {
                num_fields[4].to_string()
            } else {
                fields[3].to_string()
            }
        } else {
            fields[3].to_string()
        };

        // Extract pin length from segment 2 (path) if available
        let length = if segments.len() > 2 {
            let path_fields: Vec<&str> = segments[2].split('~').collect();
            if !path_fields.is_empty() {
                // Path format is like "M 0,0 h -10" or "M 0,0 v 10"
                // Extract the number after 'h' (horizontal) or 'v' (vertical)
                let path = path_fields[0];

                // Try 'h' first (horizontal)
                if let Some(h_pos) = path.rfind('h') {
                    let num_str = &path[h_pos + 1..].trim();
                    let parsed_length = num_str.parse::<f64>().unwrap_or(100.0).abs();
                    log::debug!(
                        "Pin {} ({}): path='{}', extracted length={}",
                        number,
                        name,
                        path,
                        parsed_length
                    );
                    parsed_length
                }
                // Try 'v' (vertical)
                else if let Some(v_pos) = path.rfind('v') {
                    let num_str = &path[v_pos + 1..].trim();
                    let parsed_length = num_str.parse::<f64>().unwrap_or(100.0).abs();
                    log::debug!(
                        "Pin {} ({}): path='{}', extracted length={}",
                        number,
                        name,
                        path,
                        parsed_length
                    );
                    parsed_length
                } else {
                    log::debug!(
                        "Pin {} ({}): path='{}' has no 'h' or 'v', using default length=100",
                        number,
                        name,
                        path
                    );
                    100.0
                }
            } else {
                100.0
            }
        } else {
            100.0
        };

        Ok(EePin {
            number,
            name,
            x,
            y,
            rotation,
            length,
            name_visible: true,
            number_visible: true,
            electric_type,
            dot: false,
            clock: false,
        })
    }

    fn parse_rectangle(fields: &[&str]) -> Result<EeRectangle> {
        if fields.len() < 7 {
            return Err(EasyedaError::InvalidData("Invalid rectangle data".to_string()).into());
        }

        log::debug!(
            "Parsing rectangle with {} fields: {:?}",
            fields.len(),
            fields
        );

        // R~pos_x~pos_y~rx~ry~width~height~stroke_color~stroke_width~stroke_style~fill_color~id~locked
        let x = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle X".to_string()))?;
        let y = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle Y".to_string()))?;
        // fields[3] and fields[4] are rx, ry (corner radius) - skip them
        let width = fields[5]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle width".to_string()))?;
        let height = fields[6]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle height".to_string()))?;

        // fill_color is at index 10
        let fill = fields.len() > 10 && !fields[10].is_empty() && fields[10] != "none";

        log::debug!(
            "Rectangle fill_color field[10] = '{}', fill = {}",
            if fields.len() > 10 { fields[10] } else { "N/A" },
            fill
        );

        Ok(EeRectangle {
            x,
            y,
            width,
            height,
            stroke_width: 1.0,
            fill,
            layer_id: 0,
        })
    }

    fn parse_circle(fields: &[&str]) -> Result<EeCircle> {
        if fields.len() < 4 {
            return Err(EasyedaError::InvalidData("Invalid circle data".to_string()).into());
        }

        // C~center_x~center_y~radius~stroke_color~stroke_width~stroke_style~fill_color~id~is_locked
        let cx = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid circle CX".to_string()))?;
        let cy = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid circle CY".to_string()))?;
        let radius = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid circle radius".to_string()))?;

        // fill_color is at index 7, check if it's not empty and not "none"
        let fill = fields.len() > 7 && !fields[7].is_empty() && fields[7] != "none";

        log::debug!(
            "Circle fill_color field[7] = '{}', fill = {}",
            if fields.len() > 7 { fields[7] } else { "N/A" },
            fill
        );

        Ok(EeCircle {
            cx,
            cy,
            radius,
            stroke_width: 1.0,
            fill,
            layer_id: 0,
        })
    }

    fn parse_ellipse(fields: &[&str]) -> Result<EeEllipse> {
        if fields.len() < 5 {
            return Err(EasyedaError::InvalidData("Invalid ellipse data".to_string()).into());
        }

        // E~center_x~center_y~radius_x~radius_y~stroke_color~stroke_width~stroke_style~fill_color~id~is_locked
        let cx = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid ellipse CX".to_string()))?;
        let cy = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid ellipse CY".to_string()))?;
        let rx = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid ellipse RX".to_string()))?;
        let ry = fields[4]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid ellipse RY".to_string()))?;

        // fill_color is at index 8, check if it's not empty and not "none"
        let fill = fields.len() > 8 && !fields[8].is_empty() && fields[8] != "none";

        log::debug!(
            "Ellipse fill_color field[8] = '{}', fill = {}",
            if fields.len() > 8 { fields[8] } else { "N/A" },
            fill
        );

        Ok(EeEllipse {
            cx,
            cy,
            rx,
            ry,
            stroke_width: 1.0,
            fill,
        })
    }

    fn parse_arc(fields: &[&str]) -> Result<EeArc> {
        if fields.len() < 6 {
            return Err(EasyedaError::InvalidData("Invalid arc data".to_string()).into());
        }

        let x = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid arc X".to_string()))?;
        let y = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid arc Y".to_string()))?;
        let radius = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid arc radius".to_string()))?;
        let start_angle = fields[4]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid arc start angle".to_string()))?;
        let end_angle = fields[5]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid arc end angle".to_string()))?;

        Ok(EeArc {
            x,
            y,
            radius,
            start_angle,
            end_angle,
            stroke_width: 1.0,
        })
    }

    fn parse_svg_arc(fields: &[&str]) -> Result<Vec<EeArc>> {
        use crate::cli::KicadVersion;
        use crate::easyeda::svg_parser::{SvgCommand, parse_svg_path};
        use crate::kicad::converter::Converter;

        if fields.len() < 2 {
            return Err(EasyedaError::InvalidData("Invalid SVG arc data".to_string()).into());
        }

        let svg_path = fields[1];
        let commands = parse_svg_path(svg_path)?;
        let conv = Converter::new(KicadVersion::V6);

        let mut arcs = Vec::new();
        let mut current_pos = (0.0, 0.0);

        for cmd in commands {
            match cmd {
                SvgCommand::MoveTo { x, y } => {
                    current_pos = (x, y);
                }
                SvgCommand::Arc {
                    rx,
                    ry,
                    angle,
                    large_arc,
                    sweep,
                    x,
                    y,
                } => {
                    if let Ok((cx, cy, start_angle, end_angle)) = conv.compute_arc_center(
                        current_pos,
                        (x, y),
                        (rx, ry),
                        angle,
                        large_arc,
                        sweep,
                    ) {
                        let radius = (rx + ry) / 2.0;
                        arcs.push(EeArc {
                            x: cx,
                            y: cy,
                            radius,
                            start_angle,
                            end_angle,
                            stroke_width: 1.0,
                        });
                    }

                    current_pos = (x, y);
                }
                SvgCommand::LineTo { x, y } => {
                    current_pos = (x, y);
                }
                SvgCommand::ClosePath => {}
            }
        }

        Ok(arcs)
    }

    fn parse_polyline(fields: &[&str]) -> Result<EePolyline> {
        if fields.len() < 2 {
            return Err(EasyedaError::InvalidData("Invalid polyline data".to_string()).into());
        }

        let points_str = fields[1];
        let points = Self::parse_points(points_str)?;

        // PL~points~stroke_color~stroke_width~stroke_style~fill_color~id~locked
        let stroke_width = if fields.len() > 3 {
            fields[3].parse::<f64>().unwrap_or(1.0)
        } else {
            1.0
        };

        Ok(EePolyline {
            points,
            stroke_width,
        })
    }

    fn parse_polygon(fields: &[&str]) -> Result<EePolygon> {
        if fields.len() < 2 {
            return Err(EasyedaError::InvalidData("Invalid polygon data".to_string()).into());
        }

        let points_str = fields[1];
        let points = Self::parse_points(points_str)?;

        let stroke_width = if fields.len() > 3 {
            fields[3].parse::<f64>().unwrap_or(1.0)
        } else {
            1.0
        };

        // PG~points~stroke_color~stroke_width~stroke_style~fill_color~id~locked
        // fill_color is at index 5; filled if not empty and not "none"
        let fill = fields.len() > 5 && !fields[5].is_empty() && fields[5] != "none";

        Ok(EePolygon {
            points,
            stroke_width,
            fill,
        })
    }

    fn parse_pt_path(fields: &[&str]) -> Result<EePolygon> {
        use crate::easyeda::svg_parser::{SvgCommand, parse_svg_path};

        if fields.len() < 2 {
            return Err(EasyedaError::InvalidData("Invalid path data".to_string()).into());
        }

        let svg_path = fields[1];
        let commands = parse_svg_path(svg_path)?;

        let mut points = Vec::new();
        let mut has_close_path = false;

        for cmd in commands {
            match cmd {
                SvgCommand::MoveTo { x, y } => {
                    points.push((x, y));
                }
                SvgCommand::LineTo { x, y } => {
                    points.push((x, y));
                }
                SvgCommand::Arc { x, y, .. } => {
                    // For paths, just add the end point
                    points.push((x, y));
                }
                SvgCommand::ClosePath => {
                    has_close_path = true;
                }
            }
        }

        // If path has ClosePath command (Z), close the polygon by adding the first point again
        if has_close_path && !points.is_empty() {
            let first_point = points[0];
            points.push(first_point);
        }

        let stroke_width = if fields.len() > 3 {
            fields[3].parse::<f64>().unwrap_or(1.0)
        } else {
            1.0
        };

        // If path has ClosePath command (Z), it should be filled
        // This is typical for shapes like triangles in diode symbols
        let fill = has_close_path;

        Ok(EePolygon {
            points,
            stroke_width,
            fill,
        })
    }

    fn parse_text(fields: &[&str]) -> Result<EeText> {
        // T~type~x~y~rotation~color~~fontSize~ ~ ~ ~comment~TEXT~...
        if fields.len() < 13 {
            return Err(EasyedaError::InvalidData("Invalid text data".to_string()).into());
        }

        let x = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid text X".to_string()))?;
        let y = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid text Y".to_string()))?;
        let rotation = fields[4].parse::<i32>().unwrap_or(0);
        let text = fields[12].to_string();

        let font_size = if fields.len() > 7 {
            fields[7].parse::<f64>().unwrap_or(12.0)
        } else {
            12.0
        };

        Ok(EeText {
            text,
            x,
            y,
            rotation,
            font_size,
            stroke_width: 0.0,
            layer_id: 0,
        })
    }

    fn parse_path(fields: &[&str]) -> Result<EePath> {
        if fields.len() < 4 {
            return Err(EasyedaError::InvalidData("Invalid path data".to_string()).into());
        }

        // PATH~width~layer~path_data~gId~locked
        let stroke_width = fields[1].parse::<f64>().unwrap_or(1.0);
        let path_data = fields[3].to_string();

        // Check if path is filled (usually paths are not filled in symbols)
        let fill = false;

        Ok(EePath {
            path_data,
            stroke_width,
            fill,
        })
    }

    fn parse_points(points_str: &str) -> Result<Vec<(f64, f64)>> {
        let coords: Vec<&str> = points_str.split_whitespace().collect();
        let mut points = Vec::new();

        for i in (0..coords.len()).step_by(2) {
            if i + 1 < coords.len() {
                let x = coords[i]
                    .parse::<f64>()
                    .map_err(|_| EasyedaError::InvalidData("Invalid point X".to_string()))?;
                let y = coords[i + 1]
                    .parse::<f64>()
                    .map_err(|_| EasyedaError::InvalidData("Invalid point Y".to_string()))?;
                points.push((x, y));
            }
        }

        Ok(points)
    }
}

pub struct FootprintImporter;

impl FootprintImporter {
    pub fn parse(shape_data: &[String]) -> Result<EeFootprint> {
        let mut footprint = EeFootprint {
            name: String::new(),
            pads: Vec::new(),
            tracks: Vec::new(),
            circles: Vec::new(),
            arcs: Vec::new(),
            rectangles: Vec::new(),
            texts: Vec::new(),
            holes: Vec::new(),
            vias: Vec::new(),
            svg_nodes: Vec::new(),
        };

        for shape in shape_data {
            let fields: Vec<&str> = shape.split('~').collect();
            if fields.is_empty() {
                continue;
            }

            let designator = fields[0];

            match designator {
                "PAD" => {
                    if let Ok(pad) = Self::parse_pad(&fields) {
                        footprint.pads.push(pad);
                    }
                }
                "TRACK" => {
                    if let Ok(track) = Self::parse_track(&fields) {
                        footprint.tracks.push(track);
                    }
                }
                "CIRCLE" => {
                    if let Ok(circle) = Self::parse_circle(&fields) {
                        footprint.circles.push(circle);
                    }
                }
                "ARC" => {
                    if let Ok(arc) = Self::parse_arc(&fields) {
                        footprint.arcs.push(arc);
                    }
                }
                "RECT" => {
                    if let Ok(rect) = Self::parse_rectangle(&fields) {
                        footprint.rectangles.push(rect);
                    }
                }
                "TEXT" => {
                    if let Ok(text) = Self::parse_text(&fields) {
                        footprint.texts.push(text);
                    }
                }
                "HOLE" => {
                    if let Ok(hole) = Self::parse_hole(&fields) {
                        footprint.holes.push(hole);
                    }
                }
                "VIA" => {
                    if let Ok(via) = Self::parse_via(&fields) {
                        footprint.vias.push(via);
                    }
                }
                "SVGNODE" => {
                    if let Ok(svg_node) = Self::parse_svg_node(&fields) {
                        footprint.svg_nodes.push(svg_node);
                    }
                }
                _ => {}
            }
        }

        Ok(footprint)
    }

    fn parse_pad(fields: &[&str]) -> Result<EePad> {
        if fields.len() < 9 {
            return Err(EasyedaError::InvalidData("Invalid pad data".to_string()).into());
        }

        // PAD~shape~center_x~center_y~width~height~layer_id~net~number~hole_radius~points~rotation~...
        // Actual field mapping based on real data:
        // [0]=PAD, [1]=shape, [2]=x, [3]=y, [4]=width, [5]=height, [6]=layer_id, [7]=net, [8]=number, [9]=hole_radius, [10]=points, [11]=rotation
        let shape = fields[1].to_string();
        let x = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid pad X".to_string()))?;
        let y = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid pad Y".to_string()))?;
        let width = fields[4]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid pad width".to_string()))?;
        let height = fields[5]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid pad height".to_string()))?;
        let layer_id = fields[6]
            .parse::<i32>()
            .map_err(|_| EasyedaError::InvalidData("Invalid pad layer_id".to_string()))?;

        // Field 8 is the pad number
        let number = if fields.len() > 8 {
            fields[8].to_string()
        } else {
            String::new()
        };

        // Field 9 is hole_radius
        let hole_radius = if fields.len() > 9 {
            let val = fields[9].parse::<f64>().unwrap_or(0.0);
            if val > 0.0 { Some(val) } else { None }
        } else {
            None
        };

        // Field 10 is points (for polygon pads)
        let points = if fields.len() > 10 {
            fields[10].to_string()
        } else {
            String::new()
        };

        // Field 11 is rotation
        let rotation = if fields.len() > 11 {
            fields[11].parse::<f64>().unwrap_or(0.0)
        } else {
            0.0
        };

        // Field 13 is hole_length (for elliptical drills) - field 12 is id
        let hole_length = if fields.len() > 13 {
            let val = fields[13].parse::<f64>().unwrap_or(0.0);
            if val > 0.0 { Some(val) } else { None }
        } else {
            None
        };

        Ok(EePad {
            number,
            shape,
            x,
            y,
            width,
            height,
            rotation,
            hole_radius,
            hole_length,
            points,
            layer_id,
        })
    }

    fn parse_track(fields: &[&str]) -> Result<EeTrack> {
        if fields.len() < 5 {
            return Err(EasyedaError::InvalidData("Invalid track data".to_string()).into());
        }

        // TRACK~stroke_width~layer_id~net~points~id~locked
        let stroke_width = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid track width".to_string()))?;
        let layer_id = fields[2]
            .parse::<i32>()
            .map_err(|_| EasyedaError::InvalidData("Invalid track layer_id".to_string()))?;
        let net = fields[3].to_string();
        let points = fields[4].to_string();

        Ok(EeTrack {
            stroke_width,
            layer_id,
            net,
            points,
        })
    }

    fn parse_circle(fields: &[&str]) -> Result<EeCircle> {
        if fields.len() < 5 {
            return Err(EasyedaError::InvalidData("Invalid circle data".to_string()).into());
        }

        // CIRCLE~cx~cy~radius~stroke_width~layer_id~id~is_locked
        let cx = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid circle CX".to_string()))?;
        let cy = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid circle CY".to_string()))?;
        let radius = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid circle radius".to_string()))?;
        let stroke_width = if fields.len() > 4 {
            fields[4].parse::<f64>().unwrap_or(1.0)
        } else {
            1.0
        };
        let layer_id = if fields.len() > 5 {
            fields[5].parse::<i32>().unwrap_or(3)
        } else {
            3
        };

        Ok(EeCircle {
            cx,
            cy,
            radius,
            stroke_width,
            fill: false,
            layer_id,
        })
    }

    fn parse_arc(fields: &[&str]) -> Result<EeFootprintArc> {
        if fields.len() < 5 {
            return Err(EasyedaError::InvalidData("Invalid arc data".to_string()).into());
        }

        // ARC~stroke_width~layer_id~net~path~helper_dots~id~is_locked
        let stroke_width = fields[1].parse::<f64>().unwrap_or(1.0);
        let layer_id = fields[2].parse::<i32>().unwrap_or(3);
        let path = fields[4].to_string();

        Ok(EeFootprintArc {
            stroke_width,
            layer_id,
            path,
        })
    }

    fn parse_rectangle(fields: &[&str]) -> Result<EeRectangle> {
        if fields.len() < 6 {
            return Err(EasyedaError::InvalidData("Invalid rectangle data".to_string()).into());
        }

        // RECT~x~y~width~height~stroke_width~id~layer_id~is_locked
        let x = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle X".to_string()))?;
        let y = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle Y".to_string()))?;
        let width = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle width".to_string()))?;
        let height = fields[4]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid rectangle height".to_string()))?;
        let stroke_width = if fields.len() > 5 {
            fields[5].parse::<f64>().unwrap_or(1.0)
        } else {
            1.0
        };
        let layer_id = if fields.len() > 7 {
            fields[7].parse::<i32>().unwrap_or(3)
        } else {
            3
        };

        Ok(EeRectangle {
            x,
            y,
            width,
            height,
            stroke_width,
            fill: false,
            layer_id,
        })
    }

    fn parse_text(fields: &[&str]) -> Result<EeText> {
        if fields.len() < 11 {
            return Err(EasyedaError::InvalidData("Invalid text data".to_string()).into());
        }

        // TEXT~type~center_x~center_y~stroke_width~rotation~mirror~layer_id~net~font_size~text~...
        let x = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid text X".to_string()))?;
        let y = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid text Y".to_string()))?;
        let stroke_width = fields[4].parse::<f64>().unwrap_or(0.0);
        let rotation = fields[5].parse::<i32>().unwrap_or(0);
        let layer_id = fields[7].parse::<i32>().unwrap_or(3);
        let font_size = fields[9].parse::<f64>().unwrap_or(12.0);
        let text = fields[10].to_string();

        Ok(EeText {
            text,
            x,
            y,
            rotation,
            font_size,
            stroke_width,
            layer_id,
        })
    }

    fn parse_hole(fields: &[&str]) -> Result<EeHole> {
        if fields.len() < 4 {
            return Err(EasyedaError::InvalidData("Invalid hole data".to_string()).into());
        }

        let x = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid hole X".to_string()))?;
        let y = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid hole Y".to_string()))?;
        let radius = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid hole radius".to_string()))?;

        Ok(EeHole { x, y, radius })
    }

    fn parse_via(fields: &[&str]) -> Result<EeVia> {
        if fields.len() < 6 {
            return Err(EasyedaError::InvalidData("Invalid via data".to_string()).into());
        }

        // VIA~x~y~diameter~net~radius~id~locked
        let x = fields[1]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid via X".to_string()))?;
        let y = fields[2]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid via Y".to_string()))?;
        let diameter = fields[3]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid via diameter".to_string()))?;
        let net = fields[4].to_string();
        let radius = fields[5]
            .parse::<f64>()
            .map_err(|_| EasyedaError::InvalidData("Invalid via radius".to_string()))?;

        Ok(EeVia {
            x,
            y,
            diameter,
            net,
            radius,
        })
    }

    fn parse_svg_node(fields: &[&str]) -> Result<EeSvgNode> {
        if fields.len() < 3 {
            return Err(EasyedaError::InvalidData("Invalid SVG node data".to_string()).into());
        }

        let path = fields[2].to_string();
        let layer = fields[1].to_string();

        Ok(EeSvgNode {
            path,
            stroke_width: 1.0,
            layer,
        })
    }
}
