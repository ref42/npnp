use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cfb::CompoundFile;
use encoding_rs::WINDOWS_1252;

use super::pcblib::{PcbLibrary, write_pcblib};
use super::schlib::Component;
use crate::error::{AppError, Result};

const PCBLIB_LIBRARY_DATA_TEMPLATE: &str = include_str!("pcblib_library_data_template.txt");

#[derive(Debug, Clone)]
pub(crate) struct SchlibRecord {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) header_part_count: usize,
    pub(crate) weight: usize,
    pub(crate) identity: Option<String>,
    pub(crate) data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct PcblibComponentRecord {
    pub(crate) name: String,
    pub(crate) primitive_count: i32,
    pub(crate) parameters: Vec<u8>,
    pub(crate) wide_strings: Vec<u8>,
    pub(crate) data: Vec<u8>,
    pub(crate) unique_id_primitive_information: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct PcblibModelRecord {
    pub(crate) entry: Vec<u8>,
    pub(crate) stream: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PcblibRecordLibrary {
    pub(crate) components: Vec<PcblibComponentRecord>,
    pub(crate) models: Vec<PcblibModelRecord>,
}

pub(crate) fn schlib_record_from_component(component: &Component) -> Result<SchlibRecord> {
    parse_schlib_record(
        component.schlib_name().to_string(),
        component.schlib_data_bytes(),
    )
}

pub(crate) fn pcblib_records_from_library(library: &PcbLibrary) -> Result<PcblibRecordLibrary> {
    let path = temp_path("npnp_pcblib_record", "PcbLib");
    let result = (|| {
        write_pcblib(library, &path)?;
        read_pcblib_records(&path)
    })();
    fs::remove_file(&path).ok();
    result
}

pub(crate) fn read_schlib_records(path: &Path) -> Result<Vec<SchlibRecord>> {
    let file = File::open(path)?;
    let mut compound = CompoundFile::open(file)?;
    let header = read_stream_bytes(&mut compound, "/FileHeader")?;
    let header_pairs = first_schlib_param_block_pairs(&header, "SchLib file header")?;
    let count = parse_usize_param(&header_pairs, "COMPCOUNT").unwrap_or(0);
    let names: Vec<String> = (0..count)
        .filter_map(|index| param_value(&header_pairs, &format!("LIBREF{index}")))
        .map(ToOwned::to_owned)
        .collect();

    let derived_sections = collect_sections(names.iter().map(String::as_str));
    let explicit_sections = read_schlib_section_keys(&mut compound).unwrap_or_default();
    let mut records = Vec::with_capacity(names.len());
    for (index, name) in names.into_iter().enumerate() {
        let section_key = resolve_section_key(
            &mut compound,
            &name,
            explicit_sections.get(&name).map(String::as_str),
            &derived_sections[index],
            "Data",
        )?;
        let data = read_stream_bytes(&mut compound, &format!("/{section_key}/Data"))?;
        records.push(parse_schlib_record(name, data)?);
    }
    Ok(records)
}

pub(crate) fn write_schlib_records(records: &[SchlibRecord], output_path: &Path) -> Result<()> {
    if records.is_empty() {
        return Err(AppError::Other(
            "cannot write empty SchLib library".to_string(),
        ));
    }

    let sections = collect_sections(records.iter().map(|record| record.name.as_str()));
    let section_pairs =
        collect_section_key_pairs(records.iter().map(|record| record.name.as_str()), &sections);
    let file = File::create(output_path)?;
    let mut compound = CompoundFile::create(file)?;

    write_stream(
        &mut compound,
        "/FileHeader",
        &schlib_file_header_bytes(records),
    )?;
    if !section_pairs.is_empty() {
        write_stream(
            &mut compound,
            "/SectionKeys",
            &schlib_section_keys_bytes(&section_pairs),
        )?;
    }
    for (record, section_key) in records.iter().zip(sections.iter()) {
        compound.create_storage(format!("/{section_key}/"))?;
        write_stream(&mut compound, &format!("/{section_key}/Data"), &record.data)?;
    }
    write_stream(&mut compound, "/Storage", &schlib_storage_bytes())?;
    compound.flush()?;
    Ok(())
}

pub(crate) fn read_pcblib_records(path: &Path) -> Result<PcblibRecordLibrary> {
    let file = File::open(path)?;
    let mut compound = CompoundFile::open(file)?;
    let names = read_pcblib_component_names(&mut compound)?;
    let derived_sections = collect_sections(names.iter().map(String::as_str));
    let explicit_sections = read_pcblib_section_keys(&mut compound).unwrap_or_default();

    let mut components = Vec::with_capacity(names.len());
    for (index, name) in names.into_iter().enumerate() {
        let section_key = resolve_section_key(
            &mut compound,
            &name,
            explicit_sections.get(&name).map(String::as_str),
            &derived_sections[index],
            "Header",
        )?;
        let primitive_count = read_storage_header(
            &mut compound,
            &format!("/{section_key}/Header"),
            "PcbLib component header",
        )?;
        let parameters = read_stream_bytes(&mut compound, &format!("/{section_key}/Parameters"))?;
        let wide_strings =
            read_stream_bytes(&mut compound, &format!("/{section_key}/WideStrings"))?;
        let data = read_stream_bytes(&mut compound, &format!("/{section_key}/Data"))?;
        let unique_id_primitive_information = read_stream_bytes(
            &mut compound,
            &format!("/{section_key}/UniqueIdPrimitiveInformation/Data"),
        )?;
        let _ = first_param_block_pairs(&parameters, "PcbLib parameters")?;
        components.push(PcblibComponentRecord {
            name,
            primitive_count,
            parameters,
            wide_strings,
            data,
            unique_id_primitive_information,
        });
    }

    let model_count = read_storage_header(
        &mut compound,
        "/Library/Models/Header",
        "PcbLib models header",
    )?;
    let model_entries = read_pcblib_model_entries(&mut compound, model_count.max(0) as usize)?;

    Ok(PcblibRecordLibrary {
        components,
        models: model_entries,
    })
}

pub(crate) fn write_pcblib_records(
    library: &PcblibRecordLibrary,
    output_path: &Path,
) -> Result<()> {
    let file = File::create(output_path)?;
    let mut compound = CompoundFile::create(file)?;
    let sections = collect_sections(library.components.iter().map(|record| record.name.as_str()));
    let section_pairs = collect_section_key_pairs(
        library.components.iter().map(|record| record.name.as_str()),
        &sections,
    );

    write_stream(&mut compound, "/FileHeader", &pcblib_file_header_bytes())?;
    if !section_pairs.is_empty() {
        write_stream(
            &mut compound,
            "/SectionKeys",
            &pcblib_section_keys_bytes(&section_pairs),
        )?;
    }

    compound.create_storage("/Library/")?;
    write_stream(&mut compound, "/Library/Header", &storage_header_bytes(1))?;
    write_stream(
        &mut compound,
        "/Library/Data",
        &pcblib_library_data_bytes(library, output_path),
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
        &pcblib_models_data_bytes(&library.models),
    )?;
    for (index, model) in library.models.iter().enumerate() {
        write_stream(
            &mut compound,
            &format!("/Library/Models/{index}"),
            &model.stream,
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

    for (component, section_key) in library.components.iter().zip(sections.iter()) {
        compound.create_storage(format!("/{section_key}/"))?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/Header"),
            &storage_header_bytes(component.primitive_count),
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/Parameters"),
            &component.parameters,
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/WideStrings"),
            &component.wide_strings,
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/Data"),
            &component.data,
        )?;
        compound.create_storage(format!("/{section_key}/UniqueIdPrimitiveInformation/"))?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/UniqueIdPrimitiveInformation/Header"),
            &storage_header_bytes(component.primitive_count),
        )?;
        write_stream(
            &mut compound,
            &format!("/{section_key}/UniqueIdPrimitiveInformation/Data"),
            &component.unique_id_primitive_information,
        )?;
    }

    compound.flush()?;
    Ok(())
}

pub(crate) fn normalize_lcsc_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let digits = trimmed
        .strip_prefix('C')
        .or_else(|| trimmed.strip_prefix('c'))?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!("C{digits}"))
}

fn parse_schlib_record(name: String, data: Vec<u8>) -> Result<SchlibRecord> {
    let blocks = parse_block_stream(&data, "SchLib component data")?;
    let mut description = String::new();
    let mut header_part_count = 2usize;
    let mut identity = None;

    for block in &blocks {
        if block.flags != 0 {
            continue;
        }
        let pairs = parse_param_pairs(&schlib_cstring_text(block.payload));
        if param_value(&pairs, "RECORD").is_some_and(|value| value == "1") {
            if let Some(value) = param_value(&pairs, "COMPONENTDESCRIPTION") {
                description = value.to_string();
            }
            if let Some(value) =
                param_value(&pairs, "PARTCOUNT").and_then(|value| value.parse().ok())
            {
                header_part_count = value;
            }
        }
        if identity.is_none() {
            identity = extract_schlib_identity(&pairs);
        }
    }

    Ok(SchlibRecord {
        name,
        description,
        header_part_count,
        weight: blocks.len(),
        identity,
        data,
    })
}

fn read_schlib_section_keys(compound: &mut CompoundFile<File>) -> Result<HashMap<String, String>> {
    let data = match read_stream_bytes(compound, "/SectionKeys") {
        Ok(data) => data,
        Err(_) => return Ok(HashMap::new()),
    };
    let pairs = first_schlib_param_block_pairs(&data, "SchLib section keys")?;
    let count = parse_usize_param(&pairs, "KeyCount").unwrap_or(0);
    let mut sections = HashMap::with_capacity(count);
    for index in 0..count {
        let Some(name) = param_value(&pairs, &format!("LibRef{index}")) else {
            continue;
        };
        let Some(section_key) = param_value(&pairs, &format!("SectionKey{index}")) else {
            continue;
        };
        if !name.trim().is_empty() && !section_key.trim().is_empty() {
            sections.insert(name.to_string(), section_key.to_string());
        }
    }
    Ok(sections)
}

fn extract_schlib_identity(pairs: &[(String, String)]) -> Option<String> {
    let name = param_value(pairs, "NAME")?;
    let value = param_value(pairs, "TEXT")?;
    if !matches_param_name(name) {
        return None;
    }
    normalize_lcsc_id(value)
}

fn matches_param_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "npnp_component_id" | "supplier part" | "supplier part number" | "lcsc id"
    )
}

fn read_pcblib_component_names(compound: &mut CompoundFile<File>) -> Result<Vec<String>> {
    let data = read_stream_bytes(compound, "/Library/Data")?;
    let mut offset = 0usize;
    let _ = read_block(&data, &mut offset, "PcbLib library data params")?;
    if offset + 4 > data.len() {
        return Err(AppError::InvalidResponse(
            "invalid PcbLib library data stream".to_string(),
        ));
    }
    let count = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let block = read_block(&data, &mut offset, "PcbLib component name")?;
        names.push(parse_pascal_short_string(block.payload));
    }
    Ok(names)
}

fn read_pcblib_section_keys(compound: &mut CompoundFile<File>) -> Result<HashMap<String, String>> {
    let data = match read_stream_bytes(compound, "/SectionKeys") {
        Ok(data) => data,
        Err(_) => return Ok(HashMap::new()),
    };
    let mut offset = 0usize;
    if offset + 4 > data.len() {
        return Err(AppError::InvalidResponse(
            "invalid PcbLib section keys stream".to_string(),
        ));
    }
    let count = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()).max(0) as usize;
    offset += 4;
    let mut sections = HashMap::with_capacity(count);
    for _ in 0..count {
        let name_block = read_block(&data, &mut offset, "PcbLib section key name")?;
        let key_block = read_block(&data, &mut offset, "PcbLib section key value")?;
        let name = parse_pascal_short_string(name_block.payload);
        let section_key = parse_pascal_short_string(key_block.payload);
        if !name.trim().is_empty() && !section_key.trim().is_empty() {
            sections.insert(name, section_key);
        }
    }
    Ok(sections)
}

fn read_pcblib_model_entries(
    compound: &mut CompoundFile<File>,
    count: usize,
) -> Result<Vec<PcblibModelRecord>> {
    let data = read_stream_bytes(compound, "/Library/Models/Data")?;
    let mut offset = 0usize;
    let mut records = Vec::with_capacity(count);
    for index in 0..count {
        if offset + 4 > data.len() {
            return Err(AppError::InvalidResponse(
                "invalid PcbLib models data stream".to_string(),
            ));
        }
        let len = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        if len < 0 {
            return Err(AppError::InvalidResponse(
                "invalid negative PcbLib model record length".to_string(),
            ));
        }
        let len = len as usize;
        offset += 4;
        if offset + len > data.len() {
            return Err(AppError::InvalidResponse(
                "truncated PcbLib model record".to_string(),
            ));
        }
        let entry = data[offset..offset + len].to_vec();
        offset += len;
        let stream = read_stream_bytes(compound, &format!("/Library/Models/{index}"))?;
        records.push(PcblibModelRecord { entry, stream });
    }
    Ok(records)
}

fn read_storage_header(compound: &mut CompoundFile<File>, path: &str, label: &str) -> Result<i32> {
    let data = read_stream_bytes(compound, path)?;
    if data.len() < 4 {
        return Err(AppError::InvalidResponse(format!("invalid {label} stream")));
    }
    Ok(i32::from_le_bytes(data[..4].try_into().unwrap()))
}

fn read_stream_bytes(compound: &mut CompoundFile<File>, path: &str) -> Result<Vec<u8>> {
    let mut stream = compound.open_stream(path)?;
    let mut data = Vec::new();
    use std::io::Read as _;
    stream.read_to_end(&mut data)?;
    Ok(data)
}

fn resolve_section_key(
    compound: &mut CompoundFile<File>,
    name: &str,
    explicit: Option<&str>,
    derived: &str,
    required_stream: &str,
) -> Result<String> {
    let candidates = section_key_candidates(name, explicit, derived);
    for section_key in &candidates {
        if compound
            .open_stream(format!("/{section_key}/{required_stream}"))
            .is_ok()
        {
            return Ok(section_key.clone());
        }
    }

    let tried = candidates
        .iter()
        .map(|section_key| format!("/{section_key}/{required_stream}"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(AppError::Other(format!(
        "component {name:?} has no {required_stream} stream; tried {tried}"
    )))
}

fn first_param_block_pairs(data: &[u8], label: &str) -> Result<Vec<(String, String)>> {
    let mut offset = 0usize;
    let block = read_block(data, &mut offset, label)?;
    if block.flags != 0 {
        return Err(AppError::InvalidResponse(format!(
            "missing text block in {label}"
        )));
    }
    Ok(parse_param_pairs(&cstring_text(block.payload)))
}

fn first_schlib_param_block_pairs(data: &[u8], label: &str) -> Result<Vec<(String, String)>> {
    let mut offset = 0usize;
    let block = read_block(data, &mut offset, label)?;
    if block.flags != 0 {
        return Err(AppError::InvalidResponse(format!(
            "missing text block in {label}"
        )));
    }
    Ok(parse_param_pairs(&schlib_cstring_text(block.payload)))
}

fn parse_usize_param(pairs: &[(String, String)], key: &str) -> Option<usize> {
    param_value(pairs, key)?.trim().parse().ok()
}

fn param_value<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .rev()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
}

fn parse_param_pairs(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for segment in text.split('|').filter(|segment| !segment.is_empty()) {
        let Some((name, value)) = segment.split_once('=') else {
            continue;
        };
        if let Some(key) = name.strip_prefix("%UTF8%") {
            pairs.push((key.to_string(), decode_utf8_parameter_value(value)));
        } else {
            pairs.push((name.to_string(), value.to_string()));
        }
    }
    pairs
}

fn cstring_text(data: &[u8]) -> String {
    let len = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    let (text, _, _) = WINDOWS_1252.decode(&data[..len]);
    text.into_owned()
}

fn schlib_cstring_text(data: &[u8]) -> String {
    let len = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    let (text, _, _) = WINDOWS_1252.decode(&data[..len]);
    text.into_owned()
}

fn decode_utf8_parameter_value(text: &str) -> String {
    let (bytes, _, _) = WINDOWS_1252.encode(text);
    String::from_utf8_lossy(&bytes).into_owned()
}

#[derive(Debug, Clone, Copy)]
struct Block<'a> {
    flags: u8,
    payload: &'a [u8],
}

fn parse_block_stream<'a>(data: &'a [u8], label: &str) -> Result<Vec<Block<'a>>> {
    let mut offset = 0usize;
    let mut blocks = Vec::new();
    while offset < data.len() {
        blocks.push(read_block(data, &mut offset, label)?);
    }
    Ok(blocks)
}

fn read_block<'a>(data: &'a [u8], offset: &mut usize, label: &str) -> Result<Block<'a>> {
    if *offset + 4 > data.len() {
        return Err(AppError::InvalidResponse(format!(
            "truncated {label} block header"
        )));
    }
    let header = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
    *offset += 4;
    let flags = (header >> 24) as u8;
    let len = (header & 0x00FF_FFFF) as usize;
    if *offset + len > data.len() {
        return Err(AppError::InvalidResponse(format!(
            "truncated {label} block payload"
        )));
    }
    let payload = &data[*offset..*offset + len];
    *offset += len;
    Ok(Block { flags, payload })
}

fn parse_pascal_short_string(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }
    let len = data[0] as usize;
    let end = (1 + len).min(data.len());
    decode_single_byte_text(&data[1..end])
}

fn decode_single_byte_text(data: &[u8]) -> String {
    data.iter().map(|byte| char::from(*byte)).collect()
}

fn temp_path(prefix: &str, extension: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}_{timestamp}.{extension}"))
}

fn collect_sections<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut used = HashSet::new();
    names
        .map(|name| unique_section_key(name, &mut used))
        .collect()
}

fn collect_section_key_pairs<'a>(
    names: impl Iterator<Item = &'a str>,
    sections: &[String],
) -> Vec<(String, String)> {
    names
        .zip(sections.iter())
        .filter(|&(name, section_key)| section_key.as_str() != name)
        .map(|(name, section_key)| (name.to_string(), section_key.clone()))
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

fn section_key_from_name(name: &str) -> String {
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

fn legacy_section_key_from_name(name: &str) -> String {
    if name.is_empty() {
        return "_".to_string();
    }
    name.chars()
        .take(31)
        .map(|character| if character == '/' { '_' } else { character })
        .collect()
}

fn section_key_candidates(name: &str, explicit: Option<&str>, derived: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(explicit) = explicit.filter(|value| !value.trim().is_empty()) {
        push_unique_section_key(&mut candidates, explicit.to_string());
    }
    push_unique_section_key(&mut candidates, derived.to_string());
    push_unique_section_key(&mut candidates, legacy_section_key_from_name(name));
    push_unique_section_key(&mut candidates, name.to_string());
    candidates
}

fn push_unique_section_key(candidates: &mut Vec<String>, value: String) {
    if value.is_empty() || candidates.iter().any(|existing| existing == &value) {
        return;
    }
    candidates.push(value);
}

fn write_stream(compound: &mut CompoundFile<File>, path: &str, data: &[u8]) -> std::io::Result<()> {
    let mut stream = compound.create_stream(path)?;
    stream.write_all(data)
}

fn schlib_file_header_bytes(records: &[SchlibRecord]) -> Vec<u8> {
    let mut writer = SchWriter::default();
    let mut params = SchParams::default();
    params.push(
        "HEADER",
        "Protel for Windows - Schematic Library Editor Binary File Version 5.0",
    );
    params.push(
        "WEIGHT",
        records
            .iter()
            .map(|record| record.weight)
            .sum::<usize>()
            .to_string(),
    );
    params.push("MINORVERSION", "2");
    params.push("FONTIDCOUNT", "1");
    params.push("SIZE1", "10");
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
    params.push("COMPCOUNT", records.len().to_string());
    for (index, record) in records.iter().enumerate() {
        params.push(format!("LIBREF{index}"), &record.name);
        params.push(format!("COMPDESCR{index}"), &record.description);
        params.push(
            format!("PARTCOUNT{index}"),
            record.header_part_count.to_string(),
        );
    }
    writer.write_cstring_param_block(&params);
    writer.write_i32(records.len() as i32);
    for record in records {
        writer.write_string_block(&record.name);
    }
    writer.into_inner()
}

fn schlib_section_keys_bytes(pairs: &[(String, String)]) -> Vec<u8> {
    let mut writer = SchWriter::default();
    let mut params = SchParams::default();
    params.push("KeyCount", pairs.len().to_string());
    for (index, (name, key)) in pairs.iter().enumerate() {
        params.push(format!("LibRef{index}"), name);
        params.push(format!("SectionKey{index}"), key);
    }
    writer.write_cstring_param_block(&params);
    writer.into_inner()
}

fn schlib_storage_bytes() -> Vec<u8> {
    let mut writer = SchWriter::default();
    let mut params = SchParams::default();
    params.push("HEADER", "Icon storage");
    writer.write_cstring_param_block(&params);
    writer.into_inner()
}

fn pcblib_file_header_bytes() -> Vec<u8> {
    let mut writer = PcbWriter::default();
    let version = "PCB 6.0 Binary Library File";
    writer.write_i32(version.len() as i32);
    writer.write_pascal_short_string(version);
    writer.into_inner()
}

fn pcblib_section_keys_bytes(pairs: &[(String, String)]) -> Vec<u8> {
    let mut writer = PcbWriter::default();
    writer.write_i32(pairs.len() as i32);
    for (name, key) in pairs {
        writer.write_pascal_string(name);
        writer.write_string_block(key);
    }
    writer.into_inner()
}

fn storage_header_bytes(record_count: i32) -> Vec<u8> {
    record_count.to_le_bytes().to_vec()
}

fn pcblib_library_data_bytes(library: &PcblibRecordLibrary, output_path: &Path) -> Vec<u8> {
    let mut writer = PcbWriter::default();
    writer.write_block(0, |inner| {
        inner.write_cstring(&pcblib_library_data_params(output_path))
    });
    writer.write_u32(library.components.len() as u32);
    for component in &library.components {
        writer.write_string_block(&component.name);
    }
    writer.into_inner()
}

fn pcblib_library_data_params(output_path: &Path) -> String {
    let mut filename = output_path
        .canonicalize()
        .unwrap_or_else(|_| output_path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\");
    if let Some(stripped) = filename.strip_prefix("\\\\?\\") {
        filename = stripped.to_string();
    }
    let (date_text, time_text) = current_library_date_time();
    PCBLIB_LIBRARY_DATA_TEMPLATE
        .replace("__FILE__", &filename)
        .replace("__DATE__", &date_text)
        .replace("__TIME__", &time_text)
}

fn current_library_date_time() -> (String, String) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

fn pcblib_models_data_bytes(models: &[PcblibModelRecord]) -> Vec<u8> {
    let mut writer = PcbWriter::default();
    for model in models {
        writer.write_i32(model.entry.len() as i32);
        writer.write_bytes(&model.entry);
    }
    writer.into_inner()
}

#[derive(Debug, Default)]
struct SchParams(Vec<(String, String)>);

impl SchParams {
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
struct SchWriter {
    data: Vec<u8>,
}

impl SchWriter {
    fn into_inner(self) -> Vec<u8> {
        self.data
    }

    fn write_u8(&mut self, value: u8) {
        self.data.push(value);
    }

    fn write_i32(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn write_pascal_short_string(&mut self, value: &str) {
        let bytes = sch_encode_ansi_lossy(value);
        let len = bytes.len().min(255);
        self.write_u8(len as u8);
        self.data.extend_from_slice(&bytes[..len]);
    }

    fn write_cstring(&mut self, value: &str) {
        self.data.extend_from_slice(&sch_encode_ansi_lossy(value));
        self.write_u8(0);
    }

    fn write_block(&mut self, flags: u8, serializer: impl FnOnce(&mut Self)) {
        let mut child = Self::default();
        serializer(&mut child);
        let child_data = child.into_inner();
        self.write_u32(((flags as u32) << 24) | child_data.len() as u32);
        self.data.extend_from_slice(&child_data);
    }

    fn write_string_block(&mut self, value: &str) {
        self.write_block(0, |writer| writer.write_pascal_short_string(value));
    }

    fn write_cstring_param_block(&mut self, params: &SchParams) {
        let text = params.as_string();
        self.write_block(0, |writer| writer.write_cstring(&text));
    }
}

fn sch_encode_ansi_lossy(text: &str) -> Vec<u8> {
    let sanitized = text.replace('\0', "?");
    let (bytes, _, _) = WINDOWS_1252.encode(&sanitized);
    bytes.into_owned()
}

#[derive(Debug, Default)]
struct PcbWriter {
    data: Vec<u8>,
}

impl PcbWriter {
    fn into_inner(self) -> Vec<u8> {
        self.data
    }

    fn write_u8(&mut self, value: u8) {
        self.data.push(value);
    }

    fn write_i32(&mut self, value: i32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.data.extend_from_slice(&value.to_le_bytes());
    }

    fn write_bytes(&mut self, value: &[u8]) {
        self.data.extend_from_slice(value);
    }

    fn write_block(&mut self, flags: u8, serializer: impl FnOnce(&mut Self)) {
        let mut child = Self::default();
        serializer(&mut child);
        let child_data = child.into_inner();
        self.write_u32(((flags as u32) << 24) | child_data.len() as u32);
        self.write_bytes(&child_data);
    }

    fn write_pascal_short_string(&mut self, value: &str) {
        let bytes = pcb_encode_ansi_lossy(value);
        let len = bytes.len().min(255);
        self.write_u8(len as u8);
        self.write_bytes(&bytes[..len]);
    }

    fn write_cstring(&mut self, value: &str) {
        self.write_bytes(&pcb_encode_ansi_lossy(value));
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
}

fn pcb_encode_ansi_lossy(text: &str) -> Vec<u8> {
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
    use super::{
        normalize_lcsc_id, pcblib_records_from_library, read_pcblib_records, read_schlib_records,
        schlib_file_header_bytes, schlib_record_from_component, schlib_storage_bytes, temp_path,
        write_pcblib_records, write_schlib_records, write_stream,
    };
    use crate::altium::footprint::build_pcblib_from_payload;
    use crate::altium::schlib::{
        SchlibMetadata, SchlibParameter, build_component_from_payload_with_metadata,
    };
    use serde_json::Value;
    use std::fs::{self, File};

    const STEP_FIXTURE: &[u8] =
        b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n";

    #[test]
    fn normalizes_lcsc_ids_case_insensitively() {
        assert_eq!(normalize_lcsc_id(" c2040 ").as_deref(), Some("C2040"));
        assert!(normalize_lcsc_id("RP2040").is_none());
    }

    #[test]
    fn schlib_records_round_trip_component_identity() {
        let payload: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/easyeda_symbol.json"))
                .expect("symbol fixture");
        let make_component = |name: &str, component_id: &str| {
            let metadata = SchlibMetadata {
                description: Some("Roundtrip component".to_string()),
                designator: Some("U?".to_string()),
                comment: Some(name.to_string()),
                parameters: vec![SchlibParameter {
                    name: "NPNP_COMPONENT_ID".to_string(),
                    value: component_id.to_string(),
                }],
                footprint_model_name: None,
                footprint_library_file: None,
            };
            build_component_from_payload_with_metadata(&payload, name, &metadata)
                .expect("build component")
        };

        let record_a = schlib_record_from_component(&make_component("COMP_A", "C2040"))
            .expect("capture SchLib record");
        let record_b = schlib_record_from_component(&make_component("COMP_B", "C42"))
            .expect("capture SchLib record");
        assert_eq!(record_a.identity.as_deref(), Some("C2040"));
        assert_eq!(record_b.identity.as_deref(), Some("C42"));

        let path = temp_path("npnp_merge_schlib_roundtrip", "SchLib");
        write_schlib_records(&[record_a, record_b], &path).expect("write merged SchLib");
        let records = read_schlib_records(&path).expect("read merged SchLib");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "COMP_A");
        assert_eq!(records[0].identity.as_deref(), Some("C2040"));
        assert_eq!(records[1].name, "COMP_B");
        assert_eq!(records[1].identity.as_deref(), Some("C42"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn schlib_records_round_trip_non_ascii_component_name() {
        let payload: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/easyeda_symbol.json"))
                .expect("symbol fixture");
        let non_ascii_name = "SMD7525-32\u{03A9}";
        let metadata = SchlibMetadata {
            description: Some("Non ASCII component".to_string()),
            designator: Some("BUZZER?".to_string()),
            comment: Some(non_ascii_name.to_string()),
            parameters: vec![SchlibParameter {
                name: "NPNP_COMPONENT_ID".to_string(),
                value: "C50387083".to_string(),
            }],
            footprint_model_name: None,
            footprint_library_file: None,
        };
        let ascii_metadata = SchlibMetadata {
            description: Some("ASCII component".to_string()),
            designator: Some("U?".to_string()),
            comment: Some("COMP_A".to_string()),
            parameters: vec![SchlibParameter {
                name: "NPNP_COMPONENT_ID".to_string(),
                value: "C2040".to_string(),
            }],
            footprint_model_name: None,
            footprint_library_file: None,
        };
        let ascii_component =
            build_component_from_payload_with_metadata(&payload, "COMP_A", &ascii_metadata)
                .expect("build ASCII component");
        let component =
            build_component_from_payload_with_metadata(&payload, non_ascii_name, &metadata)
                .expect("build component");

        let record = schlib_record_from_component(&component).expect("capture SchLib record");
        assert_eq!(record.name, non_ascii_name);
        assert_eq!(record.identity.as_deref(), Some("C50387083"));
        let ascii_record =
            schlib_record_from_component(&ascii_component).expect("capture ASCII SchLib record");

        let path = temp_path("npnp_merge_schlib_non_ascii", "SchLib");
        write_schlib_records(&[ascii_record, record], &path).expect("write merged SchLib");
        let records = read_schlib_records(&path).expect("read merged SchLib");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "COMP_A");
        assert_eq!(records[0].identity.as_deref(), Some("C2040"));
        assert_eq!(records[1].name, non_ascii_name);
        assert_eq!(records[1].identity.as_deref(), Some("C50387083"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn schlib_records_read_legacy_space_section_name_without_section_keys() {
        let payload: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/easyeda_symbol.json"))
                .expect("symbol fixture");
        let name = "0.5-8P CTSJ-H2.0 119";
        let metadata = SchlibMetadata {
            description: Some("Legacy space section".to_string()),
            designator: Some("J?".to_string()),
            comment: Some(name.to_string()),
            parameters: vec![SchlibParameter {
                name: "NPNP_COMPONENT_ID".to_string(),
                value: "C424242".to_string(),
            }],
            footprint_model_name: None,
            footprint_library_file: None,
        };
        let component = build_component_from_payload_with_metadata(&payload, name, &metadata)
            .expect("build component");
        let record = schlib_record_from_component(&component).expect("capture SchLib record");

        let path = temp_path("npnp_merge_schlib_legacy_space", "SchLib");
        let file = File::create(&path).expect("create legacy SchLib");
        let mut compound = cfb::CompoundFile::create(file).expect("create compound");
        write_stream(
            &mut compound,
            "/FileHeader",
            &schlib_file_header_bytes(std::slice::from_ref(&record)),
        )
        .expect("write header");
        compound
            .create_storage(format!("/{name}/"))
            .expect("create legacy storage");
        write_stream(&mut compound, &format!("/{name}/Data"), &record.data).expect("write data");
        write_stream(&mut compound, "/Storage", &schlib_storage_bytes()).expect("write storage");
        compound.flush().expect("flush compound");
        drop(compound);

        let records = read_schlib_records(&path).expect("read legacy SchLib");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, name);
        assert_eq!(records[0].identity.as_deref(), Some("C424242"));
        fs::remove_file(path).ok();
    }

    #[test]
    fn pcblib_records_round_trip_component_streams() {
        let payload: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/easyeda_footprint.json"))
                .expect("footprint fixture");
        let library =
            build_pcblib_from_payload(&payload, "ROUNDTRIP_FOOTPRINT", Some(STEP_FIXTURE))
                .expect("build footprint library");
        let records = pcblib_records_from_library(&library).expect("capture PcbLib records");
        assert_eq!(records.components.len(), 1);
        assert_eq!(records.models.len(), 1);

        let path = temp_path("npnp_merge_pcblib_roundtrip", "PcbLib");
        write_pcblib_records(&records, &path).expect("write merged PcbLib");
        let round_trip = read_pcblib_records(&path).expect("read merged PcbLib");
        assert_eq!(round_trip.components.len(), 1);
        assert_eq!(round_trip.components[0].name, "ROUNDTRIP_FOOTPRINT");
        assert_eq!(round_trip.models.len(), 1);
        fs::remove_file(path).ok();
    }
}
