use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::altium::merge::{
    PcblibRecordLibrary, SchlibRecord, normalize_lcsc_id, pcblib_records_from_library,
    read_pcblib_records, read_schlib_records, schlib_record_from_component, write_pcblib_records,
    write_schlib_records,
};
use crate::altium::pcblib::{PcbLibrary, write_pcblib};
use crate::altium::workflow::{
    build_pcblib_library_from_detail, build_schlib_component_from_detail, export_pcblib,
    export_schlib_with_options, resolved_footprint_name,
};
use crate::error::{AppError, Result};
use crate::lceda::{LcedaClient, SearchItem};
use crate::lcsc::{LcscClient, LcscProduct};
use crate::util::{component_identity_name, display_path, sanitize_filename};

#[derive(Debug, Clone)]
pub struct BatchOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub schlib: bool,
    pub pcblib: bool,
    pub full: bool,
    pub merge: bool,
    pub append: bool,
    pub library_name: Option<String>,
    pub parallel: usize,
    pub continue_on_error: bool,
    pub english_metadata: bool,
    pub force: bool,
    /// What happens to downloaded 3D models before they are written (logo removal).
    pub models: Arc<crate::step_model::ModelCleanup>,
}

#[derive(Debug, Clone)]
pub struct BatchSummary {
    pub total: usize,
    pub skipped: usize,
    pub success: usize,
    pub failed: usize,
    pub failed_ids: Vec<String>,
    pub failure_details: Vec<String>,
    pub output: PathBuf,
    pub generated_files: Vec<PathBuf>,
}

pub fn discover_altium_library_name(directory: &Path) -> Result<String> {
    if !directory.is_dir() {
        return Err(AppError::Other(format!(
            "Altium append directory does not exist: {}",
            display_path(directory)
        )));
    }

    let mut schlibs = HashSet::new();
    let mut pcblibs = HashSet::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".SchLib") {
            schlibs.insert(stem.to_string());
        } else if let Some(stem) = name.strip_suffix(".PcbLib") {
            pcblibs.insert(stem.to_string());
        }
    }

    let mut pairs: Vec<String> = schlibs.intersection(&pcblibs).cloned().collect();
    pairs.sort_unstable();
    match pairs.as_slice() {
        [name] => Ok(name.clone()),
        [] => Err(AppError::Other(format!(
            "Altium append directory contains no matching .SchLib/.PcbLib pair: {}",
            display_path(directory)
        ))),
        _ => Err(AppError::Other(format!(
            "Altium append directory contains multiple library pairs: {}. Use --name to select one.",
            pairs.join(", ")
        ))),
    }
}

pub async fn export_batch(client: &LcedaClient, options: BatchOptions) -> Result<BatchSummary> {
    let targets = ExportTargets::resolve(&options)?;
    if options.merge {
        return export_batch_merged(client, options, targets).await;
    }

    let options = Arc::new(options);

    fs::create_dir_all(&options.output)?;

    let input = fs::read_to_string(&options.input)?;
    let ids = parse_lcsc_ids(&input);
    if ids.is_empty() {
        return Err(AppError::Other(
            "no valid LCSC IDs found in batch input".to_string(),
        ));
    }

    let checkpoint_path = options.output.join(".checkpoint");
    let completed = load_checkpoint(&checkpoint_path)?;

    let mut pending = Vec::new();
    let mut skipped = 0usize;
    for id in ids {
        if !options.force && completed.contains(&id) {
            skipped += 1;
        } else {
            pending.push(id);
        }
    }

    let mut summary = BatchSummary {
        total: pending.len() + skipped,
        skipped,
        success: 0,
        failed: 0,
        failed_ids: Vec::new(),
        failure_details: Vec::new(),
        output: options.output.clone(),
        generated_files: Vec::new(),
    };

    let actual_parallel = if options.parallel > 1 && pending.len() > 1 {
        options.parallel
    } else {
        1
    };
    let mut progress = BatchProgress::new(
        summary.total,
        actual_parallel,
        batch_mode_label(&options),
        targets,
        &options.output,
    );
    if summary.skipped > 0 {
        progress.seed_skipped(summary.skipped, "checkpoint");
    }

    if pending.is_empty() {
        progress.finish();
        return Ok(summary);
    }

    if actual_parallel > 1 {
        run_parallel(
            client.clone(),
            options.clone(),
            targets,
            &checkpoint_path,
            pending,
            &mut summary,
            &mut progress,
        )
        .await?;
    } else {
        run_sequential(
            client.clone(),
            options.clone(),
            targets,
            &checkpoint_path,
            pending,
            &mut summary,
            &mut progress,
        )
        .await?;
    }

    progress.finish();
    Ok(summary)
}

#[derive(Debug, Clone, Copy)]
struct ExportTargets {
    schlib: bool,
    pcblib: bool,
}

impl ExportTargets {
    fn resolve(options: &BatchOptions) -> Result<Self> {
        if options.parallel == 0 {
            return Err(AppError::Other("-j must be at least 1".to_string()));
        }
        let schlib = options.schlib || options.full;
        let pcblib = options.pcblib || options.full;
        if !schlib && !pcblib {
            return Err(AppError::Other(
                "at least one export target must be selected (--schlib, --pcblib, or --full)"
                    .to_string(),
            ));
        }
        if options.append && !schlib {
            return Err(AppError::Other(
                "--append currently requires --schlib or --full".to_string(),
            ));
        }

        Ok(Self { schlib, pcblib })
    }

    fn label(self) -> &'static str {
        match (self.schlib, self.pcblib) {
            (true, true) => "SchLib+PcbLib",
            (true, false) => "SchLib",
            (false, true) => "PcbLib",
            (false, false) => "none",
        }
    }
}

#[derive(Debug)]
struct MergeArtifacts {
    identity: String,
    component_name: String,
    schlib_record: Option<SchlibRecord>,
    pcblib_library: Option<PcbLibrary>,
}

#[derive(Debug)]
struct IndexedMergeArtifacts {
    index: usize,
    artifacts: MergeArtifacts,
}

#[derive(Debug)]
struct IndexedId {
    index: usize,
    id: String,
}

#[derive(Debug)]
struct PreparedMergeComponent {
    index: usize,
    id: String,
    item: SearchItem,
    identity: String,
    component_name_base: String,
    footprint_name_base: String,
    symbol_data: Option<Value>,
    footprint_data: Option<Value>,
    english_metadata: Option<LcscProduct>,
}

#[derive(Debug)]
struct NamedMergeComponent {
    index: usize,
    id: String,
    item: SearchItem,
    identity: String,
    component_name: String,
    footprint_name: String,
    symbol_data: Option<Value>,
    footprint_data: Option<Value>,
    english_metadata: Option<LcscProduct>,
}

#[derive(Debug)]
struct ExportedComponent {
    display_name: String,
}

struct BatchProgress {
    total: usize,
    completed: usize,
    success: usize,
    skipped: usize,
    failed: usize,
    parallel: usize,
    started_at: Instant,
    last_subject: Option<String>,
    bar: ProgressBar,
}

impl BatchProgress {
    fn new(
        total: usize,
        parallel: usize,
        mode_label: &str,
        targets: ExportTargets,
        output: &Path,
    ) -> Self {
        eprintln!(
            "Batch mode: {mode_label} | targets: {} | parallel: {}",
            targets.label(),
            parallel.max(1)
        );
        eprintln!("Output: {}", display_path(output));

        let bar = ProgressBar::new(total as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{bar:24.cyan/blue}] {pos}/{len} | ok:{msg}",
            )
            .expect("valid batch progress template")
            .progress_chars("##-"),
        );
        bar.enable_steady_tick(std::time::Duration::from_millis(250));
        let mut progress = Self {
            total,
            completed: 0,
            success: 0,
            skipped: 0,
            failed: 0,
            parallel: parallel.max(1),
            started_at: Instant::now(),
            last_subject: None,
            bar,
        };
        progress.render();
        progress
    }

    fn seed_skipped(&mut self, count: usize, reason: &str) {
        if count == 0 {
            return;
        }
        self.completed = (self.completed + count).min(self.total);
        self.skipped += count;
        self.note(format!("Pre-skipped {count} item(s) from {reason}"));
    }

    fn note(&mut self, message: impl AsRef<str>) {
        self.bar.println(message.as_ref());
        self.render();
    }

    fn record_success(&mut self, id: &str, detail: Option<&str>) {
        self.completed = (self.completed + 1).min(self.total);
        self.success += 1;
        self.last_subject = Some(format_subject(id, detail));
        self.render();
    }

    fn record_skip(&mut self, id: &str, reason: &str) {
        self.completed = (self.completed + 1).min(self.total);
        self.skipped += 1;
        self.last_subject = Some(format_subject(id, None));
        self.note(format!("SKIP {id}: {reason}"));
    }

    fn record_failure(&mut self, id: &str, error: &AppError) {
        self.completed = (self.completed + 1).min(self.total);
        self.failed += 1;
        self.last_subject = Some(format_subject(id, None));
        self.note(format!("FAILED {id}: {error}"));
    }

    fn finish(&mut self) {
        self.bar.finish_and_clear();
    }

    fn render(&mut self) {
        let last = self.last_subject.as_deref().unwrap_or("-");
        self.bar.set_position(self.completed as u64);
        self.bar.set_message(format!(
            "{} skip:{} fail:{} | active:{} | last:{} | elapsed:{}",
            self.success,
            self.skipped,
            self.failed,
            self.active_count(),
            last,
            format_elapsed(self.started_at.elapsed().as_secs())
        ));
    }

    fn active_count(&self) -> usize {
        self.total.saturating_sub(self.completed).min(self.parallel)
    }
}

async fn export_batch_merged(
    client: &LcedaClient,
    options: BatchOptions,
    targets: ExportTargets,
) -> Result<BatchSummary> {
    fs::create_dir_all(&options.output)?;

    let input = fs::read_to_string(&options.input)?;
    let ids = parse_lcsc_ids(&input);
    if ids.is_empty() {
        return Err(AppError::Other(
            "no valid LCSC IDs found in batch input".to_string(),
        ));
    }

    let summary = BatchSummary {
        total: ids.len(),
        skipped: 0,
        success: 0,
        failed: 0,
        failed_ids: Vec::new(),
        failure_details: Vec::new(),
        output: options.output.clone(),
        generated_files: Vec::new(),
    };

    let actual_parallel = if options.parallel > 1 && ids.len() > 1 {
        options.parallel
    } else {
        1
    };
    let mut progress = BatchProgress::new(
        summary.total,
        actual_parallel,
        batch_mode_label(&options),
        targets,
        &options.output,
    );

    let result = if options.append {
        export_batch_merged_append(client, options, targets, ids, summary, &mut progress).await
    } else {
        export_batch_merged_fresh(client, options, targets, ids, summary, &mut progress).await
    };

    progress.finish();
    let summary = result?;
    Ok(summary)
}

async fn export_batch_merged_fresh(
    client: &LcedaClient,
    options: BatchOptions,
    targets: ExportTargets,
    ids: Vec<String>,
    mut summary: BatchSummary,
    progress: &mut BatchProgress,
) -> Result<BatchSummary> {
    let library_name = resolve_library_name(&options);
    let merged_pcblib_file = format!("{}.PcbLib", sanitize_filename(&library_name));
    let schlib_path = options
        .output
        .join(format!("{}.SchLib", sanitize_filename(&library_name)));
    let pcblib_path = options
        .output
        .join(format!("{}.PcbLib", sanitize_filename(&library_name)));

    reject_existing_merged_outputs(targets, &schlib_path, &pcblib_path, options.force)?;
    progress.note(format!("Library: {library_name}"));

    let mut used_symbol_names = HashSet::new();
    let mut used_footprint_names = HashSet::new();
    let exported = export_merged_components_parallel(
        client,
        &options,
        targets,
        ids.into_iter()
            .enumerate()
            .map(|(index, id)| IndexedId { index, id })
            .collect(),
        &mut used_symbol_names,
        &mut used_footprint_names,
        &merged_pcblib_file,
        &mut summary,
        progress,
    )
    .await?;

    if summary.success == 0 {
        return Err(AppError::Other(
            "no components exported successfully for merged batch".to_string(),
        ));
    }

    let mut schlib_records = Vec::new();
    let mut pcblib_library = PcbLibrary::default();
    for exported in exported {
        if let Some(record) = exported.artifacts.schlib_record {
            schlib_records.push(record);
        }
        if let Some(library) = exported.artifacts.pcblib_library {
            append_pcblib_library_direct(&mut pcblib_library, library);
        }
    }

    progress.note("Writing merged library files...");
    if targets.schlib {
        if schlib_records.is_empty() {
            return Err(AppError::Other(
                "cannot write merged SchLib without any components".to_string(),
            ));
        }
        write_schlib_records(&schlib_records, &schlib_path)?;
        summary.generated_files.push(schlib_path);
    }
    if targets.pcblib {
        if pcblib_library.components.is_empty() {
            return Err(AppError::Other(
                "cannot write merged PcbLib without any components".to_string(),
            ));
        }
        write_pcblib(&pcblib_library, &pcblib_path)?;
        summary.generated_files.push(pcblib_path);
    }

    Ok(summary)
}

async fn export_batch_merged_append(
    client: &LcedaClient,
    options: BatchOptions,
    targets: ExportTargets,
    ids: Vec<String>,
    mut summary: BatchSummary,
    progress: &mut BatchProgress,
) -> Result<BatchSummary> {
    let library_name = resolve_library_name(&options);
    let merged_pcblib_file = format!("{}.PcbLib", sanitize_filename(&library_name));
    let schlib_path = options
        .output
        .join(format!("{}.SchLib", sanitize_filename(&library_name)));
    let pcblib_path = options
        .output
        .join(format!("{}.PcbLib", sanitize_filename(&library_name)));

    progress.note(format!("Library: {library_name}"));

    if targets.schlib && targets.pcblib {
        let sch_exists = schlib_path.exists();
        let pcb_exists = pcblib_path.exists();
        if sch_exists != pcb_exists {
            return Err(AppError::Other(
                "append mode requires both merged SchLib and PcbLib to exist already, or neither"
                    .to_string(),
            ));
        }
    }

    let mut schlib_records = if targets.schlib && schlib_path.exists() {
        read_schlib_records(&schlib_path)?
    } else {
        Vec::new()
    };
    let mut pcblib_library = if targets.pcblib && pcblib_path.exists() {
        read_pcblib_records(&pcblib_path)?
    } else {
        PcblibRecordLibrary::default()
    };

    if schlib_records.is_empty() && pcblib_library.components.is_empty() {
        progress.note("Append mode: no existing merged npnp library found, creating a new one");
    } else {
        progress.note(format!(
            "Loaded existing merged output: SchLib components: {} | PcbLib components: {}",
            schlib_records.len(),
            pcblib_library.components.len()
        ));
    }

    let mut known_identities = HashSet::new();
    let mut used_symbol_names = HashSet::new();
    let mut used_footprint_names = HashSet::new();
    for record in &schlib_records {
        used_symbol_names.insert(record.name.to_ascii_lowercase());
        if let Some(identity) = record.identity.as_deref().and_then(normalize_lcsc_id) {
            known_identities.insert(identity);
        }
    }
    for component in &pcblib_library.components {
        used_footprint_names.insert(component.name.to_ascii_lowercase());
    }

    let mut pending = Vec::new();
    for (index, id) in ids.into_iter().enumerate() {
        let normalized_id = normalize_lcsc_id(&id).unwrap_or_else(|| id.clone());
        if known_identities.contains(&normalized_id) {
            summary.skipped += 1;
            progress.record_skip(&id, "already present");
            continue;
        }
        pending.push(IndexedId { index, id });
    }

    let exported = export_merged_components_parallel(
        client,
        &options,
        targets,
        pending,
        &mut used_symbol_names,
        &mut used_footprint_names,
        &merged_pcblib_file,
        &mut summary,
        progress,
    )
    .await?;

    let added_any = !exported.is_empty();
    for exported in exported {
        known_identities.insert(exported.artifacts.identity);
        if let Some(record) = exported.artifacts.schlib_record {
            schlib_records.push(record);
        }
        if let Some(library) = exported.artifacts.pcblib_library {
            append_pcblib_library(&mut pcblib_library, pcblib_records_from_library(&library)?);
        }
    }

    if added_any {
        progress.note("Writing merged library files...");
        write_merged_outputs(
            targets,
            &schlib_records,
            &pcblib_library,
            &schlib_path,
            &pcblib_path,
            &mut summary,
        )?;
    }

    Ok(summary)
}

/// Writes into a merged library that npnp itself produced.
///
/// A library the user has marked read-only (or that a copy left read-only)
/// cannot be overwritten as it is, and an append into it would otherwise fail
/// with a bare "access is denied" that says nothing about which file is at fault.
fn make_writable(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let permissions = metadata.permissions();
    if !permissions.readonly() {
        return Ok(());
    }
    fs::set_permissions(path, writable(permissions)).map_err(|err| {
        AppError::Other(format!(
            "the existing library {} is read-only and cannot be updated: {err}",
            path.display()
        ))
    })
}

/// The same permissions, with permission to write the file.
///
/// `set_readonly(false)` is only the right call on Windows. On Unix it clears
/// every write bit at once, which leaves the library world writable, so the
/// owner's bit is set instead and the rest of the mode is left alone.
#[cfg(unix)]
fn writable(permissions: fs::Permissions) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(permissions.mode() | 0o200)
}

#[cfg(not(unix))]
#[allow(clippy::permissions_set_readonly_false)] // the correct call on Windows
fn writable(mut permissions: fs::Permissions) -> fs::Permissions {
    permissions.set_readonly(false);
    permissions
}

fn write_merged_outputs(
    targets: ExportTargets,
    schlib_records: &[SchlibRecord],
    pcblib_library: &PcblibRecordLibrary,
    schlib_path: &Path,
    pcblib_path: &Path,
    summary: &mut BatchSummary,
) -> Result<()> {
    if targets.schlib {
        if schlib_records.is_empty() {
            return Err(AppError::Other(
                "cannot write merged SchLib without any components".to_string(),
            ));
        }
        make_writable(schlib_path)?;
        write_schlib_records(schlib_records, schlib_path)?;
        summary.generated_files.push(schlib_path.to_path_buf());
    }
    if targets.pcblib {
        if pcblib_library.components.is_empty() {
            return Err(AppError::Other(
                "cannot write merged PcbLib without any components".to_string(),
            ));
        }
        make_writable(pcblib_path)?;
        write_pcblib_records(pcblib_library, pcblib_path)?;
        summary.generated_files.push(pcblib_path.to_path_buf());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn export_merged_components_parallel(
    client: &LcedaClient,
    options: &BatchOptions,
    targets: ExportTargets,
    indexed_ids: Vec<IndexedId>,
    used_symbol_names: &mut HashSet<String>,
    used_footprint_names: &mut HashSet<String>,
    merged_pcblib_file: &str,
    summary: &mut BatchSummary,
    progress: &mut BatchProgress,
) -> Result<Vec<IndexedMergeArtifacts>> {
    if indexed_ids.is_empty() {
        return Ok(Vec::new());
    }

    let parallel = options.parallel.max(1).min(indexed_ids.len());
    let mut first_error = None;
    let mut prepared = prepare_merged_components_parallel(
        client,
        targets,
        indexed_ids,
        options.english_metadata,
        parallel,
        summary,
        progress,
        &mut first_error,
    )
    .await;

    if prepared.is_empty() {
        if let Some(err) = first_error {
            return Err(err);
        }
        return Ok(Vec::new());
    }
    if !options.continue_on_error
        && let Some(err) = first_error
    {
        return Err(err);
    }

    prepared.sort_by_key(|prepared| prepared.index);
    let named = prepared
        .into_iter()
        .map(|prepared| {
            let component_name = merged_unique_name(
                &prepared.component_name_base,
                &prepared.id,
                used_symbol_names,
            );
            let footprint_name = merged_unique_name(
                &prepared.footprint_name_base,
                &prepared.id,
                used_footprint_names,
            );
            NamedMergeComponent {
                index: prepared.index,
                id: prepared.id,
                item: prepared.item,
                identity: prepared.identity,
                component_name,
                footprint_name,
                symbol_data: prepared.symbol_data,
                footprint_data: prepared.footprint_data,
                english_metadata: prepared.english_metadata,
            }
        })
        .collect();

    let mut exported = build_merged_components_parallel(
        Arc::clone(&options.models),
        client,
        targets,
        named,
        merged_pcblib_file,
        parallel,
        summary,
        progress,
        &mut first_error,
    )
    .await;

    if summary.failed > 0 && !options.continue_on_error {
        return Err(first_error.unwrap_or_else(|| AppError::Other("merge export failed".into())));
    }

    exported.sort_by_key(|exported| exported.index);
    Ok(exported)
}

#[allow(clippy::too_many_arguments)]
async fn prepare_merged_components_parallel(
    client: &LcedaClient,
    targets: ExportTargets,
    indexed_ids: Vec<IndexedId>,
    english_metadata: bool,
    parallel: usize,
    summary: &mut BatchSummary,
    progress: &mut BatchProgress,
    first_error: &mut Option<AppError>,
) -> Vec<PreparedMergeComponent> {
    let semaphore = Arc::new(Semaphore::new(parallel));
    let mut join_set = JoinSet::new();

    for indexed in indexed_ids {
        let client = client.clone();
        let semaphore = semaphore.clone();
        join_set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("merge prepare semaphore should remain open");
            let id = indexed.id;
            let result = prepare_merged_component(
                &client,
                targets,
                indexed.index,
                id.clone(),
                english_metadata,
            )
            .await;
            (id, result)
        });
    }

    let mut prepared = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((_id, Ok(item))) => prepared.push(item),
            Ok((id, Err(err))) => {
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                summary.failure_details.push(format!("{id}: {err}"));
                progress.record_failure(&id, &err);
                if first_error.is_none() {
                    *first_error = Some(err);
                }
            }
            Err(err) => {
                summary.failed += 1;
                let batch_err = AppError::Other(format!("merge prepare task join failed: {err}"));
                summary.failed_ids.push("<join>".to_string());
                summary.failure_details.push(batch_err.to_string());
                progress.record_failure("<join>", &batch_err);
                if first_error.is_none() {
                    *first_error = Some(batch_err);
                }
            }
        }
    }

    prepared
}

async fn prepare_merged_component(
    client: &LcedaClient,
    targets: ExportTargets,
    index: usize,
    id: String,
    english_metadata: bool,
) -> Result<PreparedMergeComponent> {
    let item = client.select_item(&id, 1).await?;
    let identity = item
        .lcsc_id()
        .as_deref()
        .and_then(normalize_lcsc_id)
        .unwrap_or_else(|| id.clone());
    let component_name_base = component_identity_name(item.display_name(), &identity);

    let footprint_data = if targets.pcblib || item.footprint_uuid().is_some() {
        let footprint_uuid = item
            .footprint_uuid()
            .ok_or(AppError::MissingSymbolOrFootprint)?;
        Some(client.component_detail(&footprint_uuid).await?)
    } else {
        None
    };
    let footprint_name_base = footprint_data
        .as_ref()
        .map(|data| component_identity_name(&resolved_footprint_name(&item, data), &identity))
        .unwrap_or_else(|| component_name_base.clone());

    let symbol_data = if targets.schlib {
        let symbol_uuid = item
            .symbol_uuid()
            .ok_or(AppError::MissingSymbolOrFootprint)?;
        Some(client.component_detail(&symbol_uuid).await?)
    } else {
        None
    };

    let english_metadata = if targets.schlib && english_metadata {
        LcscClient::new().product_detail_optional(&identity).await
    } else {
        None
    };

    Ok(PreparedMergeComponent {
        index,
        id,
        item,
        identity,
        component_name_base,
        footprint_name_base,
        symbol_data,
        footprint_data,
        english_metadata,
    })
}

#[allow(clippy::too_many_arguments)]
async fn build_merged_components_parallel(
    models: Arc<crate::step_model::ModelCleanup>,
    client: &LcedaClient,
    targets: ExportTargets,
    named: Vec<NamedMergeComponent>,
    merged_pcblib_file: &str,
    parallel: usize,
    summary: &mut BatchSummary,
    progress: &mut BatchProgress,
    first_error: &mut Option<AppError>,
) -> Vec<IndexedMergeArtifacts> {
    let semaphore = Arc::new(Semaphore::new(parallel));
    let merged_pcblib_file = Arc::new(merged_pcblib_file.to_string());
    let mut join_set = JoinSet::new();

    for named in named {
        let client = client.clone();
        let semaphore = semaphore.clone();
        let merged_pcblib_file = merged_pcblib_file.clone();
        let models = Arc::clone(&models);
        join_set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("merge build semaphore should remain open");
            let id = named.id.clone();
            let result = build_named_merged_component(
                &client,
                targets,
                named,
                merged_pcblib_file.as_str(),
                &models,
            )
            .await;
            (id, result)
        });
    }

    let mut exported = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((id, Ok(item))) => {
                summary.success += 1;
                progress.record_success(&id, Some(&item.artifacts.component_name));
                exported.push(item);
            }
            Ok((id, Err(err))) => {
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                summary.failure_details.push(format!("{id}: {err}"));
                progress.record_failure(&id, &err);
                if first_error.is_none() {
                    *first_error = Some(err);
                }
            }
            Err(err) => {
                summary.failed += 1;
                let batch_err = AppError::Other(format!("merge build task join failed: {err}"));
                summary.failed_ids.push("<join>".to_string());
                summary.failure_details.push(batch_err.to_string());
                progress.record_failure("<join>", &batch_err);
                if first_error.is_none() {
                    *first_error = Some(batch_err);
                }
            }
        }
    }

    exported
}

async fn build_named_merged_component(
    client: &LcedaClient,
    targets: ExportTargets,
    named: NamedMergeComponent,
    merged_pcblib_file: &str,
    models: &crate::step_model::ModelCleanup,
) -> Result<IndexedMergeArtifacts> {
    let schlib_record = if targets.schlib {
        let symbol_data = named
            .symbol_data
            .as_ref()
            .ok_or(AppError::MissingSymbolOrFootprint)?;
        let component = build_schlib_component_from_detail(
            &named.item,
            symbol_data,
            &named.component_name,
            Some(&named.footprint_name),
            Some(merged_pcblib_file),
            named.english_metadata.as_ref(),
        )?;
        Some(schlib_record_from_component(&component)?)
    } else {
        None
    };

    let pcblib_library = if targets.pcblib {
        let footprint_data = named
            .footprint_data
            .as_ref()
            .ok_or(AppError::MissingSymbolOrFootprint)?;
        Some(
            build_pcblib_library_from_detail(
                client,
                &named.item,
                footprint_data,
                &named.footprint_name,
                models,
            )
            .await?,
        )
    } else {
        None
    };

    Ok(IndexedMergeArtifacts {
        index: named.index,
        artifacts: MergeArtifacts {
            identity: named.identity,
            component_name: named.component_name,
            schlib_record,
            pcblib_library,
        },
    })
}

fn reject_existing_merged_outputs(
    targets: ExportTargets,
    schlib_path: &Path,
    pcblib_path: &Path,
    force: bool,
) -> Result<()> {
    // `--force` is the caller saying "replace what is there", which is exactly
    // what the merge writer does, so the guard has nothing to protect.
    if force {
        return Ok(());
    }
    let mut existing = Vec::new();
    if targets.schlib && schlib_path.exists() {
        existing.push(display_path(schlib_path));
    }
    if targets.pcblib && pcblib_path.exists() {
        existing.push(display_path(pcblib_path));
    }
    if existing.is_empty() {
        return Ok(());
    }

    Err(AppError::Other(format!(
        "merged output already exists: {}. Use --append to add components, --force to replace the library, or choose a new --name.",
        existing.join(", ")
    )))
}

fn append_pcblib_library_direct(target: &mut PcbLibrary, source: PcbLibrary) {
    target.components.extend(source.components);
    target.models.extend(source.models);
}

fn merged_unique_name(base: &str, lcsc_id: &str, used_names: &mut HashSet<String>) -> String {
    let base = component_identity_name(base, lcsc_id);
    if used_names.insert(base.to_ascii_lowercase()) {
        return base;
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{base}_{index}");
        if used_names.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
        index += 1;
    }
}

fn append_pcblib_library(target: &mut PcblibRecordLibrary, source: PcblibRecordLibrary) {
    target.components.extend(source.components);
    target.models.extend(source.models);
}

fn resolve_library_name(options: &BatchOptions) -> String {
    if let Some(name) = options.library_name.as_deref() {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    options
        .input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("MergedLib")
        .to_string()
}

async fn run_sequential(
    client: LcedaClient,
    options: Arc<BatchOptions>,
    targets: ExportTargets,
    checkpoint_path: &Path,
    pending: Vec<String>,
    summary: &mut BatchSummary,
    progress: &mut BatchProgress,
) -> Result<()> {
    for id in pending {
        match export_component(&client, &options, targets, &id).await {
            Ok(exported) => {
                append_checkpoint(checkpoint_path, &id)?;
                summary.success += 1;
                progress.record_success(&id, Some(&exported.display_name));
            }
            Err(err) => {
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                summary.failure_details.push(format!("{id}: {err}"));
                progress.record_failure(&id, &err);
                if !options.continue_on_error {
                    return Err(err);
                }
            }
        }
    }

    Ok(())
}

async fn run_parallel(
    client: LcedaClient,
    options: Arc<BatchOptions>,
    targets: ExportTargets,
    checkpoint_path: &Path,
    pending: Vec<String>,
    summary: &mut BatchSummary,
    progress: &mut BatchProgress,
) -> Result<()> {
    let semaphore = Arc::new(Semaphore::new(options.parallel));
    let mut join_set = JoinSet::new();

    for id in pending {
        let client = client.clone();
        let options = options.clone();
        let semaphore = semaphore.clone();
        join_set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("batch semaphore should remain open");
            let result = export_component(&client, &options, targets, &id).await;
            (id, result)
        });
    }

    let mut first_error = None;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((id, Ok(exported))) => {
                append_checkpoint(checkpoint_path, &id)?;
                summary.success += 1;
                progress.record_success(&id, Some(&exported.display_name));
            }
            Ok((id, Err(err))) => {
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                summary.failure_details.push(format!("{id}: {err}"));
                progress.record_failure(&id, &err);
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(err) => {
                summary.failed += 1;
                let batch_err = AppError::Other(format!("batch task join failed: {err}"));
                summary.failed_ids.push("<join>".to_string());
                summary.failure_details.push(batch_err.to_string());
                progress.record_failure("<join>", &batch_err);
                if first_error.is_none() {
                    first_error = Some(batch_err);
                }
            }
        }
    }

    if summary.failed > 0 && !options.continue_on_error {
        return Err(first_error.unwrap_or_else(|| AppError::Other("batch export failed".into())));
    }

    Ok(())
}

async fn export_component(
    client: &LcedaClient,
    options: &BatchOptions,
    targets: ExportTargets,
    lcsc_id: &str,
) -> Result<ExportedComponent> {
    let item = client.select_item(lcsc_id, 1).await?;
    let display_name = item.display_name().to_string();

    if targets.schlib {
        let schlib_dir = options.output.join("schlib");
        export_schlib_with_options(
            client,
            &item,
            &schlib_dir,
            options.force,
            options.english_metadata,
            // The same stroke and fill the plain `export_schlib` uses.
            0x68380B,
            0xE0FFFF,
        )
        .await?;
    }

    if targets.pcblib {
        let pcblib_dir = options.output.join("pcblib");
        export_pcblib(client, &item, &pcblib_dir, options.force, &options.models).await?;
    }

    Ok(ExportedComponent { display_name })
}

fn load_checkpoint(path: &Path) -> Result<HashSet<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(err) => Err(err.into()),
    }
}

fn append_checkpoint(path: &Path, id: &str) -> Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{id}")?;
    Ok(())
}

pub(crate) fn parse_lcsc_ids(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'C' || byte == b'c' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }

            if end > start {
                let digits = std::str::from_utf8(&bytes[start..end])
                    .expect("ASCII digits must be valid UTF-8");
                let id = format!("C{digits}");
                if seen.insert(id.clone()) {
                    ids.push(id);
                }
                index = end;
                continue;
            }
        }

        index += 1;
    }

    ids
}

fn batch_mode_label(options: &BatchOptions) -> &'static str {
    match (options.merge, options.append) {
        (true, true) => "merge+append",
        (true, false) => "merge",
        (false, _) => "batch",
    }
}

fn format_subject(id: &str, detail: Option<&str>) -> String {
    let mut text = match detail {
        Some(detail) if !detail.trim().is_empty() => format!("{id} {detail}"),
        _ => id.to_string(),
    };
    if text.chars().count() > 52 {
        text = format!("{}...", text.chars().take(49).collect::<String>());
    }
    text
}

fn format_elapsed(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BatchOptions, ExportTargets, discover_altium_library_name, format_elapsed, format_subject,
        parse_lcsc_ids, reject_existing_merged_outputs, resolve_library_name,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_ids_deduplicates_and_preserves_order() {
        let ids = parse_lcsc_ids("C2040\nfoo C5676243 bar c2040 baz C42");
        assert_eq!(ids, vec!["C2040", "C5676243", "C42"]);
    }

    #[test]
    fn discovers_single_altium_append_pair() {
        let root =
            std::env::temp_dir().join(format!("npnp-altium-discovery-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Parts.SchLib"), b"").unwrap();
        fs::write(root.join("Parts.PcbLib"), b"").unwrap();
        assert_eq!(discover_altium_library_name(&root).unwrap(), "Parts");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_ids_ignores_invalid_matches() {
        let ids = parse_lcsc_ids("C abc c-1 test");
        assert!(ids.is_empty());
    }

    #[test]
    fn resolve_library_name_defaults_to_input_stem() {
        let options = BatchOptions {
            input: PathBuf::from("ids.txt"),
            output: PathBuf::from("out"),
            schlib: true,
            pcblib: false,
            full: false,
            merge: true,
            append: false,
            library_name: None,
            parallel: 1,
            continue_on_error: false,
            english_metadata: false,
            force: false,
            models: std::sync::Arc::new(crate::step_model::ModelCleanup::new(false)),
        };

        assert_eq!(resolve_library_name(&options), "ids");
    }

    #[test]
    fn fresh_merge_rejects_existing_selected_output() {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let schlib_path = std::env::temp_dir().join(format!("npnp_merge_guard_{timestamp}.SchLib"));
        let pcblib_path = std::env::temp_dir().join(format!("npnp_merge_guard_{timestamp}.PcbLib"));
        fs::write(&schlib_path, b"old library").unwrap();

        let err = reject_existing_merged_outputs(
            ExportTargets {
                schlib: true,
                pcblib: false,
            },
            &schlib_path,
            &pcblib_path,
            false,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("already exists"));
        assert!(err.contains("--append"));
        assert!(err.contains("--force"));

        // --force is the caller asking for the replace the merge writer does
        // anyway, so the guard steps aside.
        assert!(
            reject_existing_merged_outputs(
                ExportTargets {
                    schlib: true,
                    pcblib: false,
                },
                &schlib_path,
                &pcblib_path,
                true,
            )
            .is_ok()
        );
        fs::remove_file(schlib_path).ok();
    }

    #[test]
    fn formats_progress_subject_with_component_name() {
        assert_eq!(format_subject("C2040", Some("RP2040")), "C2040 RP2040");
    }

    #[test]
    fn formats_elapsed_time_without_hours() {
        assert_eq!(format_elapsed(65), "01:05");
    }
}
