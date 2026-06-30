#[derive(Debug, Clone)]
pub struct KiFootprint {
    pub name: String,
    pub pads: Vec<KiPad>,
    pub tracks: Vec<KiTrack>,
    pub circles: Vec<KiCircle>,
    pub arcs: Vec<KiArc>,
    pub texts: Vec<KiText>,
    pub lines: Vec<KiLine>,
    pub model_3d: Option<Ki3dModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadType {
    Smd,
    ThroughHole,
    NpThroughHole,
    Connect,
}

impl PadType {
    pub fn to_kicad(&self) -> &'static str {
        match self {
            PadType::Smd => "smd",
            PadType::ThroughHole => "thru_hole",
            PadType::NpThroughHole => "np_thru_hole",
            PadType::Connect => "connect",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadShape {
    Circle,
    Rect,
    Oval,
    Trapezoid,
    RoundRect,
    Custom,
}

impl PadShape {
    pub fn from_easyeda(shape: &str) -> Self {
        match shape {
            "ELLIPSE" | "ROUND" => PadShape::Circle,
            "RECT" => PadShape::Rect,
            "OVAL" => PadShape::Oval,
            "POLYGON" => PadShape::Custom,
            _ => PadShape::Rect,
        }
    }

    pub fn to_kicad(&self) -> &'static str {
        match self {
            PadShape::Circle => "circle",
            PadShape::Rect => "rect",
            PadShape::Oval => "oval",
            PadShape::Trapezoid => "trapezoid",
            PadShape::RoundRect => "roundrect",
            PadShape::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone)]
pub struct KiPad {
    pub number: String,
    pub pad_type: PadType,
    pub shape: PadShape,
    pub pos_x: f64,
    pub pos_y: f64,
    pub size_x: f64,
    pub size_y: f64,
    pub rotation: f64,
    pub layers: Vec<String>,
    pub drill: Option<Drill>,
    pub polygon: Option<String>, // For custom polygon pads
}

#[derive(Debug, Clone)]
pub struct Drill {
    pub diameter: f64,
    pub width: Option<f64>, // For oval drills: width (if different from diameter)
    pub offset_x: f64,
    pub offset_y: f64,
}

#[derive(Debug, Clone)]
pub struct KiTrack {
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub width: f64,
    pub layer: String,
}

#[derive(Debug, Clone)]
pub struct KiCircle {
    pub center_x: f64,
    pub center_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub width: f64,
    pub layer: String,
    pub fill: bool,
}

#[derive(Debug, Clone)]
pub struct KiArc {
    pub start_x: f64,
    pub start_y: f64,
    pub mid_x: f64,
    pub mid_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub width: f64,
    pub layer: String,
}

#[derive(Debug, Clone)]
pub struct KiLine {
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub width: f64,
    pub layer: String,
}

#[derive(Debug, Clone)]
pub struct KiText {
    pub text: String,
    pub pos_x: f64,
    pub pos_y: f64,
    pub rotation: f64,
    pub layer: String,
    pub size: f64,
    pub thickness: f64,
}

#[derive(Debug, Clone)]
pub struct Ki3dModel {
    pub path: String,
    pub offset: (f64, f64, f64),
    pub scale: (f64, f64, f64),
    pub rotate: (f64, f64, f64),
}
