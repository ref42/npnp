use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, to_string_pretty};

use crate::altium::footprint::build_pcblib_from_payload;
use crate::altium::pcblib::{PcbLibrary, write_pcblib};
use crate::altium::schlib::{
    Component, SchlibMetadata, SchlibParameter, build_component_from_payload_with_metadata,
    write_schlib,
};
use crate::error::{AppError, Result};
use crate::lceda::{LcedaClient, SearchItem};
use crate::lcsc::{LcscClient, LcscProduct};
use crate::metadata::ResolvedMetadata;
use crate::util::{component_identity_name, nested_string, sanitize_filename, split_obj_and_mtl};

pub async fn download_step(
    client: &LcedaClient,
    item: &SearchItem,
    out_dir: &Path,
    force: bool,
) -> Result<PathBuf> {
    fs::create_dir_all(out_dir)?;
    let out_file = out_dir.join(item.choose_step_filename());
    if out_file.exists() && !force {
        return Ok(out_file);
    }

    let model_uuid = client.get_model_uuid(item).await?;
    let content = client.download_step_bytes(&model_uuid).await?;
    // The STEP is stored in LCEDA's own frame, which does not always rest on the board plane.
    fs::write(&out_file, crate::step_model::place_step_on_board(&content))?;
    Ok(out_file)
}

pub async fn download_obj(
    client: &LcedaClient,
    item: &SearchItem,
    out_dir: &Path,
    force: bool,
) -> Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(out_dir)?;
    let base_name = item.choose_obj_basename();
    let obj_file = out_dir.join(format!("{base_name}.obj"));
    let mtl_file = out_dir.join(format!("{base_name}.mtl"));

    if obj_file.exists() && mtl_file.exists() && !force {
        return Ok((obj_file, mtl_file));
    }

    let model_uuid = client.get_model_uuid(item).await?;
    let content = client.download_obj_bytes(&model_uuid).await?;
    let text = String::from_utf8_lossy(&content);
    let (obj_text, mtl_text) = split_obj_and_mtl(&text);
    let obj_with_header = format!("mtllib {base_name}.mtl\n{obj_text}");

    fs::write(&obj_file, obj_with_header)?;
    fs::write(&mtl_file, mtl_text)?;
    Ok((obj_file, mtl_file))
}

pub async fn export_easyeda_sources(
    client: &LcedaClient,
    item: &SearchItem,
    out_dir: &Path,
    force: bool,
) -> Result<BTreeMap<String, PathBuf>> {
    fs::create_dir_all(out_dir)?;

    let base = sanitize_filename(item.display_name());
    let symbol_uuid = item.symbol_uuid();
    let footprint_uuid = item.footprint_uuid();
    if symbol_uuid.is_none() && footprint_uuid.is_none() {
        return Err(AppError::MissingSymbolOrFootprint);
    }

    let mut exported = BTreeMap::new();

    if let Some(symbol_uuid) = symbol_uuid {
        let symbol_data = client.component_detail(&symbol_uuid).await?;
        let symbol_file = out_dir.join(format!("{base}_symbol_easyeda.json"));
        if force || !symbol_file.exists() {
            fs::write(&symbol_file, to_string_pretty(&symbol_data)?)?;
        }
        exported.insert("symbol".to_string(), symbol_file);
    }

    if let Some(footprint_uuid) = footprint_uuid {
        let footprint_data = client.component_detail(&footprint_uuid).await?;
        let footprint_file = out_dir.join(format!("{base}_footprint_easyeda.json"));
        if force || !footprint_file.exists() {
            fs::write(&footprint_file, to_string_pretty(&footprint_data)?)?;
        }
        exported.insert("footprint".to_string(), footprint_file);
    }

    Ok(exported)
}

fn resolved_symbol_component_name(item: &SearchItem, payload: &Value) -> String {
    first_non_empty([
        (!item.display_name().trim().is_empty()).then(|| item.display_name().to_string()),
        nested_string(payload, &["result", "display_title"]),
        nested_string(payload, &["display_title"]),
        nested_string(payload, &["result", "title"]),
        nested_string(payload, &["title"]),
    ])
    .unwrap_or_else(|| "component".to_string())
}

pub(crate) fn resolved_footprint_name(item: &SearchItem, payload: &Value) -> String {
    first_non_empty([
        nested_string(payload, &["result", "display_title"]),
        nested_string(payload, &["display_title"]),
        nested_string(payload, &["result", "package"]),
        nested_string(payload, &["package"]),
        nested_string(&item.raw, &["footprint", "display_title"]),
        nested_string(&item.raw, &["attributes", "Supplier Footprint"]),
        nested_string(&item.raw, &["attributes", "Footprint"]),
        (!item.display_name().trim().is_empty()).then(|| item.display_name().to_string()),
    ])
    .unwrap_or_else(|| "footprint".to_string())
}

async fn load_step_bytes_for_pcblib(
    client: &LcedaClient,
    item: &SearchItem,
    footprint_data: &Value,
) -> Option<Vec<u8>> {
    let mut model_candidates = Vec::new();
    if let Some(model_uuid) = nested_string(footprint_data, &["result", "model_3d", "uri"])
        .filter(|uuid| !uuid.trim().is_empty())
    {
        model_candidates.push(model_uuid);
    }
    if let Some(model_uuid) = item
        .model_uuid
        .clone()
        .filter(|uuid| !uuid.trim().is_empty())
        && !model_candidates
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&model_uuid))
    {
        model_candidates.push(model_uuid);
    }
    if item.model_uuid.is_some()
        && let Ok(model_uuid) = client.get_model_uuid(item).await
        && !model_candidates
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&model_uuid))
    {
        model_candidates.push(model_uuid);
    }

    for model_uuid in model_candidates {
        if let Ok(bytes) = client.download_step_bytes(&model_uuid).await {
            return Some(bytes);
        }
    }

    None
}

pub(crate) async fn build_pcblib_library_from_detail(
    client: &LcedaClient,
    item: &SearchItem,
    footprint_data: &Value,
    component_name: &str,
) -> Result<PcbLibrary> {
    let step_bytes = load_step_bytes_for_pcblib(client, item, footprint_data).await;
    build_pcblib_from_payload(footprint_data, component_name, step_bytes.as_deref())
}

pub(crate) fn build_schlib_component_from_detail(
    item: &SearchItem,
    symbol_data: &Value,
    component_name: &str,
    footprint_model_name: Option<&str>,
    footprint_library_file: Option<&str>,
    english_metadata: Option<&LcscProduct>,
) -> Result<Component> {
    let metadata = build_schlib_metadata(
        item,
        symbol_data,
        footprint_model_name,
        footprint_library_file,
        english_metadata,
    );
    build_component_from_payload_with_metadata(symbol_data, component_name, &metadata)
}

pub async fn build_pcblib_library_for_item(
    client: &LcedaClient,
    item: &SearchItem,
    footprint_name: &str,
) -> Result<PcbLibrary> {
    let footprint_uuid = item
        .footprint_uuid()
        .ok_or(AppError::MissingSymbolOrFootprint)?;
    let footprint_data = client.component_detail(&footprint_uuid).await?;
    build_pcblib_library_from_detail(client, item, &footprint_data, footprint_name).await
}

pub async fn build_schlib_component_for_item(
    client: &LcedaClient,
    item: &SearchItem,
    component_name: &str,
    footprint_model_name: Option<&str>,
    footprint_library_file: Option<&str>,
) -> Result<Component> {
    build_schlib_component_for_item_with_metadata(
        client,
        item,
        component_name,
        footprint_model_name,
        footprint_library_file,
        None,
    )
    .await
}

pub async fn build_schlib_component_for_item_with_metadata(
    client: &LcedaClient,
    item: &SearchItem,
    component_name: &str,
    footprint_model_name: Option<&str>,
    footprint_library_file: Option<&str>,
    english_metadata: Option<&LcscProduct>,
) -> Result<Component> {
    let symbol_uuid = item
        .symbol_uuid()
        .ok_or(AppError::MissingSymbolOrFootprint)?;
    let symbol_data = client.component_detail(&symbol_uuid).await?;
    build_schlib_component_from_detail(
        item,
        &symbol_data,
        component_name,
        footprint_model_name,
        footprint_library_file,
        english_metadata,
    )
}
pub async fn export_pcblib(
    client: &LcedaClient,
    item: &SearchItem,
    out_dir: &Path,
    force: bool,
) -> Result<PathBuf> {
    fs::create_dir_all(out_dir)?;
    let footprint_uuid = item
        .footprint_uuid()
        .ok_or(AppError::MissingSymbolOrFootprint)?;
    let footprint_data = client.component_detail(&footprint_uuid).await?;
    let footprint_name = component_identity_name(
        &resolved_footprint_name(item, &footprint_data),
        &item.lcsc_id().unwrap_or_default(),
    );
    let out_file = out_dir.join(format!("{}.PcbLib", sanitize_filename(&footprint_name)));
    if out_file.exists() && !force {
        return Ok(out_file);
    }

    let library =
        build_pcblib_library_from_detail(client, item, &footprint_data, &footprint_name).await?;
    write_pcblib(&library, &out_file)?;
    Ok(out_file)
}

pub async fn export_schlib(
    client: &LcedaClient,
    item: &SearchItem,
    out_dir: &Path,
    force: bool,
) -> Result<PathBuf> {
    export_schlib_with_options(client, item, out_dir, force, false).await
}

pub async fn export_schlib_with_options(
    client: &LcedaClient,
    item: &SearchItem,
    out_dir: &Path,
    force: bool,
    english_metadata: bool,
) -> Result<PathBuf> {
    fs::create_dir_all(out_dir)?;
    let symbol_uuid = item
        .symbol_uuid()
        .ok_or(AppError::MissingSymbolOrFootprint)?;
    let symbol_data = client.component_detail(&symbol_uuid).await?;
    let component_name = component_identity_name(
        &resolved_symbol_component_name(item, &symbol_data),
        &item.lcsc_id().unwrap_or_default(),
    );
    let out_file = out_dir.join(format!("{}.SchLib", sanitize_filename(&component_name)));
    if out_file.exists() && !force {
        return Ok(out_file);
    }

    let (footprint_model_name, footprint_library_file) =
        if let Some(footprint_uuid) = item.footprint_uuid() {
            let footprint_data = client.component_detail(&footprint_uuid).await?;
            let footprint_name = component_identity_name(
                &resolved_footprint_name(item, &footprint_data),
                &item.lcsc_id().unwrap_or_default(),
            );
            (
                Some(footprint_name.clone()),
                Some(format!("{}.PcbLib", sanitize_filename(&footprint_name))),
            )
        } else {
            (None, None)
        };
    let english_metadata = if english_metadata {
        if let Some(lcsc_id) = item.lcsc_id() {
            LcscClient::new().product_detail_optional(&lcsc_id).await
        } else {
            None
        }
    } else {
        None
    };

    let component = build_schlib_component_from_detail(
        item,
        &symbol_data,
        &component_name,
        footprint_model_name.as_deref(),
        footprint_library_file.as_deref(),
        english_metadata.as_ref(),
    )?;
    write_schlib(&component, &out_file)?;
    Ok(out_file)
}

fn build_schlib_metadata(
    item: &SearchItem,
    symbol_data: &Value,
    footprint_model_name: Option<&str>,
    footprint_library_file: Option<&str>,
    english_metadata: Option<&LcscProduct>,
) -> SchlibMetadata {
    let resolved = ResolvedMetadata::from_lceda_search_item(
        item,
        symbol_data,
        footprint_model_name,
        english_metadata,
    );
    SchlibMetadata {
        description: resolved.description,
        designator: resolved.designator,
        comment: resolved.comment,
        parameters: resolved
            .parameters
            .into_iter()
            .map(|parameter| SchlibParameter {
                name: parameter.name,
                value: parameter.value,
            })
            .collect(),
        footprint_model_name: resolved.footprint,
        footprint_library_file: footprint_library_file
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}

fn first_non_empty<const N: usize>(candidates: [Option<String>; N]) -> Option<String> {
    candidates
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_manufacturer_part_formula_comment() {
        let item = SearchItem {
            index: 0,
            display_title: "RP2040".to_string(),
            title: String::new(),
            manufacturer: "Raspberry Pi".to_string(),
            model_uuid: None,
            raw: json!({
                "attributes": {
                    "Name": "={Manufacturer Part}",
                    "Manufacturer Part": "RP2040"
                }
            }),
        };

        let metadata = build_schlib_metadata(&item, &json!({}), None, None, None);
        assert_eq!(metadata.comment.as_deref(), Some("RP2040"));
    }

    #[test]
    fn resolves_value_formula_comment() {
        let item = SearchItem {
            index: 0,
            display_title: "TMB12A05".to_string(),
            title: String::new(),
            manufacturer: "XUNPU".to_string(),
            model_uuid: None,
            raw: json!({
                "attributes": {
                    "Name": "={Value}",
                    "Value": "2.4kHz",
                    "Manufacturer Part": "TMB12A05"
                }
            }),
        };

        let metadata = build_schlib_metadata(&item, &json!({}), None, None, None);
        assert_eq!(metadata.comment.as_deref(), Some("2.4kHz"));
    }

    #[test]
    fn falls_back_when_formula_cannot_be_resolved() {
        let item = SearchItem {
            index: 0,
            display_title: "XC7Z020-2CLG400I".to_string(),
            title: String::new(),
            manufacturer: "AMD".to_string(),
            model_uuid: None,
            raw: json!({
                "attributes": {
                    "Name": "={Missing Field}",
                    "Manufacturer Part": "XC7Z020-2CLG400I"
                }
            }),
        };

        let metadata = build_schlib_metadata(&item, &json!({}), None, None, None);
        assert_eq!(metadata.comment.as_deref(), Some("XC7Z020-2CLG400I"));
    }

    #[test]
    fn resolves_footprint_name_from_payload_metadata() {
        let item = SearchItem {
            index: 0,
            display_title: "STM8S003F3U6TR".to_string(),
            title: String::new(),
            manufacturer: "ST".to_string(),
            model_uuid: None,
            raw: json!({
                "footprint": {
                    "display_title": "UFQFPN-20_L3.0-W3.0-P0.50-TL"
                }
            }),
        };
        let payload = json!({
            "result": {
                "display_title": "UFQFPN-20_L3.0-W3.0-P0.50-TL",
                "package": "UFQFPN-20"
            }
        });

        assert_eq!(
            resolved_footprint_name(&item, &payload),
            "UFQFPN-20_L3.0-W3.0-P0.50-TL"
        );
    }
}
