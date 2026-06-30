#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
}

impl PinType {
    pub fn from_easyeda(electric_type: &str) -> Self {
        match electric_type {
            "I" => PinType::Input,
            "O" => PinType::Output,
            "B" => PinType::Bidirectional,
            "T" => PinType::TriState,
            "P" => PinType::Passive,
            "U" => PinType::Unspecified,
            "W" => PinType::PowerIn,
            "w" => PinType::PowerOut,
            "C" => PinType::OpenCollector,
            "E" => PinType::OpenEmitter,
            "N" => PinType::NoConnect,
            _ => PinType::Passive, // Default to Passive
        }
    }

    pub fn to_kicad_v6(&self) -> &'static str {
        match self {
            PinType::Input => "input",
            PinType::Output => "output",
            PinType::Bidirectional => "bidirectional",
            PinType::TriState => "tri_state",
            PinType::Passive => "passive",
            PinType::Unspecified => "unspecified",
            PinType::PowerIn => "power_in",
            PinType::PowerOut => "power_out",
            PinType::OpenCollector => "open_collector",
            PinType::OpenEmitter => "open_emitter",
            PinType::NoConnect => "no_connect",
        }
    }

    pub fn to_kicad_v5(&self) -> &'static str {
        match self {
            PinType::Input => "I",
            PinType::Output => "O",
            PinType::Bidirectional => "B",
            PinType::TriState => "T",
            PinType::Passive => "P",
            PinType::Unspecified => "U",
            PinType::PowerIn => "W",
            PinType::PowerOut => "w",
            PinType::OpenCollector => "C",
            PinType::OpenEmitter => "E",
            PinType::NoConnect => "N",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinStyle {
    Line,
    Inverted,
    Clock,
    InvertedClock,
    InputLow,
    ClockLow,
    OutputLow,
    EdgeClockHigh,
    NonLogic,
}

impl PinStyle {
    pub fn to_kicad_v6(&self) -> &'static str {
        match self {
            PinStyle::Line => "line",
            PinStyle::Inverted => "inverted",
            PinStyle::Clock => "clock",
            PinStyle::InvertedClock => "inverted_clock",
            PinStyle::InputLow => "input_low",
            PinStyle::ClockLow => "clock_low",
            PinStyle::OutputLow => "output_low",
            PinStyle::EdgeClockHigh => "edge_clock_high",
            PinStyle::NonLogic => "non_logic",
        }
    }

    pub fn to_kicad_v5(&self) -> &'static str {
        match self {
            PinStyle::Line => "",
            PinStyle::Inverted => "I",
            PinStyle::Clock => "C",
            PinStyle::InvertedClock => "IC",
            PinStyle::InputLow => "L",
            PinStyle::ClockLow => "CL",
            PinStyle::OutputLow => "V",
            PinStyle::EdgeClockHigh => "F",
            PinStyle::NonLogic => "X",
        }
    }
}

#[derive(Debug, Clone)]
pub struct KiSymbol {
    pub name: String,
    pub reference: String,
    pub value: String,
    pub description: String,
    pub footprint: String,
    pub datasheet: String,
    pub manufacturer: String,
    pub lcsc_id: String,
    pub jlc_id: String,
    pub units: Vec<KiSymbolUnit>,
    pub pins: Vec<KiPin>,
    pub rectangles: Vec<KiRectangle>,
    pub circles: Vec<KiCircle>,
    pub arcs: Vec<KiArc>,
    pub polylines: Vec<KiPolyline>,
    pub texts: Vec<KiText>,
}

#[derive(Debug, Clone, Default)]
pub struct KiSymbolUnit {
    pub pins: Vec<KiPin>,
    pub rectangles: Vec<KiRectangle>,
    pub circles: Vec<KiCircle>,
    pub arcs: Vec<KiArc>,
    pub polylines: Vec<KiPolyline>,
    pub texts: Vec<KiText>,
}

#[derive(Debug, Clone)]
pub struct KiText {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
    pub font_size: f64,
}

#[derive(Debug, Clone)]
pub struct KiPin {
    pub number: String,
    pub name: String,
    pub pin_type: PinType,
    pub style: PinStyle,
    pub pos_x: f64,
    pub pos_y: f64,
    pub rotation: i32,
    pub length: f64,
}

#[derive(Debug, Clone)]
pub struct KiRectangle {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub stroke_width: f64,
    pub fill: bool,
}

#[derive(Debug, Clone)]
pub struct KiCircle {
    pub cx: f64,
    pub cy: f64,
    pub radius: f64,
    pub stroke_width: f64,
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
    pub stroke_width: f64,
}

#[derive(Debug, Clone)]
pub struct KiPolyline {
    pub points: Vec<(f64, f64)>,
    pub stroke_width: f64,
    pub fill: bool,
}
