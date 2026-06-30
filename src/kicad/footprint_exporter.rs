use crate::cli::KicadVersion;
use crate::error::Result;
use crate::kicad::converter::Converter;
use crate::kicad::footprint::*;

pub struct FootprintExporter {
    converter: Converter,
}

impl FootprintExporter {
    pub fn new() -> Self {
        Self {
            converter: Converter::new(KicadVersion::V6),
        }
    }

    pub fn export(&self, footprint: &KiFootprint) -> Result<String> {
        let mut output = String::new();

        // Module header
        output.push_str(&format!(
            "(footprint \"{}\" (version 20221018) (generator npnp)\n",
            footprint.name
        ));
        output.push_str("  (layer \"F.Cu\")\n");

        // Reference and value text
        output.push_str("  (fp_text reference \"REF**\" (at 0 0) (layer \"F.SilkS\")\n");
        output.push_str("    (effects (font (size 1 1) (thickness 0.15)))\n");
        output.push_str("  )\n");

        output.push_str(&format!(
            "  (fp_text value \"{}\" (at 0 2.5) (layer \"F.Fab\")\n",
            footprint.name
        ));
        output.push_str("    (effects (font (size 1 1) (thickness 0.15)))\n");
        output.push_str("  )\n");

        // Pads
        for pad in &footprint.pads {
            output.push_str(&self.format_pad(pad));
        }

        // Lines
        for line in &footprint.lines {
            output.push_str(&self.format_line(line));
        }

        // Circles
        for circle in &footprint.circles {
            output.push_str(&self.format_circle(circle));
        }

        // Arcs
        for arc in &footprint.arcs {
            output.push_str(&self.format_arc(arc));
        }

        // Texts
        for text in &footprint.texts {
            output.push_str(&self.format_text(text));
        }

        // 3D model
        if let Some(model) = &footprint.model_3d {
            output.push_str(&self.format_3d_model(model));
        }

        output.push_str(")\n");

        Ok(output)
    }

    fn format_pad(&self, pad: &KiPad) -> String {
        let x = self.converter.px_to_mm(pad.pos_x);
        let y = self.converter.px_to_mm(pad.pos_y); // No flip_y for footprints
        let size_x = self.converter.px_to_mm(pad.size_x);
        let size_y = self.converter.px_to_mm(pad.size_y);

        let mut output = format!(
            "  (pad \"{}\" {} {} (at {:.4} {:.4}",
            pad.number,
            pad.pad_type.to_kicad(),
            pad.shape.to_kicad(),
            x,
            y
        );

        if pad.rotation != 0.0 {
            output.push_str(&format!(" {:.4}", pad.rotation));
        }

        output.push_str(&format!(") (size {:.4} {:.4})", size_x, size_y));

        // Layers
        output.push_str(" (layers");
        for layer in &pad.layers {
            output.push_str(&format!(" \"{}\"", layer));
        }
        output.push(')');

        // Drill
        if let Some(drill) = &pad.drill {
            let drill_dia = self.converter.px_to_mm(drill.diameter);
            if let Some(width) = drill.width {
                // Elliptical drill
                let drill_width = self.converter.px_to_mm(width);
                output.push_str(&format!(
                    " (drill oval {:.4} {:.4})",
                    drill_dia, drill_width
                ));
            } else {
                // Circular drill
                output.push_str(&format!(" (drill {:.4})", drill_dia));
            }
        }

        // Polygon (for custom pads)
        if let Some(polygon) = &pad.polygon {
            output.push_str(polygon);
        }

        output.push_str(")\n");

        output
    }

    fn format_line(&self, line: &KiLine) -> String {
        let start_x = self.converter.px_to_mm(line.start_x);
        let start_y = self.converter.px_to_mm(line.start_y); // No flip_y for footprints
        let end_x = self.converter.px_to_mm(line.end_x);
        let end_y = self.converter.px_to_mm(line.end_y); // No flip_y for footprints
        let width = self.converter.px_to_mm(line.width);

        format!(
            "  (fp_line (start {:.4} {:.4}) (end {:.4} {:.4})\n    (stroke (width {:.4}) (type solid)) (layer \"{}\")\n  )\n",
            start_x, start_y, end_x, end_y, width, line.layer
        )
    }

    fn format_circle(&self, circle: &KiCircle) -> String {
        let center_x = self.converter.px_to_mm(circle.center_x);
        let center_y = self.converter.px_to_mm(circle.center_y); // No flip_y for footprints
        let end_x = self.converter.px_to_mm(circle.end_x);
        let end_y = self.converter.px_to_mm(circle.end_y); // No flip_y for footprints
        let width = self.converter.px_to_mm(circle.width);

        let fill = if circle.fill { "solid" } else { "none" };

        format!(
            "  (fp_circle (center {:.4} {:.4}) (end {:.4} {:.4})\n    (stroke (width {:.4}) (type solid)) (fill {}) (layer \"{}\")\n  )\n",
            center_x, center_y, end_x, end_y, width, fill, circle.layer
        )
    }

    fn format_arc(&self, arc: &KiArc) -> String {
        let start_x = self.converter.px_to_mm(arc.start_x);
        let start_y = self.converter.px_to_mm(arc.start_y); // No flip_y for footprints
        let mid_x = self.converter.px_to_mm(arc.mid_x);
        let mid_y = self.converter.px_to_mm(arc.mid_y); // No flip_y for footprints
        let end_x = self.converter.px_to_mm(arc.end_x);
        let end_y = self.converter.px_to_mm(arc.end_y); // No flip_y for footprints
        let width = self.converter.px_to_mm(arc.width);

        format!(
            "  (fp_arc (start {:.4} {:.4}) (mid {:.4} {:.4}) (end {:.4} {:.4})\n    (stroke (width {:.4}) (type solid)) (layer \"{}\")\n  )\n",
            start_x, start_y, mid_x, mid_y, end_x, end_y, width, arc.layer
        )
    }

    fn format_text(&self, text: &KiText) -> String {
        let x = self.converter.px_to_mm(text.pos_x);
        let y = self.converter.px_to_mm(text.pos_y); // No flip_y for footprints
        let size = self.converter.px_to_mm(text.size);
        let thickness = self.converter.px_to_mm(text.thickness);

        format!("  (fp_text user \"{}\" (at {:.4} {:.4}", text.text, x, y)
            + &(if text.rotation != 0.0 {
                format!(" {:.4}", text.rotation)
            } else {
                String::new()
            })
            + &format!(
                ") (layer \"{}\")\n    (effects (font (size {:.4} {:.4}) (thickness {:.4})))\n  )\n",
                text.layer, size, size, thickness
            )
    }

    fn format_3d_model(&self, model: &Ki3dModel) -> String {
        format!(
            "  (model \"{}\"\n    (offset (xyz {:.4} {:.4} {:.4}))\n    (scale (xyz {:.4} {:.4} {:.4}))\n    (rotate (xyz {:.4} {:.4} {:.4}))\n  )\n",
            model.path,
            model.offset.0,
            model.offset.1,
            model.offset.2,
            model.scale.0,
            model.scale.1,
            model.scale.2,
            model.rotate.0,
            model.rotate.1,
            model.rotate.2
        )
    }
}

impl Default for FootprintExporter {
    fn default() -> Self {
        Self::new()
    }
}
