use std::collections::{BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::easyeda::{ComponentData, EasyedaApi};
use crate::error::{AppError, EasyedaError, Result};
use crate::kicad::{
    KicadExportOptions, KicadLibraryOptions, LibraryManager, ModelExportStatus, ModelStageStatus,
    convert_3d_model, convert_footprint, convert_symbol,
};
use crate::lcsc::LcscClient;
use crate::metadata::ResolvedMetadata;

/// A component that has not answered within this is failed and skipped, so one
/// stuck part cannot hold the whole batch.
const COMPONENT_TIMEOUT: Duration = Duration::from_secs(180);

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
    /// Add to the library that is already in the output directory. Without it
    /// the export owns the library and refuses to write into one that is there,
    /// which is what Altium's append flag means as well.
    pub append: bool,
}

#[derive(Debug, Clone)]
pub struct KicadBatchSummary {
    pub total: usize,
    pub skipped: usize,
    pub success: usize,
    pub failed: usize,
    pub failed_ids: Vec<String>,
    pub failure_details: Vec<String>,
    pub generated_files: Vec<PathBuf>,
    pub output: PathBuf,
    pub timings: KicadStageTimings,
    pub models: KicadModelSummary,
}

#[derive(Debug, Clone)]
pub struct KicadExportedComponent {
    pub lcsc_id: String,
    pub component_name: String,
    pub timings: KicadStageTimings,
    pub model: Option<ModelExportStatus>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KicadStageTimings {
    pub component_fetch: Duration,
    pub metadata: Duration,
    pub symbol: Duration,
    pub footprint: Duration,
    pub model_3d: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KicadModelSummary {
    pub written: usize,
    pub skipped: usize,
    pub missing: usize,
}

impl KicadStageTimings {
    fn add(&mut self, other: Self) {
        self.component_fetch += other.component_fetch;
        self.metadata += other.metadata;
        self.symbol += other.symbol;
        self.footprint += other.footprint;
        self.model_3d += other.model_3d;
    }
}

impl KicadModelSummary {
    fn record(&mut self, status: Option<ModelExportStatus>) {
        match status {
            Some(ModelExportStatus::Written) => self.written += 1,
            Some(ModelExportStatus::Skipped) => self.skipped += 1,
            Some(ModelExportStatus::Missing) => self.missing += 1,
            None => {}
        }
    }
}

pub async fn export_kicad_single(options: KicadSingleOptions) -> Result<KicadBatchSummary> {
    let batch_options = KicadBatchOptions {
        ids: vec![options.lcsc_id],
        library: options.library,
        export: options.export,
        english_metadata: options.english_metadata,
        parallel: 1,
        continue_on_error: false,
        // One component named one library: it is written into whatever is there,
        // which is how the CLI's --symbol-lib/--footprint-lib work.
        append: true,
    };
    export_kicad_batch(batch_options).await
}

pub async fn export_kicad_batch(options: KicadBatchOptions) -> Result<KicadBatchSummary> {
    validate_batch_options(&options)?;

    let api = Arc::new(EasyedaApi::new());
    let lcsc_client = options.english_metadata.then(LcscClient::new).map(Arc::new);
    let manager = Arc::new(LibraryManager::from_options(&options.library)?);
    reject_existing_library(&options, &manager)?;
    manager.create_directories()?;

    let model_status = Arc::new(ModelStageStatus::new());
    let export_options = Arc::new(options.export.clone());
    let checkpoint_path =
        kicad_checkpoint_path(manager.lib_name(), &options.library.output, &options.export);
    // The checkpoint is the record of what has already been exported into this
    // library, so it decides what to skip on its own. `force` says to replace
    // entries that are there, which is a different job: reading it as "do the
    // work again" is what made a repeat export re-download everything.
    let completed = if library_present(&options, &manager) {
        load_checkpoint(&checkpoint_path)?
    } else {
        HashSet::new()
    };
    let (ids, skipped) = pending_ids(options.ids.clone(), &completed);
    let mut summary = KicadBatchSummary {
        total: ids.len() + skipped,
        skipped,
        success: 0,
        failed: 0,
        failed_ids: Vec::new(),
        failure_details: Vec::new(),
        generated_files: generated_files_for(&manager, &options.export),
        output: options.library.output.clone(),
        timings: KicadStageTimings::default(),
        models: KicadModelSummary::default(),
    };
    if ids.is_empty() {
        return Ok(summary);
    }

    let parallel = options.parallel.max(1).min(ids.len().max(1));
    let progress = ProgressBar::new(ids.len() as u64);
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:24.cyan/blue}] {pos}/{len} | {msg}")
            .expect("valid KiCad progress template")
            .progress_chars("##-"),
    );

    // A part that is waiting on a slow or throttled store can sit for minutes,
    // and a bar that only moves when something finishes reads as a hang. So the
    // bar is fed from outside the tasks: the ids being fetched right now, the
    // tally so far, and the elapsed time. This is the same thing seex shows as
    // "Fetching C1234".
    let in_flight: Arc<Mutex<BTreeSet<String>>> = Arc::new(Mutex::new(BTreeSet::new()));
    let succeeded = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let already_skipped = summary.skipped;
    let ticker = {
        let progress = progress.clone();
        let in_flight = Arc::clone(&in_flight);
        let succeeded = Arc::clone(&succeeded);
        let failed_count = Arc::clone(&failed_count);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(500));
            // Redrawing only when the line changes keeps a slow part from
            // scrolling the terminal once a second; the spinner is what shows
            // that the run is alive.
            let mut last = String::new();
            loop {
                tick.tick().await;
                let (first, count) = {
                    let guard = match in_flight.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    (guard.iter().next().cloned(), guard.len())
                };
                let fetching = match (first, count) {
                    (None, _) => "waiting for the store".to_string(),
                    (Some(id), 1) => format!("fetching {id}"),
                    (Some(id), n) => format!("fetching {id} +{}", n - 1),
                };
                let message = format!(
                    "ok:{} skip:{} fail:{} | {fetching}",
                    succeeded.load(Ordering::Relaxed),
                    already_skipped,
                    failed_count.load(Ordering::Relaxed),
                );
                if message != last {
                    progress.set_message(message.clone());
                    last = message;
                }
            }
        })
    };
    let semaphore = Arc::new(Semaphore::new(parallel));
    let mut join_set = JoinSet::new();

    for id in ids {
        let api = api.clone();
        let lcsc_client = lcsc_client.clone();
        let manager = manager.clone();
        let model_status = model_status.clone();
        let export_options = export_options.clone();
        let semaphore = semaphore.clone();
        let in_flight = Arc::clone(&in_flight);
        join_set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("KiCad batch semaphore should remain open");
            if let Ok(mut guard) = in_flight.lock() {
                guard.insert(id.clone());
            }
            let result = match tokio::time::timeout(
                COMPONENT_TIMEOUT,
                export_one_kicad_component(
                    &api,
                    lcsc_client.as_deref(),
                    &manager,
                    &export_options,
                    &model_status,
                    &id,
                ),
            )
            .await
            {
                Ok(result) => result,
                // One stuck part must not hold the whole batch: it fails, and the
                // run carries on with the rest.
                Err(_) => Err(AppError::Other(format!(
                    "gave up after {}s",
                    COMPONENT_TIMEOUT.as_secs()
                ))),
            };
            if let Ok(mut guard) = in_flight.lock() {
                guard.remove(&id);
            }
            (id, result)
        });
    }

    let mut first_error = None;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((id, Ok(exported))) => {
                append_checkpoint(&checkpoint_path, &id)?;
                succeeded.fetch_add(1, Ordering::Relaxed);
                summary.success += 1;
                summary.timings.add(exported.timings);
                summary.models.record(exported.model);
                progress.inc(1);
            }
            Ok((id, Err(err))) => {
                failed_count.fetch_add(1, Ordering::Relaxed);
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                summary.failure_details.push(format!("{id}: {err}"));
                progress.inc(1);
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(err) => {
                failed_count.fetch_add(1, Ordering::Relaxed);
                summary.failed += 1;
                progress.inc(1);
                let batch_err = AppError::Other(format!("KiCad batch task join failed: {err}"));
                summary.failure_details.push(batch_err.to_string());
                if first_error.is_none() {
                    first_error = Some(batch_err);
                }
            }
        }
    }

    ticker.abort();
    progress.finish_and_clear();

    if summary.failed > 0 && !options.continue_on_error {
        return Err(first_error.unwrap_or_else(|| AppError::Other("KiCad export failed".into())));
    }
    if summary.success == 0 && summary.skipped == 0 {
        return Err(first_error.unwrap_or_else(|| {
            AppError::Other("no KiCad components exported successfully".to_string())
        }));
    }

    Ok(summary)
}

/// The checkpoint file for one library and one set of targets. The library is
/// part of the name because two libraries written into one directory are two
/// different sets of exported ids.
fn kicad_checkpoint_path(
    library_name: &str,
    output: &Path,
    options: &KicadExportOptions,
) -> PathBuf {
    let mut key = Vec::new();
    if options.full || options.symbol {
        key.push("symbol");
    }
    if options.full || options.footprint {
        key.push("footprint");
    }
    if options.full || options.model_3d {
        key.push("3d");
    }
    output.join(format!(
        ".kicad_checkpoint_{library_name}_{}",
        key.join("_")
    ))
}

/// Whether the outputs the checkpoint would be talking about are still on disk.
///
/// A checkpoint means "these ids are in this library", so it only counts while
/// the library is there: deleting it is how a user asks for everything to be
/// exported again. An absent 3D model directory does not count, because a part
/// without a model is ordinary and must not stop the rest from being skipped.
fn library_present(options: &KicadBatchOptions, manager: &LibraryManager) -> bool {
    let full = options.export.full;
    if options.export.model_3d && !full {
        // A 3D-only export is judged by its models: it writes nothing else.
        return holds_extension(manager.model_lib_dir(), "step");
    }
    let symbols = (options.export.symbol || full) && manager.get_symbol_lib_path().exists();
    let footprints = (options.export.footprint || full)
        && holds_extension(manager.footprint_lib_dir(), "kicad_mod");
    symbols || footprints
}

/// The ids still to export, and how many the checkpoint already covers.
fn pending_ids(ids: Vec<String>, completed: &HashSet<String>) -> (Vec<String>, usize) {
    let mut pending = Vec::new();
    let mut skipped = 0usize;
    for id in ids {
        if completed.contains(&normalize_checkpoint_id(&id)) {
            skipped += 1;
        } else {
            pending.push(id);
        }
    }
    (pending, skipped)
}

/// Refuses to write into a library that is already there unless the caller asked
/// to append, the same rule — and for the same reason — as the Altium export
/// refusing an existing merged library.
///
/// The writers themselves add what is missing to an existing library and leave
/// the rest alone, which *is* the append behaviour, so without this check the
/// flag would mean nothing on this path: exporting into a directory that already
/// holds a library would quietly merge the two.
fn reject_existing_library(options: &KicadBatchOptions, manager: &LibraryManager) -> Result<()> {
    // `--force` is already the caller saying "write into what is there", and
    // `--append` says the same for a library it means to add to. Rejecting the
    // library in either case would leave no way to finish a run that was
    // interrupted half way.
    if options.append || options.export.force {
        return Ok(());
    }

    let full = options.export.full;
    let mut existing = Vec::new();
    if options.export.symbol || full {
        let path = manager.get_symbol_lib_path();
        if path.exists() {
            existing.push(path.display().to_string());
        }
    }
    if options.export.footprint || full {
        let dir = manager.footprint_lib_dir();
        if holds_extension(dir, "kicad_mod") {
            existing.push(dir.display().to_string());
        }
    }
    if options.export.model_3d || full {
        let dir = manager.model_lib_dir();
        if holds_extension(dir, "step") {
            existing.push(dir.display().to_string());
        }
    }

    if existing.is_empty() {
        return Ok(());
    }
    Err(AppError::Other(format!(
        "KiCad library already exists: {}. Turn on Append to add components to it, or choose another library name.",
        existing.join(", ")
    )))
}

/// Whether a directory holds at least one file with this extension, which is what
/// makes it an existing library rather than an empty directory the export may as
/// well fill.
fn holds_extension(dir: &Path, extension: &str) -> bool {
    fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some(extension)
            })
        })
        .unwrap_or(false)
}

fn load_checkpoint(path: &Path) -> Result<HashSet<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content
            .lines()
            .map(normalize_checkpoint_id)
            .filter(|line| !line.is_empty())
            .collect()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(err) => Err(err.into()),
    }
}

fn append_checkpoint(path: &Path, id: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", normalize_checkpoint_id(id))?;
    Ok(())
}

fn normalize_checkpoint_id(id: &str) -> String {
    id.trim().to_ascii_uppercase()
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
    let mut timings = KicadStageTimings::default();

    let started = Instant::now();
    let mut component_data = api.get_component_data(lcsc_id).await?;
    timings.component_fetch = started.elapsed();

    let started = Instant::now();
    apply_requested_metadata(&mut component_data, lcsc_client).await;
    timings.metadata = started.elapsed();

    if options.targets_symbol() {
        let started = Instant::now();
        convert_symbol(options, &component_data, manager, lcsc_id)?;
        timings.symbol = started.elapsed();
    }
    if options.targets_footprint() {
        let started = Instant::now();
        convert_footprint(options, &component_data, manager, lcsc_id)?;
        timings.footprint = started.elapsed();
    }
    let mut model = None;
    if options.targets_model_3d() {
        let started = Instant::now();
        model = Some(
            convert_3d_model(
                options,
                api,
                &component_data,
                manager,
                lcsc_id,
                model_status,
            )
            .await?,
        );
        timings.model_3d = started.elapsed();
    }

    Ok(KicadExportedComponent {
        lcsc_id: lcsc_id.to_string(),
        component_name: component_data.title,
        timings,
        model,
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
        return Err(AppError::Other("-j must be at least 1".to_string()));
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
        return Err(AppError::Other("--name must not be empty".to_string()));
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

    #[test]
    fn rejects_empty_target_selection() {
        let export = KicadExportOptions {
            symbol: false,
            footprint: false,
            model_3d: false,
            full: false,
            force: false,
            project_relative: false,
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
            designator: "U?".to_string(),
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
            package: String::new(),
            value: String::new(),
        };

        let metadata = ResolvedMetadata::from_component_data(&component_data, None);
        metadata.apply_to_component_data(&mut component_data);

        assert_eq!(component_data.title, "CN-TITLE");
        assert_eq!(component_data.description, "CN description");
        assert_eq!(component_data.manufacturer, "CN Manufacturer");
        assert_eq!(component_data.datasheet, "https://source.test/ds");
    }
}
