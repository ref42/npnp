use crate::cli::KicadVersion;
use crate::error::{ConversionError, Result};

pub struct Converter {
    _kicad_version: KicadVersion,
}

impl Converter {
    pub fn new(kicad_version: KicadVersion) -> Self {
        Self {
            _kicad_version: kicad_version,
        }
    }

    /// Convert pixels to mils (1 px = 10 mils in EasyEDA)
    pub fn px_to_mil(&self, px: f64) -> i32 {
        (10.0 * px) as i32
    }

    /// Convert pixels to millimeters (1 px = 10 mils = 0.254 mm)
    pub fn px_to_mm(&self, px: f64) -> f64 {
        10.0 * px * 0.0254
    }

    /// Flip Y coordinate (EasyEDA uses top-left origin, KiCad uses bottom-left)
    pub fn flip_y(&self, y: f64) -> f64 {
        -y
    }

    /// Normalize coordinate to bounding box origin
    pub fn normalize_to_bbox(&self, coord: f64, bbox_origin: f64) -> f64 {
        coord - bbox_origin
    }

    /// Convert degrees to radians
    pub fn deg_to_rad(&self, degrees: f64) -> f64 {
        degrees * std::f64::consts::PI / 180.0
    }

    /// Convert radians to degrees
    pub fn rad_to_deg(&self, radians: f64) -> f64 {
        radians * 180.0 / std::f64::consts::PI
    }

    /// Compute arc center from SVG elliptical arc endpoint parameters
    /// Based on W3C SVG specification for arc conversion
    /// Returns (center_x, center_y, start_angle_deg, end_angle_deg)
    pub fn compute_arc_center(
        &self,
        start: (f64, f64),
        end: (f64, f64),
        radii: (f64, f64),
        x_axis_rotation: f64,
        large_arc: bool,
        sweep: bool,
    ) -> Result<(f64, f64, f64, f64)> {
        let (x1, y1) = start;
        let (x2, y2) = end;
        let (mut rx, mut ry) = radii;

        // Handle degenerate cases
        if (x1 - x2).abs() < 1e-10 && (y1 - y2).abs() < 1e-10 {
            return Err(ConversionError::ArcConversion(
                "Start and end points are identical".to_string(),
            )
            .into());
        }

        if rx.abs() < 1e-10 || ry.abs() < 1e-10 {
            return Err(ConversionError::ArcConversion("Radii are too small".to_string()).into());
        }

        // Ensure radii are positive
        rx = rx.abs();
        ry = ry.abs();

        // Convert rotation angle to radians
        let phi = self.deg_to_rad(x_axis_rotation);
        let cos_phi = phi.cos();
        let sin_phi = phi.sin();

        // Step 1: Compute (x1', y1') - transformed start point
        let dx = (x1 - x2) / 2.0;
        let dy = (y1 - y2) / 2.0;
        let x1_prime = cos_phi * dx + sin_phi * dy;
        let y1_prime = -sin_phi * dx + cos_phi * dy;

        // Step 2: Correct radii if needed
        let lambda = (x1_prime / rx).powi(2) + (y1_prime / ry).powi(2);
        if lambda > 1.0 {
            rx *= lambda.sqrt();
            ry *= lambda.sqrt();
        }

        // Step 3: Compute center point (cx', cy') in transformed space
        let sq = ((rx * ry).powi(2) - (rx * y1_prime).powi(2) - (ry * x1_prime).powi(2))
            / ((rx * y1_prime).powi(2) + (ry * x1_prime).powi(2));

        let sq = if sq < 0.0 { 0.0 } else { sq.sqrt() };

        let sign = if large_arc == sweep { -1.0 } else { 1.0 };
        let cx_prime = sign * sq * rx * y1_prime / ry;
        let cy_prime = -sign * sq * ry * x1_prime / rx;

        // Step 4: Compute center point (cx, cy) in original space
        let cx = cos_phi * cx_prime - sin_phi * cy_prime + (x1 + x2) / 2.0;
        let cy = sin_phi * cx_prime + cos_phi * cy_prime + (y1 + y2) / 2.0;

        // Step 5: Compute start and end angles
        let ux = (x1_prime - cx_prime) / rx;
        let uy = (y1_prime - cy_prime) / ry;
        let vx = (-x1_prime - cx_prime) / rx;
        let vy = (-y1_prime - cy_prime) / ry;

        // Compute start angle
        let n = (ux.powi(2) + uy.powi(2)).sqrt();
        let p = ux;
        let sign = if uy < 0.0 { -1.0 } else { 1.0 };
        let mut theta1 = sign * (p / n).acos();

        // Compute angle extent
        let n = ((ux.powi(2) + uy.powi(2)) * (vx.powi(2) + vy.powi(2))).sqrt();
        let p = ux * vx + uy * vy;
        let sign = if ux * vy - uy * vx < 0.0 { -1.0 } else { 1.0 };
        let mut dtheta = sign * (p / n).acos();

        if !sweep && dtheta > 0.0 {
            dtheta -= 2.0 * std::f64::consts::PI;
        } else if sweep && dtheta < 0.0 {
            dtheta += 2.0 * std::f64::consts::PI;
        }

        let theta2 = theta1 + dtheta;

        // Convert to degrees
        theta1 = self.rad_to_deg(theta1);
        let mut theta2 = self.rad_to_deg(theta2);

        // Normalize angles
        while theta1 < 0.0 {
            theta1 += 360.0;
        }
        while theta2 < 0.0 {
            theta2 += 360.0;
        }
        while theta1 >= 360.0 {
            theta1 -= 360.0;
        }
        while theta2 >= 360.0 {
            theta2 -= 360.0;
        }

        Ok((cx, cy, theta1, theta2))
    }

    /// Calculate bounding box for a set of points
    pub fn calculate_bbox(&self, points: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
        if points.is_empty() {
            return None;
        }

        let mut min_x = points[0].0;
        let mut max_x = points[0].0;
        let mut min_y = points[0].1;
        let mut max_y = points[0].1;

        for &(x, y) in points.iter().skip(1) {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }

        Some((min_x, min_y, max_x, max_y))
    }
}

/// Sanitize a component name for use as a filename/identifier
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Convert EasyEDA rotation to KiCad rotation
/// Negates angles > 180 degrees to match KiCad convention
pub fn angle_to_ki(rotation: f64) -> f64 {
    if rotation > 180.0 {
        -(360.0 - rotation)
    } else {
        rotation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_px_to_mil() {
        let converter = Converter::new(KicadVersion::V6);
        assert_eq!(converter.px_to_mil(10.0), 100);
        assert_eq!(converter.px_to_mil(5.5), 55);
    }

    #[test]
    fn test_px_to_mm() {
        let converter = Converter::new(KicadVersion::V6);
        let result = converter.px_to_mm(10.0);
        assert!((result - 2.54).abs() < 0.001);
    }

    #[test]
    fn test_flip_y() {
        let converter = Converter::new(KicadVersion::V6);
        assert_eq!(converter.flip_y(10.0), -10.0);
        assert_eq!(converter.flip_y(-5.0), 5.0);
    }

    #[test]
    fn test_deg_to_rad() {
        let converter = Converter::new(KicadVersion::V6);
        let result = converter.deg_to_rad(180.0);
        assert!((result - std::f64::consts::PI).abs() < 0.001);
    }
}
