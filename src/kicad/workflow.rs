use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::easyeda::{ComponentData, EasyedaApi};
use crate::error::{AppError, EasyedaError, Result};
use crate::kicad::{
    KicadExportOptions, KicadLibraryOptions, LibraryManager, ModelStageStatus, convert_3d_model,
    convert_footprint, convert_symbol,
};
use crate::lcsc::LcscClient;
use crate::metadata::ResolvedMetadata;

#[derive(Debug, Clone)]
pub struct KicadSingleOptions {
    pub lcsc_id: String,
    pub library: KicadLibraryOptions,
    pub export: KicadExportOptions,
    pub english_metadata: bool,
}

#[derive(Debug, Clone)]
pub struct KicadBatchOptions {
    pub ids: Vec<String>,
    pub library: KicadLibraryOptions,
    pub export: KicadExportOptions,
    pub english_metadata: bool,
    pub parallel: usize,
    pub continue_on_error: bool,
}

#[derive(Debug, Clone)]
pub struct KicadBatchSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub failed_ids: Vec<String>,
    pub failure_details: Vec<String>,
    pub generated_files: Vec<PathBuf>,
    pub output: PathBuf,
}

#[derive(Debug, Clone)]
pub struct KicadExportedComponent {
    pub lcsc_id: String,
    pub component_name: String,
}

pub async fn export_kicad_single(options: KicadSingleOptions) -> Result<KicadBatchSummary> {
    let batch_options = KicadBatchOptions {
        ids: vec![options.lcsc_id],
        library: options.library,
        export: options.export,
        english_metadata: options.english_metadata,
        parallel: 1,
        continue_on_error: false,
    };
    export_kicad_batch(batch_options).await
}

pub async fn export_kicad_batch(options: KicadBatchOptions) -> Result<KicadBatchSummary> {
    validate_batch_options(&options)?;

    let api = Arc::new(EasyedaApi::new());
    let lcsc_client = options.english_metadata.then(LcscClient::new).map(Arc::new);
    let manager = Arc::new(LibraryManager::from_options(&options.library)?);
    manager.create_directories()?;

    let model_status = Arc::new(ModelStageStatus::new());
    let export_options = Arc::new(options.export.clone());
    let ids = options.ids.clone();
    let mut summary = KicadBatchSummary {
        total: ids.len(),
        success: 0,
        failed: 0,
        failed_ids: Vec::new(),
        failure_details: Vec::new(),
        generated_files: generated_files_for(&manager, &options.export),
        output: options.library.output.clone(),
    };

    let parallel = options.parallel.max(1).min(ids.len().max(1));
    let semaphore = Arc::new(Semaphore::new(parallel));
    let mut join_set = JoinSet::new();

    for id in ids {
        let api = api.clone();
        let lcsc_client = lcsc_client.clone();
        let manager = manager.clone();
        let model_status = model_status.clone();
        let export_options = export_options.clone();
        let semaphore = semaphore.clone();
        join_set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("KiCad batch semaphore should remain open");
            let result = export_one_kicad_component(
                &api,
                lcsc_client.as_deref(),
                &manager,
                &export_options,
                &model_status,
                &id,
            )
            .await;
            (id, result)
        });
    }

    let mut first_error = None;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((_, Ok(_exported))) => {
                summary.success += 1;
            }
            Ok((id, Err(err))) => {
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                summary.failure_details.push(format!("{id}: {err}"));
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(err) => {
                summary.failed += 1;
                let batch_err = AppError::Other(format!("KiCad batch task join failed: {err}"));
                if first_error.is_none() {
                    first_error = Some(batch_err);
                }
            }
        }
    }

    if summary.failed > 0 && !options.continue_on_error {
        return Err(first_error.unwrap_or_else(|| AppError::Other("KiCad export failed".into())));
    }
    if summary.success == 0 {
        return Err(first_error.unwrap_or_else(|| {
            AppError::Other("no KiCad components exported successfully".to_string())
        }));
    }

    Ok(summary)
}

async fn export_one_kicad_component(
    api: &EasyedaApi,
    lcsc_client: Option<&LcscClient>,
    manager: &LibraryManager,
    options: &KicadExportOptions,
    model_status: &ModelStageStatus,
    lcsc_id: &str,
) -> Result<KicadExportedComponent> {
    validate_lcsc_id(lcsc_id)?;
    let mut component_data = api.get_component_data(lcsc_id).await?;
    apply_requested_metadata(&mut component_data, lcsc_client).await;

    if options.targets_symbol() {
        convert_symbol(options, &component_data, manager, lcsc_id)?;
    }
    if options.targets_footprint() {
        convert_footprint(options, &component_data, manager, lcsc_id)?;
    }
    if options.targets_model_3d() {
        convert_3d_model(
            options,
            api,
            &component_data,
            manager,
            lcsc_id,
            model_status,
        )
        .await?;
    }

    Ok(KicadExportedComponent {
        lcsc_id: lcsc_id.to_string(),
        component_name: component_data.title,
    })
}

async fn apply_requested_metadata(component_data: &mut ComponentData, client: Option<&LcscClient>) {
    let product = if let Some(client) = client {
        client
            .product_detail_optional(&component_data.lcsc_id)
            .await
    } else {
        None
    };
    let metadata = ResolvedMetadata::from_component_data(component_data, product.as_ref());
    metadata.apply_to_component_data(component_data);
}

fn validate_batch_options(options: &KicadBatchOptions) -> Result<()> {
    if options.ids.is_empty() {
        return Err(AppError::Other(
            "no valid LCSC IDs found for KiCad export".to_string(),
        ));
    }
    if options.parallel == 0 {
        return Err(AppError::Other("--parallel must be at least 1".to_string()));
    }
    validate_targets(&options.export)?;
    validate_library_options(&options.library)
}

fn validate_targets(options: &KicadExportOptions) -> Result<()> {
    if !options.symbol && !options.footprint && !options.model_3d && !options.full {
        return Err(AppError::Other(
            "at least one KiCad export target must be selected (--symbol, --footprint, --3d, or --full)"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_library_options(options: &KicadLibraryOptions) -> Result<()> {
    if let Some(library_name) = &options.library_name
        && library_name.trim().is_empty()
    {
        return Err(AppError::Other(
            "--library-name must not be empty".to_string(),
        ));
    }
    if let Some(symbol_lib) = &options.symbol_lib
        && !path_ends_with(symbol_lib, ".kicad_sym")
    {
        return Err(AppError::Other(
            "--symbol-lib must point to a .kicad_sym file".to_string(),
        ));
    }
    if let Some(footprint_lib) = &options.footprint_lib
        && !path_ends_with(footprint_lib, ".pretty")
    {
        return Err(AppError::Other(
            "--footprint-lib must point to a .pretty directory".to_string(),
        ));
    }
    if let Some(model_lib) = &options.model_lib
        && !path_ends_with(model_lib, ".3dshapes")
    {
        return Err(AppError::Other(
            "--model-lib must point to a .3dshapes directory".to_string(),
        ));
    }
    Ok(())
}

fn validate_lcsc_id(id: &str) -> Result<()> {
    let trimmed = id.trim();
    let Some(digits) = trimmed
        .strip_prefix('C')
        .or_else(|| trimmed.strip_prefix('c'))
    else {
        return Err(AppError::Easyeda(EasyedaError::InvalidLcscId(
            id.to_string(),
        )));
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::Easyeda(EasyedaError::InvalidLcscId(
            id.to_string(),
        )));
    }
    Ok(())
}

fn path_ends_with(path: &std::path::Path, suffix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.ends_with(suffix))
        .unwrap_or(false)
}

fn generated_files_for(manager: &LibraryManager, options: &KicadExportOptions) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if options.targets_symbol() {
        files.push(manager.get_symbol_lib_path());
    }
    if options.targets_footprint() {
        files.push(manager.footprint_lib_dir().to_path_buf());
    }
    if options.targets_model_3d() {
        files.push(manager.model_lib_dir().to_path_buf());
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::KicadVersion;

    #[test]
    fn rejects_empty_target_selection() {
        let export = KicadExportOptions {
            symbol: false,
            footprint: false,
            model_3d: false,
            full: false,
            force: false,
            project_relative: false,
            kicad_version: KicadVersion::V6,
        };

        let err = validate_targets(&export).unwrap_err().to_string();
        assert!(err.contains("at least one KiCad export target"));
    }

    #[test]
    fn validates_explicit_library_extensions() {
        let options = KicadLibraryOptions {
            output: PathBuf::from("out"),
            library_name: None,
            symbol_lib: Some(PathBuf::from("bad.lib")),
            footprint_lib: None,
            model_lib: None,
        };

        let err = validate_library_options(&options).unwrap_err().to_string();
        assert!(err.contains(".kicad_sym"));
    }

    #[test]
    fn accepts_valid_lcsc_ids_case_insensitively() {
        validate_lcsc_id("C2040").unwrap();
        validate_lcsc_id("c2040").unwrap();
        assert!(validate_lcsc_id("C20A40").is_err());
    }

    #[test]
    fn requested_metadata_keeps_base_component_fields_without_product() {
        let mut component_data = ComponentData {
            lcsc_id: "C123".to_string(),
            title: "CN-TITLE".to_string(),
            description: "CN description".to_string(),
            data_str: Vec::new(),
            bbox_x: 0.0,
            bbox_y: 0.0,
            symbol_parts: Vec::new(),
            package_detail: Vec::new(),
            package_bbox_x: 0.0,
            package_bbox_y: 0.0,
            model_3d: None,
            manufacturer: "CN Manufacturer".to_string(),
            datasheet: "https://source.test/ds".to_string(),
            jlc_id: String::new(),
        };

        let metadata = ResolvedMetadata::from_component_data(&component_data, None);
        metadata.apply_to_component_data(&mut component_data);

        assert_eq!(component_data.title, "CN-TITLE");
        assert_eq!(component_data.description, "CN description");
        assert_eq!(component_data.manufacturer, "CN Manufacturer");
        assert_eq!(component_data.datasheet, "https://source.test/ds");
    }
}
