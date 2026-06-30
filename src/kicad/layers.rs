//! KiCad layer mapping for EasyEDA footprints.

/// Map EasyEDA layer ID to KiCad layer name for general graphics
pub fn map_layer(layer_id: i32) -> String {
    match layer_id {
        1 => "F.Cu".to_string(),       // Front copper
        2 => "B.Cu".to_string(),       // Back copper
        3 => "F.SilkS".to_string(),    // Front silk screen
        4 => "B.SilkS".to_string(),    // Back silk screen
        5 => "F.Paste".to_string(),    // Front paste
        6 => "B.Paste".to_string(),    // Back paste
        7 => "F.Mask".to_string(),     // Front mask
        8 => "B.Mask".to_string(),     // Back mask
        10 => "Edge.Cuts".to_string(), // Board edge
        11 => "Edge.Cuts".to_string(), // Board edge (duplicate)
        12 => "Cmts.User".to_string(), // User comments
        13 => "F.Fab".to_string(),     // Front fabrication
        14 => "B.Fab".to_string(),     // Back fabrication
        15 => "Dwgs.User".to_string(), // User drawings
        101 => "F.Fab".to_string(),    // Front fabrication (alternate)
        _ => "F.SilkS".to_string(),    // Default to front silk screen
    }
}

/// Map EasyEDA layer ID to KiCad pad layers for SMD pads
pub fn map_pad_layers_smd(layer_id: i32) -> Vec<String> {
    match layer_id {
        1 => vec![
            "F.Cu".to_string(),
            "F.Paste".to_string(),
            "F.Mask".to_string(),
        ],
        2 => vec![
            "B.Cu".to_string(),
            "B.Paste".to_string(),
            "B.Mask".to_string(),
        ],
        3 => vec!["F.SilkS".to_string()],
        11 => vec![
            "*.Cu".to_string(),
            "*.Paste".to_string(),
            "*.Mask".to_string(),
        ],
        13 => vec!["F.Fab".to_string()],
        15 => vec!["Dwgs.User".to_string()],
        _ => vec![
            "F.Cu".to_string(),
            "F.Paste".to_string(),
            "F.Mask".to_string(),
        ],
    }
}

/// Map EasyEDA layer ID to KiCad pad layers for through-hole pads
/// Note: Through-hole pads don't have paste layers
pub fn map_pad_layers_tht(layer_id: i32) -> Vec<String> {
    match layer_id {
        1 => vec!["F.Cu".to_string(), "F.Mask".to_string()],
        2 => vec!["B.Cu".to_string(), "B.Mask".to_string()],
        3 => vec!["F.SilkS".to_string()],
        11 => vec!["*.Cu".to_string(), "*.Mask".to_string()],
        13 => vec!["F.Fab".to_string()],
        15 => vec!["Dwgs.User".to_string()],
        _ => vec!["*.Cu".to_string(), "*.Mask".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_layer() {
        assert_eq!(map_layer(1), "F.Cu");
        assert_eq!(map_layer(2), "B.Cu");
        assert_eq!(map_layer(3), "F.SilkS");
        assert_eq!(map_layer(13), "F.Fab");
    }

    #[test]
    fn test_map_pad_layers_smd() {
        let layers = map_pad_layers_smd(1);
        assert_eq!(layers, vec!["F.Cu", "F.Paste", "F.Mask"]);

        let layers = map_pad_layers_smd(2);
        assert_eq!(layers, vec!["B.Cu", "B.Paste", "B.Mask"]);
    }

    #[test]
    fn test_map_pad_layers_tht() {
        let layers = map_pad_layers_tht(1);
        assert_eq!(layers, vec!["F.Cu", "F.Mask"]);

        let layers = map_pad_layers_tht(11);
        assert_eq!(layers, vec!["*.Cu", "*.Mask"]);
    }
}
