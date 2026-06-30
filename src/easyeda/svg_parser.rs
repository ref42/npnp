use crate::error::{ConversionError, Result};
use regex::Regex;

#[derive(Debug, Clone)]
pub enum SvgCommand {
    MoveTo {
        x: f64,
        y: f64,
    },
    LineTo {
        x: f64,
        y: f64,
    },
    Arc {
        rx: f64,
        ry: f64,
        angle: f64,
        large_arc: bool,
        sweep: bool,
        x: f64,
        y: f64,
    },
    ClosePath,
}

pub fn parse_svg_path(path: &str) -> Result<Vec<SvgCommand>> {
    let mut commands = Vec::new();
    let path = path.trim();

    // Regex patterns for SVG commands
    let move_re = Regex::new(r"M\s*([-\d.]+)[,\s]+([-\d.]+)").unwrap();
    let line_re = Regex::new(r"L\s*([-\d.]+)[,\s]+([-\d.]+)").unwrap();
    let arc_re = Regex::new(
        r"A\s*([-\d.]+)[,\s]+([-\d.]+)[,\s]+([-\d.]+)[,\s]+([01])[,\s]+([01])[,\s]+([-\d.]+)[,\s]+([-\d.]+)"
    ).unwrap();

    let mut pos = 0;
    let chars: Vec<char> = path.chars().collect();

    while pos < chars.len() {
        let remaining = &path[pos..];

        if remaining.starts_with('M') {
            if let Some(cap) = move_re.captures(remaining) {
                let x = cap[1].parse::<f64>().map_err(|_| {
                    ConversionError::SvgParse("Invalid MoveTo X coordinate".to_string())
                })?;
                let y = cap[2].parse::<f64>().map_err(|_| {
                    ConversionError::SvgParse("Invalid MoveTo Y coordinate".to_string())
                })?;
                commands.push(SvgCommand::MoveTo { x, y });
                pos += cap.get(0).unwrap().len();
            } else {
                pos += 1;
            }
        } else if remaining.starts_with('L') {
            if let Some(cap) = line_re.captures(remaining) {
                let x = cap[1].parse::<f64>().map_err(|_| {
                    ConversionError::SvgParse("Invalid LineTo X coordinate".to_string())
                })?;
                let y = cap[2].parse::<f64>().map_err(|_| {
                    ConversionError::SvgParse("Invalid LineTo Y coordinate".to_string())
                })?;
                commands.push(SvgCommand::LineTo { x, y });
                pos += cap.get(0).unwrap().len();
            } else {
                pos += 1;
            }
        } else if remaining.starts_with('A') {
            if let Some(cap) = arc_re.captures(remaining) {
                let rx = cap[1]
                    .parse::<f64>()
                    .map_err(|_| ConversionError::SvgParse("Invalid Arc RX".to_string()))?;
                let ry = cap[2]
                    .parse::<f64>()
                    .map_err(|_| ConversionError::SvgParse("Invalid Arc RY".to_string()))?;
                let angle = cap[3]
                    .parse::<f64>()
                    .map_err(|_| ConversionError::SvgParse("Invalid Arc angle".to_string()))?;
                let large_arc = &cap[4] == "1";
                let sweep = &cap[5] == "1";
                let x = cap[6].parse::<f64>().map_err(|_| {
                    ConversionError::SvgParse("Invalid Arc X coordinate".to_string())
                })?;
                let y = cap[7].parse::<f64>().map_err(|_| {
                    ConversionError::SvgParse("Invalid Arc Y coordinate".to_string())
                })?;
                commands.push(SvgCommand::Arc {
                    rx,
                    ry,
                    angle,
                    large_arc,
                    sweep,
                    x,
                    y,
                });
                pos += cap.get(0).unwrap().len();
            } else {
                pos += 1;
            }
        } else if remaining.starts_with('Z') || remaining.starts_with('z') {
            commands.push(SvgCommand::ClosePath);
            pos += 1;
        } else {
            pos += 1;
        }
    }

    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_path() {
        let path = "M 10,20 L 30,40 Z";
        let commands = parse_svg_path(path).unwrap();
        assert_eq!(commands.len(), 3);
    }

    #[test]
    fn test_parse_arc() {
        let path = "M 0,0 A 10,10 0 0 1 20,20";
        let commands = parse_svg_path(path).unwrap();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn test_parse_footprint_arc_comma_no_space() {
        let path = "M3956.6958,3000.9151 A29.0043,29.0043 0 1 0 3898.9082,2999.9286";
        let commands = parse_svg_path(path).unwrap();
        assert_eq!(commands.len(), 2);
    }
}
