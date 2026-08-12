use crate::error::Result;
use crate::kicad::converter::Converter;
use crate::kicad::symbol::*;

#[derive(Debug, Default)]
pub struct SymbolExporter {
    converter: Converter,
}

fn has_flat_symbol_content(symbol: &KiSymbol) -> bool {
    !symbol.pins.is_empty()
        || !symbol.rectangles.is_empty()
        || !symbol.circles.is_empty()
        || !symbol.arcs.is_empty()
        || !symbol.polylines.is_empty()
        || !symbol.texts.is_empty()
}

impl SymbolExporter {
    pub fn new() -> Self {
        Self {
            converter: Converter::new(),
        }
    }

    pub fn export(&self, symbol: &KiSymbol) -> Result<String> {
        self.export_v6(symbol)
    }

    fn export_v6(&self, symbol: &KiSymbol) -> Result<String> {
        let mut output = String::new();

        // Calculate y_high and y_low from pin positions
        let (y_high, y_low) = self.calculate_y_bounds(symbol);

        // Start symbol definition - match Python formatting
        output.push_str(&format!("  (symbol \"{}\"\n", symbol.name));
        output.push_str("    (in_bom yes)\n");
        output.push_str("    (on_board yes)\n");

        // Properties with proper formatting
        const FIELD_OFFSET_START: f64 = 5.08;
        const FIELD_OFFSET_INCREMENT: f64 = 2.54;
        let mut field_offset_y = FIELD_OFFSET_START;
        let mut property_id = 0;

        // Reference property
        output.push_str("    (property\n");
        output.push_str("      \"Reference\"\n");
        output.push_str(&format!("      \"{}\"\n", symbol.reference));
        output.push_str(&format!("      (id {})\n", property_id));
        output.push_str(&format!("      (at 0 {:.2} 0)\n", y_high + field_offset_y));
        output.push_str("      (effects (font (size 1.27 1.27) ) )\n");
        output.push_str("    )\n");
        property_id += 1;

        // Value property
        output.push_str("    (property\n");
        output.push_str("      \"Value\"\n");
        output.push_str(&format!("      \"{}\"\n", symbol.value));
        output.push_str(&format!("      (id {})\n", property_id));
        output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
        output.push_str("      (effects (font (size 1.27 1.27) ) )\n");
        output.push_str("    )\n");
        property_id += 1;

        // Footprint property
        if !symbol.footprint.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"Footprint\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.footprint));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
            property_id += 1;
        }

        // Datasheet property (always emit)
        field_offset_y += FIELD_OFFSET_INCREMENT;
        output.push_str("    (property\n");
        output.push_str("      \"Datasheet\"\n");
        output.push_str(&format!("      \"{}\"\n", symbol.datasheet));
        output.push_str(&format!("      (id {})\n", property_id));
        output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
        output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
        output.push_str("    )\n");
        property_id += 1;

        // Description property (always emit)
        field_offset_y += FIELD_OFFSET_INCREMENT;
        output.push_str("    (property\n");
        output.push_str("      \"Description\"\n");
        output.push_str(&format!("      \"{}\"\n", symbol.description));
        output.push_str(&format!("      (id {})\n", property_id));
        output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
        output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
        output.push_str("    )\n");
        property_id += 1;

        // Manufacturer property
        if !symbol.manufacturer.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"Manufacturer\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.manufacturer));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
            property_id += 1;
        }

        // LCSC Part property
        if !symbol.lcsc_id.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"LCSC Part\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.lcsc_id));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
            property_id += 1;
        }

        // JLC Part property
        if !symbol.jlc_id.is_empty() {
            field_offset_y += FIELD_OFFSET_INCREMENT;
            output.push_str("    (property\n");
            output.push_str("      \"JLC Part\"\n");
            output.push_str(&format!("      \"{}\"\n", symbol.jlc_id));
            output.push_str(&format!("      (id {})\n", property_id));
            output.push_str(&format!("      (at 0 {:.2} 0)\n", y_low - field_offset_y));
            output.push_str("      (effects (font (size 1.27 1.27) ) hide)\n");
            output.push_str("    )\n");
        }

        if symbol.units.is_empty() {
            self.write_flat_unit_v6(&mut output, symbol, 0);
        } else {
            if has_flat_symbol_content(symbol) {
                self.write_flat_unit_v6(&mut output, symbol, 0);
            }

            for (index, unit) in symbol.units.iter().enumerate() {
                self.write_unit_v6(&mut output, &symbol.name, index + 1, unit);
            }
        }

        output.push_str("  )\n");

        Ok(output)
    }

    fn write_flat_unit_v6(&self, output: &mut String, symbol: &KiSymbol, unit_index: usize) {
        output.push_str(&format!(
            "    (symbol \"{}_{}_1\"\n",
            symbol.name, unit_index
        ));

        for rect in &symbol.rectangles {
            output.push_str(&self.format_rectangle_v6(rect));
        }

        for circle in &symbol.circles {
            output.push_str(&self.format_circle_v6(circle));
        }

        for arc in &symbol.arcs {
            output.push_str(&self.format_arc_v6(arc));
        }

        for polyline in &symbol.polylines {
            output.push_str(&self.format_polyline_v6(polyline));
        }

        for text in &symbol.texts {
            output.push_str(&self.format_text_v6(text));
        }

        for pin in &symbol.pins {
            output.push_str(&self.format_pin_v6(pin));
        }

        output.push_str("    )\n");
    }

    fn write_unit_v6(
        &self,
        output: &mut String,
        symbol_name: &str,
        unit_index: usize,
        unit: &KiSymbolUnit,
    ) {
        output.push_str(&format!(
            "    (symbol \"{}_{}_1\"\n",
            symbol_name, unit_index
        ));

        for rect in &unit.rectangles {
            output.push_str(&self.format_rectangle_v6(rect));
        }

        for circle in &unit.circles {
            output.push_str(&self.format_circle_v6(circle));
        }

        for arc in &unit.arcs {
            output.push_str(&self.format_arc_v6(arc));
        }

        for polyline in &unit.polylines {
            output.push_str(&self.format_polyline_v6(polyline));
        }

        for text in &unit.texts {
            output.push_str(&self.format_text_v6(text));
        }

        for pin in &unit.pins {
            output.push_str(&self.format_pin_v6(pin));
        }

        output.push_str("    )\n");
    }

    fn calculate_y_bounds(&self, symbol: &KiSymbol) -> (f64, f64) {
        if symbol.pins.is_empty() && symbol.units.iter().all(|unit| unit.pins.is_empty()) {
            return (0.0, 0.0);
        }

        let mut y_high = f64::MIN;
        let mut y_low = f64::MAX;

        for pin in symbol
            .pins
            .iter()
            .chain(symbol.units.iter().flat_map(|unit| unit.pins.iter()))
        {
            let y = self.converter.px_to_mm(pin.pos_y);
            if y > y_high {
                y_high = y;
            }
            if y < y_low {
                y_low = y;
            }
        }

        (y_high, y_low)
    }

    #[allow(dead_code)]
    fn export_v5(&self, symbol: &KiSymbol) -> Result<String> {
        let mut output = String::new();
        let geometry = self.geometry_for_v5(symbol);

        // DEF name reference unused text_offset draw_pinnumber draw_pinname unit_count units_locked option_flag
        output.push_str(&format!(
            "DEF {} {} 0 40 Y Y 1 F N\n",
            symbol.name, symbol.reference
        ));

        // F0 reference x y size orientation visibility hjustify vjustify/italic/bold
        output.push_str(&format!("F0 \"{}\" 0 0 50 H V C CNN\n", symbol.reference));
        output.push_str(&format!("F1 \"{}\" 0 -100 50 H V C CNN\n", symbol.value));
        output.push_str(&format!("F2 \"{}\" 0 0 50 H I C CNN\n", symbol.footprint));
        output.push_str(&format!("F3 \"{}\" 0 0 50 H I C CNN\n", symbol.datasheet));

        // DRAW
        output.push_str("DRAW\n");

        // Rectangles
        for rect in &geometry.rectangles {
            output.push_str(&self.format_rectangle_v5(rect));
        }

        // Circles
        for circle in &geometry.circles {
            output.push_str(&self.format_circle_v5(circle));
        }

        // Polylines
        for polyline in &geometry.polylines {
            output.push_str(&self.format_polyline_v5(polyline));
        }

        // Pins
        for pin in &geometry.pins {
            output.push_str(&self.format_pin_v5(pin));
        }

        output.push_str("ENDDRAW\n");
        output.push_str("ENDDEF\n");

        Ok(output)
    }

    fn geometry_for_v5(&self, symbol: &KiSymbol) -> KiSymbolUnit {
        if symbol.units.is_empty() {
            return KiSymbolUnit {
                pins: symbol.pins.clone(),
                rectangles: symbol.rectangles.clone(),
                circles: symbol.circles.clone(),
                arcs: symbol.arcs.clone(),
                polylines: symbol.polylines.clone(),
                texts: symbol.texts.clone(),
            };
        }

        let mut geometry = KiSymbolUnit::default();
        for unit in &symbol.units {
            geometry.pins.extend(unit.pins.clone());
            geometry.rectangles.extend(unit.rectangles.clone());
            geometry.circles.extend(unit.circles.clone());
            geometry.arcs.extend(unit.arcs.clone());
            geometry.polylines.extend(unit.polylines.clone());
            geometry.texts.extend(unit.texts.clone());
        }

        geometry
    }

    fn format_pin_v6(&self, pin: &KiPin) -> String {
        let x = self.converter.px_to_mm(pin.pos_x);
        let y = self.converter.px_to_mm(pin.pos_y);
        let length = self.converter.px_to_mm(pin.length);

        // Convert pin rotation: (180 + orientation) % 360
        let orientation = (180 + pin.rotation) % 360;

        format!(
            "      (pin {} {}\n        (at {:.2} {:.2} {})\n        (length {:.2})\n        (name \"{}\" (effects (font (size 1.27 1.27))))\n        (number \"{}\" (effects (font (size 1.27 1.27))))\n      )\n",
            pin.pin_type.to_kicad_v6(),
            pin.style.to_kicad_v6(),
            x,
            y,
            orientation,
            length,
            pin.name,
            pin.number
        )
    }

    fn format_pin_v5(&self, pin: &KiPin) -> String {
        let x = self.converter.px_to_mil(pin.pos_x);
        let y = self.converter.px_to_mil(pin.pos_y); // Don't flip, already handled
        let length = self.converter.px_to_mil(pin.length);

        // X name number posx posy length orientation Snum Snom unit convert Etype [shape]
        format!(
            "X {} {} {} {} {} {} {} {} {} {} {}\n",
            pin.name,
            pin.number,
            x,
            y,
            length,
            self.rotation_to_direction(pin.rotation),
            50, // name size
            50, // number size
            1,  // unit
            1,  // convert
            pin.pin_type.to_kicad_v5()
        )
    }

    fn format_rectangle_v6(&self, rect: &KiRectangle) -> String {
        let x1 = self.converter.px_to_mm(rect.x1);
        let y1 = self.converter.px_to_mm(rect.y1);
        let x2 = self.converter.px_to_mm(rect.x2);
        let y2 = self.converter.px_to_mm(rect.y2);
        let _width = self.converter.px_to_mm(rect.stroke_width);

        let fill = if rect.fill {
            "(fill (type color) (color 255 192 203 1))"
        } else {
            "(fill (type none))"
        };

        format!(
            "      (rectangle\n        (start {:.2} {:.2})\n        (end {:.2} {:.2})\n        (stroke (width {}) (type default) (color 0 0 0 0))\n        {}\n      )\n",
            x1, y1, x2, y2, 0, fill
        )
    }

    fn format_rectangle_v5(&self, rect: &KiRectangle) -> String {
        let x1 = self.converter.px_to_mil(rect.x1);
        let y1 = self.converter.px_to_mil(rect.y1); // Don't flip, already handled
        let x2 = self.converter.px_to_mil(rect.x2);
        let y2 = self.converter.px_to_mil(rect.y2); // Don't flip, already handled

        let fill = if rect.fill { "F" } else { "N" };

        // S startx starty endx endy unit convert thickness fill
        format!("S {} {} {} {} 1 1 10 {}\n", x1, y1, x2, y2, fill)
    }

    fn format_circle_v6(&self, circle: &KiCircle) -> String {
        let cx = self.converter.px_to_mm(circle.cx);
        let cy = self.converter.px_to_mm(circle.cy);
        let radius = self.converter.px_to_mm(circle.radius);

        // Circles in symbols should always have fill type "none" to match Python output
        let fill = "none";

        format!(
            "      (circle\n        (center {:.2} {:.2})\n        (radius {:.2})\n        (stroke (width {}) (type default) (color 0 0 0 0))\n        (fill (type {}))\n      )\n",
            cx, cy, radius, 0, fill
        )
    }

    fn format_circle_v5(&self, circle: &KiCircle) -> String {
        let cx = self.converter.px_to_mil(circle.cx);
        let cy = self.converter.px_to_mil(circle.cy); // Don't flip, already handled
        let radius = self.converter.px_to_mil(circle.radius);

        let fill = if circle.fill { "F" } else { "N" };

        // C posx posy radius unit convert thickness fill
        format!("C {} {} {} 1 1 10 {}\n", cx, cy, radius, fill)
    }

    fn format_arc_v6(&self, arc: &KiArc) -> String {
        let start_x = self.converter.px_to_mm(arc.start_x);
        let start_y = self.converter.px_to_mm(arc.start_y); // Don't flip, already handled
        let mid_x = self.converter.px_to_mm(arc.mid_x);
        let mid_y = self.converter.px_to_mm(arc.mid_y); // Don't flip, already handled
        let end_x = self.converter.px_to_mm(arc.end_x);
        let end_y = self.converter.px_to_mm(arc.end_y); // Don't flip, already handled
        let width = self.converter.px_to_mm(arc.stroke_width);

        format!(
            "    (arc (start {:.4} {:.4}) (mid {:.4} {:.4}) (end {:.4} {:.4})\n      (stroke (width {:.4}) (type default))\n      (fill (type none))\n    )\n",
            start_x, start_y, mid_x, mid_y, end_x, end_y, width
        )
    }

    fn format_polyline_v6(&self, polyline: &KiPolyline) -> String {
        let mut output = String::from("    (polyline\n      (pts\n");

        for (x, y) in &polyline.points {
            let x = self.converter.px_to_mm(*x);
            let y = self.converter.px_to_mm(*y); // Don't flip, already handled
            output.push_str(&format!("        (xy {:.4} {:.4})\n", x, y));
        }

        let width = self.converter.px_to_mm(polyline.stroke_width);
        let fill = if polyline.fill { "outline" } else { "none" };

        output.push_str("      )\n");
        output.push_str(&format!(
            "      (stroke (width {:.4}) (type default))\n",
            width
        ));
        output.push_str(&format!("      (fill (type {}))\n", fill));
        output.push_str("    )\n");

        output
    }

    fn format_polyline_v5(&self, polyline: &KiPolyline) -> String {
        let point_count = polyline.points.len();
        let mut output = format!("P {} 1 1 10", point_count);

        for (x, y) in &polyline.points {
            let x = self.converter.px_to_mil(*x);
            let y = self.converter.px_to_mil(*y); // Don't flip, already handled
            output.push_str(&format!(" {} {}", x, y));
        }

        let fill = if polyline.fill { "F" } else { "N" };
        output.push_str(&format!(" {}\n", fill));

        output
    }

    fn rotation_to_direction(&self, rotation: i32) -> char {
        match rotation {
            0 => 'R',
            90 => 'U',
            180 => 'L',
            270 => 'D',
            _ => 'R',
        }
    }

    fn format_text_v6(&self, text: &super::symbol::KiText) -> String {
        let x = self.converter.px_to_mm(text.x);
        let y = self.converter.px_to_mm(text.y);
        let size = (text.font_size * 0.15).clamp(0.5, 1.27);
        let rotation = text.rotation;

        format!(
            "    (text \"{}\" (at {:.4} {:.4} {})\n      (effects (font (size {:.4} {:.4})))\n    )\n",
            text.text, x, y, rotation as i32, size, size
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v6_exports_each_symbol_unit() {
        let exporter = SymbolExporter::new();
        let symbol = KiSymbol {
            name: "MultiPart".to_string(),
            reference: "U".to_string(),
            value: "MultiPart".to_string(),
            description: String::new(),
            footprint: String::new(),
            datasheet: String::new(),
            manufacturer: String::new(),
            lcsc_id: String::new(),
            jlc_id: String::new(),
            units: vec![unit_with_pin("A1", "1"), unit_with_pin("B1", "2")],
            pins: Vec::new(),
            rectangles: Vec::new(),
            circles: Vec::new(),
            arcs: Vec::new(),
            polylines: Vec::new(),
            texts: Vec::new(),
        };

        let output = exporter.export(&symbol).unwrap();

        assert!(output.contains("(symbol \"MultiPart_1_1\""));
        assert!(output.contains("(symbol \"MultiPart_2_1\""));
        assert!(!output.contains("(symbol \"MultiPart_0_1\""));
        assert!(output.contains("(name \"A1\""));
        assert!(output.contains("(name \"B1\""));
        assert_eq!(output.matches("(pin ").count(), 2);
    }

    fn unit_with_pin(name: &str, number: &str) -> KiSymbolUnit {
        KiSymbolUnit {
            pins: vec![KiPin {
                number: number.to_string(),
                name: name.to_string(),
                pin_type: PinType::Passive,
                style: PinStyle::Line,
                pos_x: 0.0,
                pos_y: 0.0,
                rotation: 0,
                length: 10.0,
            }],
            rectangles: Vec::new(),
            circles: Vec::new(),
            arcs: Vec::new(),
            polylines: Vec::new(),
            texts: Vec::new(),
        }
    }
}
