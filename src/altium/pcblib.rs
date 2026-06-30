use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use encoding_rs::WINDOWS_1252;

use crate::error::Result;

const LIBRARY_DATA_TEMPLATE: &str = include_str!("pcblib_library_data_template.txt");

pub const LAYER_TOP: u8 = 1;
pub const LAYER_BOTTOM: u8 = 32;
pub const LAYER_TOP_OVERLAY: u8 = 33;
pub const LAYER_BOTTOM_OVERLAY: u8 = 34;
pub const LAYER_TOP_PASTE: u8 = 35;
pub const LAYER_BOTTOM_PASTE: u8 = 36;
pub const LAYER_TOP_SOLDER: u8 = 37;
pub const LAYER_BOTTOM_SOLDER: u8 = 38;
pub const LAYER_MECHANICAL_1: u8 = 57;
pub const LAYER_MECHANICAL_2: u8 = 58;
pub const LAYER_MECHANICAL_5: u8 = 61;
pub const LAYER_MECHANICAL_6: u8 = 62;
pub const LAYER_MECHANICAL_8: u8 = 64;
pub const LAYER_MECHANICAL_9: u8 = 65;
pub const LAYER_MULTI: u8 = 74;
pub const PAD_HOLE_ROUND: u8 = 0;
pub const PAD_HOLE_SQUARE: u8 = 1;
pub const PAD_HOLE_SLOT: u8 = 2;
pub const PAD_SHAPE_ROUND: u8 = 1;
pub const PAD_SHAPE_RECTANGULAR: u8 = 2;
pub const PAD_SHAPE_OCTAGONAL: u8 = 3;
pub const PAD_SHAPE_ROUNDED_RECTANGLE: u8 = 9;

const FLAG_BASE: u16 = 0x08;
const FLAG_UNLOCKED: u16 = 0x04;
const FLAG_TENTING_TOP: u16 = 0x20;
const FLAG_TENTING_BOTTOM: u16 = 0x40;
const FLAG_KEEPOUT: u16 = 0x200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordPoint {
    pub x: i32,
    pub y: i32,
}
impl CoordPoint {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone)]
pub struct PcbLibrary {
    pub unique_id: String,
    pub components: Vec<PcbComponent>,
    pub models: Vec<PcbModel>,
}
impl Default for PcbLibrary {
    fn default() -> Self {
        Self {
            unique_id: stable_alpha_id("NPNP-PCBLIB", "library"),
            components: Vec::new(),
            models: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PcbComponent {
    pub name: String,
    pub description: String,
    pub height_raw: i32,
    pub pads: Vec<PcbPad>,
    pub arcs: Vec<PcbArc>,
    pub tracks: Vec<PcbTrack>,
    pub regions: Vec<PcbRegion>,
    pub bodies: Vec<PcbComponentBody>,
    pub extended_primitive_information: Vec<PcbExtendedPrimitiveInfo>,
}
impl PcbComponent {
    pub fn primitive_count(&self) -> usize {
        self.pads.len()
            + self.arcs.len()
            + self.tracks.len()
            + self.regions.len()
            + self.bodies.len()
    }
}

#[derive(Debug, Clone)]
pub struct PcbModel {
    pub id: String,
    pub name: String,
    pub is_embedded: bool,
    pub model_source: String,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    pub dz_raw: i32,
    pub checksum: i32,
    pub step_data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PcbPad {
    pub designator: String,
    pub location: CoordPoint,
    pub size_top: CoordPoint,
    pub size_middle: CoordPoint,
    pub size_bottom: CoordPoint,
    pub hole_size_raw: i32,
    pub shape_top: u8,
    pub shape_middle: u8,
    pub shape_bottom: u8,
    pub rotation: f64,
    pub is_plated: bool,
    pub layer: u8,
    pub is_locked: bool,
    pub is_tenting_top: bool,
    pub is_tenting_bottom: bool,
    pub is_keepout: bool,
    pub mode: u8,
    pub power_plane_connect_style: u8,
    pub relief_air_gap_raw: i32,
    pub relief_conductor_width_raw: i32,
    pub relief_entries: i16,
    pub power_plane_clearance_raw: i32,
    pub power_plane_relief_expansion_raw: i32,
    pub paste_mask_expansion_raw: i32,
    pub solder_mask_expansion_raw: i32,
    pub drill_type: u8,
    pub jumper_id: i16,
    pub hole_type: u8,
    pub hole_slot_length_raw: i32,
    pub hole_rotation: f64,
    pub corner_radius_percentage: u8,
}

#[derive(Debug, Clone)]
pub struct PcbTrack {
    pub layer: u8,
    pub start: CoordPoint,
    pub end: CoordPoint,
    pub width_raw: i32,
    pub is_locked: bool,
    pub is_tenting_top: bool,
    pub is_tenting_bottom: bool,
    pub is_keepout: bool,
    pub net_index: u16,
    pub component_index: u8,
}

#[derive(Debug, Clone)]
pub struct PcbArc {
    pub layer: u8,
    pub center: CoordPoint,
    pub radius_raw: i32,
    pub start_angle: f64,
    pub end_angle: f64,
    pub width_raw: i32,
    pub is_locked: bool,
    pub is_tenting_top: bool,
    pub is_tenting_bottom: bool,
    pub is_keepout: bool,
}

#[derive(Debug, Clone)]
pub struct PcbRegion {
    pub layer: u8,
    pub outline: Vec<CoordPoint>,
    pub kind: i32,
    pub net: Option<String>,
    pub unique_id: Option<String>,
    pub name: Option<String>,
    pub is_locked: bool,
    pub is_tenting_top: bool,
    pub is_tenting_bottom: bool,
    pub is_keepout: bool,
    pub additional_params: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct PcbExtendedPrimitiveInfo {
    pub primitive_index: usize,
    pub object_name: String,
    pub params: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct PcbComponentBody {
    pub layer_name: String,
    pub name: String,
    pub kind: i32,
    pub subpoly_index: i32,
    pub union_index: i32,
    pub arc_resolution_raw: i32,
    pub is_shape_based: bool,
    pub cavity_height_raw: i32,
    pub standoff_height_raw: i32,
    pub overall_height_raw: i32,
    pub body_color_3d: i32,
    pub body_opacity_3d: f64,
    pub body_projection: i32,
    pub model_id: String,
    pub model_embed: bool,
    pub model_2d_location: CoordPoint,
    pub model_2d_rotation: f64,
    pub model_3d_rot_x: f64,
    pub model_3d_rot_y: f64,
    pub model_3d_rot_z: f64,
    pub model_3d_dz_raw: i32,
    pub model_checksum: i32,
    pub model_name: String,
    pub model_type: i32,
    pub model_source: String,
    pub identifier: Option<String>,
    pub texture: String,
    pub outline: Vec<CoordPoint>,
    pub is_locked: bool,
    pub is_tenting_top: bool,
    pub is_tenting_bottom: bool,
    pub is_keepout: bool,
}

pub fn write_pcblib(library: &PcbLibrary, output_path: &Path) -> Result<()> {
    let file = File::create(output_path)?;
    let mut compound = cfb::CompoundFile::create(file)?;
    let sections = collect_sections(&library.components);
    let section_keys = collect_section_key_pairs(&sections);

    write_stream(&mut compound, "/FileHeader", &file_header_bytes(library))?;
    if !section_keys.is_empty() {
        write_stream(
            &mut compound,
            "/SectionKeys",
            &section_keys_bytes(&section_keys),
        )?;
    }

    compound.create_storage("/Library/")?;
    write_stream(&mut compound, "/Library/Header", &storage_header_bytes(1))?;
    write_stream(
        &mut compound,
        "/Library/Data",
        &library_data_bytes(library, output_path),
    )?;

    compound.create_storage("/Library/Models/")?;
    write_stream(
        &mut compound,
        "/Library/Models/Header",
        &storage_header_bytes(library.models.len() as i32),
    )?;
    write_stream(
        &mut compound,
        "/Library/Models/Data",
        &models_data_bytes(library),
    )?;
    for (index, model) in library.models.iter().enumerate() {
        write_stream(
            &mut compound,
            &format!("/Library/Models/{index}"),
            &zlib_store(&model.step_data),
        )?;
    }

    compound.create_storage("/Library/Textures/")?;
    write_stream(
        &mut compound,
        "/Library/Textures/Header",
        &storage_header_bytes(0),
    )?;
    write_stream(&mut compound, "/Library/Textures/Data", &[])?;
    compound.create_storage("/Library/ModelsNoEmbed/")?;
    write_stream(
        &mut compound,
        "/Library/ModelsNoEmbed/Header",
        &storage_header_bytes(0),
    )?;
    write_stream(&mut compound, "/Library/ModelsNoEmbed/Data", &[])?;

    for (component, section_key) in &sections {
        compound.create_storage(&format!("/{section_key}/"))?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/Header"),
            &storage_header_bytes(component.primitive_count() as i32),
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/Parameters"),
            &component_parameters_bytes(component),
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/WideStrings"),
            &wide_strings_bytes(),
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/Data"),
            &component_data_bytes(component),
        )?;
        if !component.extended_primitive_information.is_empty() {
            compound.create_storage(&format!("/{section_key}/ExtendedPrimitiveInformation/"))?;
            write_stream(
                &mut compound,
                &format!("/{section_key}/ExtendedPrimitiveInformation/Header"),
                &storage_header_bytes(component.extended_primitive_information.len() as i32),
            )?;
            write_stream(
                &mut compound,
                &format!("/{section_key}/ExtendedPrimitiveInformation/Data"),
                &extended_primitive_information_bytes(component),
            )?;
        }
        compound.create_storage(&format!("/{section_key}/UniqueIdPrimitiveInformation/"))?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/UniqueIdPrimitiveInformation/Header"),
            &storage_header_bytes(component.primitive_count() as i32),
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/UniqueIdPrimitiveInformation/Data"),
            &unique_id_primitive_information_bytes(component),
        )?;
    }

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

fn collect_sections<'a>(components: &'a [PcbComponent]) -> Vec<(&'a PcbComponent, String)> {
    let mut used = HashSet::new();
    components
        .iter()
        .map(|component| {
            let section_key = unique_section_key(&component.name, &mut used);
            (component, section_key)
        })
        .collect()
}

fn collect_section_key_pairs(sections: &[(&PcbComponent, String)]) -> Vec<(String, String)> {
    sections
        .iter()
        .filter_map(|(component, section_key)| {
            (section_key.as_str() != component.name.as_str())
                .then(|| (component.name.clone(), section_key.clone()))
        })
        .collect()
}

fn unique_section_key(name: &str, used: &mut HashSet<String>) -> String {
    let base = section_key_from_name(name);
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

pub fn section_key_from_name(name: &str) -> String {
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

fn file_header_bytes(_library: &PcbLibrary) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    let version_text = "PCB 6.0 Binary Library File";
    writer.write_i32(version_text.len() as i32);
    writer.write_pascal_short_string(version_text);
    writer.into_inner()
}

fn section_keys_bytes(section_keys: &[(String, String)]) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    writer.write_i32(section_keys.len() as i32);
    for (name, key) in section_keys {
        writer.write_pascal_string(name);
        writer.write_string_block(key);
    }
    writer.into_inner()
}

fn storage_header_bytes(record_count: i32) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    writer.write_i32(record_count);
    writer.into_inner()
}

fn library_data_bytes(library: &PcbLibrary, output_path: &Path) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    writer.write_block(0, |w| w.write_cstring(&library_data_params(output_path)));
    writer.write_u32(library.components.len() as u32);
    for component in &library.components {
        writer.write_string_block(&component.name);
    }
    writer.into_inner()
}

fn library_data_params(output_path: &Path) -> String {
    let mut filename = output_path
        .canonicalize()
        .unwrap_or_else(|_| output_path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\");
    if let Some(stripped) = filename.strip_prefix("\\\\?\\") {
        filename = stripped.to_string();
    }
    let (date_text, time_text) = current_library_date_time();
    LIBRARY_DATA_TEMPLATE
        .replace("__FILE__", &filename)
        .replace("__DATE__", &date_text)
        .replace("__TIME__", &time_text)
}

fn current_library_date_time() -> (String, String) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour24 = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (hour12, suffix) = match hour24 {
        0 => (12, "AM"),
        1..=11 => (hour24, "AM"),
        12 => (12, "PM"),
        _ => (hour24 - 12, "PM"),
    };
    (
        format!("{month}/{day}/{year}"),
        format!("{hour12}:{minute:02}:{second:02} {suffix}"),
    )
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

fn models_data_bytes(library: &PcbLibrary) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    for model in &library.models {
        let text = format!(
            "EMBED={}|MODELSOURCE={}|ID={}|ROTX={}|ROTY={}|ROTZ={}|DZ={}|CHECKSUM={}|NAME={}",
            bool_text(model.is_embedded),
            model.model_source,
            model.id,
            format_decimal(model.rotation_x),
            format_decimal(model.rotation_y),
            format_decimal(model.rotation_z),
            model.dz_raw,
            model.checksum,
            model.name,
        );
        let mut block = BinaryWriter::default();
        block.write_cstring(&text);
        let data = block.into_inner();
        writer.write_i32(data.len() as i32);
        writer.write_bytes(&data);
    }
    writer.into_inner()
}

fn component_parameters_bytes(component: &PcbComponent) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    let mut params = Params::default();
    params.push("PATTERN", &component.name);
    params.push("HEIGHT", component.height_raw.to_string());
    if !component.description.trim().is_empty() {
        params.push("DESCRIPTION", &component.description);
    }
    writer.write_cstring_param_block(&params);
    writer.into_inner()
}

fn wide_strings_bytes() -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    writer.write_cstring_param_block(&Params::default());
    writer.into_inner()
}

fn component_data_bytes(component: &PcbComponent) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    writer.write_string_block(&component.name);
    for pad in &component.pads {
        writer.write_u8(2);
        write_pad(&mut writer, pad);
    }
    for track in &component.tracks {
        writer.write_u8(4);
        write_track(&mut writer, track);
    }
    for arc in &component.arcs {
        writer.write_u8(1);
        write_arc(&mut writer, arc);
    }
    for region in &component.regions {
        writer.write_u8(11);
        write_region(&mut writer, region);
    }
    for body in &component.bodies {
        writer.write_u8(12);
        write_component_body(&mut writer, body);
    }
    writer.into_inner()
}

fn primitive_object_names(component: &PcbComponent) -> Vec<&'static str> {
    let mut names = Vec::with_capacity(component.primitive_count());
    names.extend(std::iter::repeat_n("Pad", component.pads.len()));
    names.extend(std::iter::repeat_n("Track", component.tracks.len()));
    names.extend(std::iter::repeat_n("Arc", component.arcs.len()));
    names.extend(std::iter::repeat_n("Region", component.regions.len()));
    names.extend(std::iter::repeat_n("ComponentBody", component.bodies.len()));
    names
}

fn extended_primitive_information_bytes(component: &PcbComponent) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    for info in &component.extended_primitive_information {
        let mut params = Params::default();
        params.push("PRIMITIVEINDEX", info.primitive_index.to_string());
        params.push("PRIMITIVEOBJECTID", &info.object_name);
        for (key, value) in &info.params {
            params.push(key, value);
        }
        writer.write_cstring_param_block(&params);
    }
    writer.into_inner()
}

fn unique_id_primitive_information_bytes(component: &PcbComponent) -> Vec<u8> {
    let mut writer = BinaryWriter::default();
    for (index, object_name) in primitive_object_names(component).into_iter().enumerate() {
        let mut params = Params::default();
        if index > 0 {
            params.push("PRIMITIVEINDEX", index.to_string());
        }
        params.push("PRIMITIVEOBJECTID", object_name);
        writer.write_cstring_param_block(&params);
    }
    writer.into_inner()
}

fn write_arc(writer: &mut BinaryWriter, arc: &PcbArc) {
    writer.write_block(0, |w| {
        write_common_primitive_data(
            w,
            arc.layer,
            encode_flags(
                arc.is_locked,
                arc.is_tenting_top,
                arc.is_tenting_bottom,
                arc.is_keepout,
            ),
        );
        w.write_coord_point(arc.center);
        w.write_coord(arc.radius_raw);
        w.write_f64(arc.start_angle);
        w.write_f64(arc.end_angle);
        w.write_coord(arc.width_raw);
    });
}

fn write_track(writer: &mut BinaryWriter, track: &PcbTrack) {
    writer.write_block(0, |w| {
        write_common_primitive_data(
            w,
            track.layer,
            encode_flags(
                track.is_locked,
                track.is_tenting_top,
                track.is_tenting_bottom,
                track.is_keepout,
            ),
        );
        w.write_coord_point(track.start);
        w.write_coord_point(track.end);
        w.write_coord(track.width_raw);
        w.write_u16(track.net_index);
        w.write_u8(track.component_index);
    });
}

fn write_region(writer: &mut BinaryWriter, region: &PcbRegion) {
    writer.write_block(0, |w| {
        write_common_primitive_data(
            w,
            region.layer,
            encode_flags(
                region.is_locked,
                region.is_tenting_top,
                region.is_tenting_bottom,
                region.is_keepout,
            ),
        );
        w.write_u32(0);
        w.write_u8(0);
        let mut params = Params::default();
        for (key, value) in &region.additional_params {
            params.push(key, value);
        }
        if region.kind != 0 {
            params.push("KIND", region.kind.to_string());
        }
        if let Some(net) = &region.net {
            if !net.is_empty() {
                params.push("NET", net);
            }
        }
        if let Some(unique_id) = &region.unique_id {
            if !unique_id.is_empty() {
                params.push("UNIQUEID", unique_id);
            }
        }
        if let Some(name) = &region.name {
            if !name.is_empty() {
                params.push("NAME", name);
            }
        }
        w.write_cstring_param_block(&params);
        w.write_u32(region.outline.len() as u32);
        for point in &region.outline {
            w.write_f64(point.x as f64);
            w.write_f64(point.y as f64);
        }
    });
}

fn write_component_body(writer: &mut BinaryWriter, body: &PcbComponentBody) {
    writer.write_block(0, |w| {
        write_common_primitive_data(
            w,
            layer_name_to_byte(&body.layer_name),
            encode_flags(
                body.is_locked,
                body.is_tenting_top,
                body.is_tenting_bottom,
                body.is_keepout,
            ),
        );
        w.write_u32(0);
        w.write_u8(0);
        let mut params = Params::default();
        params.push("V7_LAYER", &body.layer_name);
        params.push("NAME", &body.name);
        params.push("KIND", body.kind.to_string());
        params.push("SUBPOLYINDEX", body.subpoly_index.to_string());
        params.push("UNIONINDEX", body.union_index.to_string());
        params.push("ARCRESOLUTION", format_raw_mil(body.arc_resolution_raw));
        params.push("ISSHAPEBASED", bool_text(body.is_shape_based));
        params.push("CAVITYHEIGHT", format_raw_mil(body.cavity_height_raw));
        params.push("STANDOFFHEIGHT", format_raw_mil(body.standoff_height_raw));
        params.push("OVERALLHEIGHT", format_raw_mil(body.overall_height_raw));
        params.push("BODYPROJECTION", body.body_projection.to_string());
        params.push("BODYCOLOR3D", body.body_color_3d.to_string());
        params.push("BODYOPACITY3D", format_decimal(body.body_opacity_3d));
        params.push("IDENTIFIER", body.identifier.clone().unwrap_or_default());
        params.push("TEXTURE", &body.texture);
        params.push("TEXTURECENTERX", "0mil");
        params.push("TEXTURECENTERY", "0mil");
        params.push("TEXTURESIZEX", "0mil");
        params.push("TEXTURESIZEY", "0mil");
        params.push("TEXTUREROTATION", " 0.00000000000000E+0000");
        params.push("MODELID", &body.model_id);
        params.push("MODEL.CHECKSUM", body.model_checksum.to_string());
        params.push("MODEL.EMBED", bool_text(body.model_embed));
        params.push("MODEL.NAME", &body.model_name);
        params.push("MODEL.2D.X", format_raw_mil(body.model_2d_location.x));
        params.push("MODEL.2D.Y", format_raw_mil(body.model_2d_location.y));
        params.push("MODEL.2D.ROTATION", format_decimal(body.model_2d_rotation));
        params.push("MODEL.3D.ROTX", format_decimal(body.model_3d_rot_x));
        params.push("MODEL.3D.ROTY", format_decimal(body.model_3d_rot_y));
        params.push("MODEL.3D.ROTZ", format_decimal(body.model_3d_rot_z));
        params.push("MODEL.3D.DZ", format_raw_mil(body.model_3d_dz_raw));
        params.push("MODEL.MODELTYPE", body.model_type.to_string());
        params.push("MODEL.MODELSOURCE", &body.model_source);
        w.write_cstring_param_block(&params);
        w.write_u32(body.outline.len() as u32);
        for point in &body.outline {
            w.write_f64(point.x as f64);
            w.write_f64(point.y as f64);
        }
    });
}

fn write_pad(writer: &mut BinaryWriter, pad: &PcbPad) {
    writer.write_string_block(&pad.designator);
    writer.write_block_raw(0, &[0]);
    writer.write_string_block("|&|0");
    writer.write_block_raw(0, &[0]);

    writer.write_block(0, |w| {
        write_common_primitive_data(
            w,
            pad.layer,
            encode_flags(
                pad.is_locked,
                pad.is_tenting_top,
                pad.is_tenting_bottom,
                pad.is_keepout,
            ),
        );
        w.write_coord_point(pad.location);
        w.write_coord_point(pad.size_top);
        w.write_coord_point(pad.size_middle);
        w.write_coord_point(pad.size_bottom);
        w.write_coord(pad.hole_size_raw);
        w.write_u8(pad.shape_top);
        w.write_u8(pad.shape_middle);
        w.write_u8(pad.shape_bottom);
        w.write_f64(pad.rotation);
        w.write_bool(pad.is_plated);
        w.write_u8(0);
        w.write_u8(pad.mode);
        w.write_u8(pad.power_plane_connect_style);
        w.write_coord(pad.relief_air_gap_raw);
        w.write_coord(pad.relief_conductor_width_raw);
        w.write_i16(pad.relief_entries);
        w.write_coord(pad.power_plane_clearance_raw);
        w.write_coord(pad.power_plane_relief_expansion_raw);
        w.write_i32(0);
        w.write_coord(pad.paste_mask_expansion_raw);
        w.write_coord(pad.solder_mask_expansion_raw);
        w.write_bytes(&[0; 7]);
        w.write_u8(if pad.paste_mask_expansion_raw != 0 {
            2
        } else {
            0
        });
        w.write_u8(if pad.solder_mask_expansion_raw != 0 {
            2
        } else {
            1
        });
        w.write_u8(pad.drill_type);
        w.write_i16(0);
        w.write_i32(0);
        w.write_i16(pad.jumper_id);
        w.write_i16(0);
    });

    writer.write_block(0, |w| write_pad_extended_block(w, pad));
}

fn write_pad_extended_block(writer: &mut BinaryWriter, pad: &PcbPad) {
    for _ in 0..29 {
        writer.write_coord(pad.size_middle.x);
    }
    for _ in 0..29 {
        writer.write_coord(pad.size_middle.y);
    }
    for _ in 0..29 {
        writer.write_u8(pad.shape_middle);
    }
    writer.write_u8(0);
    writer.write_u8(pad.hole_type);
    writer.write_coord(pad.hole_slot_length_raw);
    writer.write_f64(pad.hole_rotation);
    for _ in 0..32 {
        writer.write_coord(0);
    }
    for _ in 0..32 {
        writer.write_coord(0);
    }
    let has_rounded_rect =
        [pad.shape_top, pad.shape_middle, pad.shape_bottom].contains(&PAD_SHAPE_ROUNDED_RECTANGLE);
    writer.write_bool(has_rounded_rect);
    writer.write_u8(pad.shape_top);
    for _ in 0..30 {
        writer.write_u8(pad.shape_middle);
    }
    writer.write_u8(pad.shape_bottom);
    for _ in 0..32 {
        writer.write_u8(pad.corner_radius_percentage);
    }
}

fn write_common_primitive_data(writer: &mut BinaryWriter, layer: u8, flags: u16) {
    writer.write_u8(layer);
    writer.write_u16(flags);
    writer.write_fill(0xFF, 10);
}

fn encode_flags(
    is_locked: bool,
    is_tenting_top: bool,
    is_tenting_bottom: bool,
    is_keepout: bool,
) -> u16 {
    let mut flags = FLAG_BASE;
    if !is_locked {
        flags |= FLAG_UNLOCKED;
    }
    if is_tenting_top {
        flags |= FLAG_TENTING_TOP;
    }
    if is_tenting_bottom {
        flags |= FLAG_TENTING_BOTTOM;
    }
    if is_keepout {
        flags |= FLAG_KEEPOUT;
    }
    flags
}

pub fn stable_alpha_id(name: &str, salt: &str) -> String {
    const ALPHABET: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut hash = fnv1a64(name.as_bytes()) ^ fnv1a64(salt.as_bytes());
    let mut id = String::with_capacity(8);
    for _ in 0..8 {
        id.push(ALPHABET[(hash % 26) as usize] as char);
        hash /= 26;
    }
    id
}

pub fn stable_guid(seed: &str) -> String {
    let hi = fnv1a64(seed.as_bytes());
    let mut reversed = seed.as_bytes().to_vec();
    reversed.reverse();
    let lo = fnv1a64(&reversed);
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:04X}-{:012X}}}",
        (hi >> 32) as u32,
        ((hi >> 16) & 0xFFFF) as u16,
        (hi & 0xFFFF) as u16,
        ((lo >> 48) & 0xFFFF) as u16,
        lo & 0x0000_FFFF_FFFF_FFFF
    )
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

fn bool_text(value: bool) -> &'static str {
    if value { "TRUE" } else { "FALSE" }
}
fn format_decimal(value: f64) -> String {
    format!("{value:.3}")
}
fn format_raw_mil(raw_value: i32) -> String {
    let mut text = format!("{:.4}", raw_value as f64 / 10_000.0);
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        text = "0".to_string();
    }
    format!("{text}mil")
}

fn layer_name_to_byte(layer_name: &str) -> u8 {
    let name = layer_name
        .trim()
        .to_ascii_uppercase()
        .replace([' ', '-'], "");
    if let Some(rest) = name.strip_prefix("MECHANICAL") {
        if let Ok(number) = rest.parse::<u8>() {
            if (1..=16).contains(&number) {
                return 56 + number;
            }
        }
    }
    match name.as_str() {
        "TOPLAYER" | "TOP" => LAYER_TOP,
        "BOTTOMLAYER" | "BOTTOM" => LAYER_BOTTOM,
        "TOPOVERLAY" => LAYER_TOP_OVERLAY,
        "BOTTOMOVERLAY" => LAYER_BOTTOM_OVERLAY,
        "TOPPASTE" => LAYER_TOP_PASTE,
        "BOTTOMPASTE" => LAYER_BOTTOM_PASTE,
        "TOPSOLDER" => LAYER_TOP_SOLDER,
        "BOTTOMSOLDER" => LAYER_BOTTOM_SOLDER,
        "MULTILAYER" => LAYER_MULTI,
        _ => 0,
    }
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + (data.len() / 65_535 + 1) * 5 + 6);
    out.push(0x78);
    out.push(0x01);
    if data.is_empty() {
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(!0u16).to_le_bytes());
    } else {
        let mut offset = 0usize;
        while offset < data.len() {
            let chunk_len = (data.len() - offset).min(65_535);
            let final_block = offset + chunk_len >= data.len();
            out.push(if final_block { 0x01 } else { 0x00 });
            let len = chunk_len as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(&data[offset..offset + chunk_len]);
            offset += chunk_len;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % MOD_ADLER;
        b = (b + a) % MOD_ADLER;
    }
    (b << 16) | a
}

#[derive(Debug, Default)]
struct Params(Vec<(String, String)>);
impl Params {
    fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.push((key.into(), value.into()));
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
struct BinaryWriter {
    data: Vec<u8>,
}
impl BinaryWriter {
    fn into_inner(self) -> Vec<u8> {
        self.data
    }
    fn write_u8(&mut self, value: u8) {
        self.data.push(value);
    }
    fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }
    fn write_i16(&mut self, value: i16) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    fn write_u16(&mut self, value: u16) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    fn write_i32(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    fn write_u32(&mut self, value: u32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    fn write_f64(&mut self, value: f64) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }
    fn write_bytes(&mut self, value: &[u8]) {
        self.data.extend_from_slice(value);
    }
    fn write_coord(&mut self, value: i32) {
        self.write_i32(value);
    }
    fn write_coord_point(&mut self, point: CoordPoint) {
        self.write_coord(point.x);
        self.write_coord(point.y);
    }
    fn write_fill(&mut self, byte: u8, count: usize) {
        self.data.extend(std::iter::repeat_n(byte, count));
    }
    fn write_block(&mut self, flags: u8, serializer: impl FnOnce(&mut Self)) {
        let mut child = Self::default();
        serializer(&mut child);
        self.write_block_raw(flags, &child.into_inner());
    }
    fn write_block_raw(&mut self, flags: u8, data: &[u8]) {
        self.write_u32(((flags as u32) << 24) | data.len() as u32);
        self.write_bytes(data);
    }
    fn write_pascal_short_string(&mut self, value: &str) {
        let bytes = encode_ansi_lossy(value);
        let len = bytes.len().min(255);
        self.write_u8(len as u8);
        self.write_bytes(&bytes[..len]);
    }
    fn write_cstring(&mut self, value: &str) {
        self.write_bytes(&encode_ansi_lossy(value));
        self.write_u8(0);
    }
    fn write_string_block(&mut self, value: &str) {
        self.write_block(0, |writer| writer.write_pascal_short_string(value));
    }
    fn write_pascal_string(&mut self, value: &str) {
        self.write_block(0, |writer| {
            writer.write_pascal_short_string(value);
            writer.write_u8(0);
        });
    }
    fn write_cstring_param_block(&mut self, params: &Params) {
        let text = params.as_string();
        self.write_block(0, |writer| writer.write_cstring(&text));
    }
}

fn encode_ansi_lossy(text: &str) -> Vec<u8> {
    let sanitized = text.replace('\0', "?");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_pcblib_compound_streams() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("npnp_pcblib_{timestamp}.PcbLib"));
        let component = PcbComponent {
            name: "TEST/FOOTPRINT".to_string(),
            description: "Generated".to_string(),
            height_raw: 100_000,
            pads: vec![PcbPad {
                designator: "1".to_string(),
                location: CoordPoint::new(0, 0),
                size_top: CoordPoint::new(100_000, 50_000),
                size_middle: CoordPoint::new(100_000, 50_000),
                size_bottom: CoordPoint::new(100_000, 50_000),
                hole_size_raw: 0,
                shape_top: PAD_SHAPE_ROUNDED_RECTANGLE,
                shape_middle: PAD_SHAPE_ROUNDED_RECTANGLE,
                shape_bottom: PAD_SHAPE_ROUNDED_RECTANGLE,
                rotation: 0.0,
                is_plated: true,
                layer: LAYER_TOP,
                is_locked: false,
                is_tenting_top: false,
                is_tenting_bottom: false,
                is_keepout: false,
                mode: 0,
                power_plane_connect_style: 0,
                relief_air_gap_raw: 0,
                relief_conductor_width_raw: 0,
                relief_entries: 4,
                power_plane_clearance_raw: 0,
                power_plane_relief_expansion_raw: 0,
                paste_mask_expansion_raw: 0,
                solder_mask_expansion_raw: 0,
                drill_type: 0,
                jumper_id: 0,
                hole_type: PAD_HOLE_ROUND,
                hole_slot_length_raw: 0,
                hole_rotation: 0.0,
                corner_radius_percentage: 50,
            }],
            arcs: vec![],
            tracks: vec![PcbTrack {
                layer: LAYER_TOP_OVERLAY,
                start: CoordPoint::new(0, 0),
                end: CoordPoint::new(100_000, 0),
                width_raw: 10_000,
                is_locked: false,
                is_tenting_top: false,
                is_tenting_bottom: false,
                is_keepout: false,
                net_index: 0,
                component_index: 0,
            }],
            regions: vec![],
            bodies: vec![],
            extended_primitive_information: vec![],
        };
        let library = PcbLibrary {
            unique_id: stable_alpha_id("TEST", "library"),
            components: vec![component],
            models: vec![],
        };
        write_pcblib(&library, &path).unwrap();
        let file = File::open(&path).unwrap();
        let mut compound = cfb::CompoundFile::open(file).unwrap();
        assert!(compound.open_stream("/FileHeader").is_ok());
        assert!(compound.open_stream("/Library/Data").is_ok());
        assert!(compound.open_stream("/TEST_FOOTPRINT/Data").is_ok());
        fs::remove_file(path).ok();
    }

    #[test]
    fn writes_full_pad_extended_slot_metadata_block() {
        let pad = PcbPad {
            designator: "13".to_string(),
            location: CoordPoint::new(0, 0),
            size_top: CoordPoint::new(433_070, 787_400),
            size_middle: CoordPoint::new(433_070, 787_400),
            size_bottom: CoordPoint::new(433_070, 787_400),
            hole_size_raw: 236_220,
            shape_top: PAD_SHAPE_ROUNDED_RECTANGLE,
            shape_middle: PAD_SHAPE_ROUNDED_RECTANGLE,
            shape_bottom: PAD_SHAPE_ROUNDED_RECTANGLE,
            rotation: 90.0,
            is_plated: true,
            layer: LAYER_MULTI,
            is_locked: false,
            is_tenting_top: false,
            is_tenting_bottom: false,
            is_keepout: false,
            mode: 0,
            power_plane_connect_style: 0,
            relief_air_gap_raw: 0,
            relief_conductor_width_raw: 0,
            relief_entries: 4,
            power_plane_clearance_raw: 0,
            power_plane_relief_expansion_raw: 0,
            paste_mask_expansion_raw: 0,
            solder_mask_expansion_raw: 0,
            drill_type: 0,
            jumper_id: 0,
            hole_type: PAD_HOLE_SLOT,
            hole_slot_length_raw: 590_550,
            hole_rotation: 90.0,
            corner_radius_percentage: 50,
        };
        let mut writer = BinaryWriter::default();
        write_pad(&mut writer, &pad);
        let bytes = writer.into_inner();
        let extended = last_block_payload(&bytes);
        let hole_shape_offset = (29 * 4) + (29 * 4) + 29 + 1;
        let slot_length_offset = hole_shape_offset + 1;
        let hole_rotation_offset = slot_length_offset + 4;
        let has_rounded_offset = hole_rotation_offset + 8 + (32 * 4) + (32 * 4);
        let rounded_shapes_offset = has_rounded_offset + 1;
        let corner_radius_offset = rounded_shapes_offset + 32;

        assert_eq!(extended.len(), 596);
        assert_eq!(extended[hole_shape_offset], PAD_HOLE_SLOT);
        assert_eq!(
            i32::from_le_bytes(
                extended[slot_length_offset..slot_length_offset + 4]
                    .try_into()
                    .unwrap()
            ),
            590_550
        );
        assert_eq!(
            f64::from_le_bytes(
                extended[hole_rotation_offset..hole_rotation_offset + 8]
                    .try_into()
                    .unwrap()
            ),
            90.0
        );
        assert_eq!(extended[has_rounded_offset], 1);
        assert_eq!(extended[rounded_shapes_offset], PAD_SHAPE_ROUNDED_RECTANGLE);
        assert_eq!(
            extended[rounded_shapes_offset + 31],
            PAD_SHAPE_ROUNDED_RECTANGLE
        );
        assert!(
            extended[corner_radius_offset..]
                .iter()
                .all(|value| *value == 50)
        );
    }

    fn last_block_payload(bytes: &[u8]) -> &[u8] {
        let mut offset = 0usize;
        let mut last_payload = &[][..];
        while offset + 4 <= bytes.len() {
            let header = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            let length = (header & 0x00FF_FFFF) as usize;
            let start = offset + 4;
            let end = start + length;
            assert!(end <= bytes.len());
            last_payload = &bytes[start..end];
            offset = end;
        }
        assert_eq!(offset, bytes.len());
        last_payload
    }

    #[test]
    fn writes_unique_section_keys_for_colliding_component_names() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("npnp_pcblib_multi_{timestamp}.PcbLib"));
        let name_a = format!("{}1", "A".repeat(31));
        let name_b = format!("{}2", "A".repeat(31));
        let make_component = |name: String| PcbComponent {
            name,
            description: "Generated".to_string(),
            height_raw: 100_000,
            pads: vec![],
            arcs: vec![],
            tracks: vec![],
            regions: vec![],
            bodies: vec![],
            extended_primitive_information: vec![],
        };
        let library = PcbLibrary {
            unique_id: stable_alpha_id("TEST_MULTI", "library"),
            components: vec![make_component(name_a), make_component(name_b)],
            models: vec![],
        };

        write_pcblib(&library, &path).unwrap();
        let file = File::open(&path).unwrap();
        let mut compound = cfb::CompoundFile::open(file).unwrap();
        let first_key = "A".repeat(31);
        let second_key = format!("{}{}", "A".repeat(29), "_2");
        assert!(compound.open_stream(&format!("/{first_key}/Data")).is_ok());
        assert!(compound.open_stream(&format!("/{second_key}/Data")).is_ok());
        fs::remove_file(path).ok();
    }
}
