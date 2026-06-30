use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::altium::merge::{
    PcblibRecordLibrary, SchlibRecord, normalize_lcsc_id, pcblib_records_from_library,
    read_pcblib_records, read_schlib_records, schlib_record_from_component, write_pcblib_records,
    write_schlib_records,
};
use crate::altium::pcblib::{PcbLibrary, write_pcblib};
use crate::altium::workflow::{
    build_pcblib_library_for_item, build_schlib_component_for_item_with_metadata, export_pcblib,
    export_schlib_with_options, resolved_footprint_name,
};
use crate::error::{AppError, Result};
use crate::lceda::{LcedaClient, SearchItem};
use crate::lcsc::LcscClient;
use crate::util::sanitize_filename;

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
}

#[derive(Debug, Clone)]
pub struct BatchSummary {
    pub total: usize,
    pub skipped: usize,
    pub success: usize,
    pub failed: usize,
    pub failed_ids: Vec<String>,
    pub output: PathBuf,
    pub generated_files: Vec<PathBuf>,
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
            return Err(AppError::Other("--parallel must be at least 1".to_string()));
        }
        if options.append && !options.merge {
            return Err(AppError::Other(
                "--append is only supported together with --merge".to_string(),
            ));
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
    last_render_width: usize,
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
        eprintln!("Output: {}", output.display());

        let mut progress = Self {
            total,
            completed: 0,
            success: 0,
            skipped: 0,
            failed: 0,
            parallel: parallel.max(1),
            started_at: Instant::now(),
            last_subject: None,
            last_render_width: 0,
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
        self.clear_status_line();
        eprintln!("{}", message.as_ref());
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
        if self.last_render_width > 0 {
            eprintln!();
            self.last_render_width = 0;
        }
    }

    fn render(&mut self) {
        let bar_width = 24usize;
        let filled = if self.total == 0 {
            bar_width
        } else {
            (self.completed * bar_width + self.total / 2) / self.total
        }
        .min(bar_width);
        let remaining = bar_width.saturating_sub(filled);
        let last = self.last_subject.as_deref().unwrap_or("-");
        let message = format!(
            "[{}{}] {}/{} | ok:{} skip:{} fail:{} | active:{} | last:{} | elapsed:{}",
            "#".repeat(filled),
            "-".repeat(remaining),
            self.completed,
            self.total,
            self.success,
            self.skipped,
            self.failed,
            self.active_count(),
            last,
            format_elapsed(self.started_at.elapsed().as_secs())
        );
        self.draw_status_line(&message);
    }

    fn active_count(&self) -> usize {
        self.total.saturating_sub(self.completed).min(self.parallel)
    }

    fn clear_status_line(&mut self) {
        if self.last_render_width == 0 {
            return;
        }
        eprint!("\r{}\r", " ".repeat(self.last_render_width));
        let _ = io::stderr().flush();
        self.last_render_width = 0;
    }

    fn draw_status_line(&mut self, message: &str) {
        let current_width = message.chars().count();
        let padding = self.last_render_width.saturating_sub(current_width);
        eprint!("\r{}{}", message, " ".repeat(padding));
        let _ = io::stderr().flush();
        self.last_render_width = current_width;
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
        output: options.output.clone(),
        generated_files: Vec::new(),
    };

    let mut progress = BatchProgress::new(
        summary.total,
        1,
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
    result
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

    progress.note(format!("Library: {library_name}"));

    let mut used_symbol_names = HashSet::new();
    let mut used_footprint_names = HashSet::new();
    let mut schlib_records = Vec::new();
    let mut pcblib_library = PcbLibrary::default();
    let mut first_error = None;

    for id in ids {
        match export_merged_component(
            client,
            targets,
            &id,
            &mut used_symbol_names,
            &mut used_footprint_names,
            &merged_pcblib_file,
            options.english_metadata,
        )
        .await
        {
            Ok(artifacts) => {
                if let Some(record) = artifacts.schlib_record {
                    schlib_records.push(record);
                }
                if let Some(library) = artifacts.pcblib_library {
                    append_pcblib_library_direct(&mut pcblib_library, library);
                }
                summary.success += 1;
                progress.record_success(&id, Some(&artifacts.component_name));
            }
            Err(err) => {
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                progress.record_failure(&id, &err);
                if first_error.is_none() {
                    first_error = Some(err);
                }
                if !options.continue_on_error {
                    return Err(first_error.unwrap());
                }
            }
        }
    }

    if summary.success == 0 {
        return Err(first_error.unwrap_or_else(|| {
            AppError::Other("no components exported successfully for merged batch".to_string())
        }));
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

    let mut added_any = false;
    let mut first_error = None;

    for id in ids {
        let normalized_id = normalize_lcsc_id(&id).unwrap_or_else(|| id.clone());
        if known_identities.contains(&normalized_id) {
            summary.skipped += 1;
            progress.record_skip(&id, "already present");
            continue;
        }

        match export_merged_component(
            client,
            targets,
            &id,
            &mut used_symbol_names,
            &mut used_footprint_names,
            &merged_pcblib_file,
            options.english_metadata,
        )
        .await
        {
            Ok(artifacts) => {
                known_identities.insert(artifacts.identity);
                if let Some(record) = artifacts.schlib_record {
                    schlib_records.push(record);
                }
                if let Some(library) = artifacts.pcblib_library {
                    append_pcblib_library(
                        &mut pcblib_library,
                        pcblib_records_from_library(&library)?,
                    );
                }
                summary.success += 1;
                added_any = true;
                progress.record_success(&id, Some(&artifacts.component_name));
            }
            Err(err) => {
                summary.failed += 1;
                summary.failed_ids.push(id.clone());
                progress.record_failure(&id, &err);
                if first_error.is_none() {
                    first_error = Some(err);
                }
                if !options.continue_on_error {
                    return Err(first_error.unwrap());
                }
            }
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
    } else if summary.failed > 0 && !options.continue_on_error {
        return Err(
            first_error.unwrap_or_else(|| AppError::Other("append merge failed".to_string()))
        );
    }

    Ok(summary)
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
        write_schlib_records(schlib_records, schlib_path)?;
        summary.generated_files.push(schlib_path.to_path_buf());
    }
    if targets.pcblib {
        if pcblib_library.components.is_empty() {
            return Err(AppError::Other(
                "cannot write merged PcbLib without any components".to_string(),
            ));
        }
        write_pcblib_records(pcblib_library, pcblib_path)?;
        summary.generated_files.push(pcblib_path.to_path_buf());
    }
    Ok(())
}

async fn export_merged_component(
    client: &LcedaClient,
    targets: ExportTargets,
    lcsc_id: &str,
    used_symbol_names: &mut HashSet<String>,
    used_footprint_names: &mut HashSet<String>,
    merged_pcblib_file: &str,
    english_metadata: bool,
) -> Result<MergeArtifacts> {
    let item = client.select_item(lcsc_id, 1).await?;
    let component_name = merged_symbol_component_name(&item, lcsc_id, used_symbol_names);
    let identity = item
        .lcsc_id()
        .as_deref()
        .and_then(normalize_lcsc_id)
        .unwrap_or_else(|| lcsc_id.to_string());
    let footprint_name = if let Some(footprint_uuid) = item.footprint_uuid() {
        let footprint_data = client.component_detail(&footprint_uuid).await?;
        merged_footprint_name(&item, &footprint_data, lcsc_id, used_footprint_names)
    } else {
        merged_symbol_component_name(&item, lcsc_id, used_footprint_names)
    };

    let schlib_record = if targets.schlib {
        let english_metadata = if english_metadata {
            LcscClient::new().product_detail_optional(&identity).await
        } else {
            None
        };
        let component = build_schlib_component_for_item_with_metadata(
            client,
            &item,
            &component_name,
            Some(&footprint_name),
            Some(merged_pcblib_file),
            english_metadata.as_ref(),
        )
        .await?;
        Some(schlib_record_from_component(&component)?)
    } else {
        None
    };

    let pcblib_library = if targets.pcblib {
        let library = build_pcblib_library_for_item(client, &item, &footprint_name).await?;
        Some(library)
    } else {
        None
    };

    Ok(MergeArtifacts {
        identity,
        component_name,
        schlib_record,
        pcblib_library,
    })
}

fn append_pcblib_library_direct(target: &mut PcbLibrary, source: PcbLibrary) {
    target.components.extend(source.components);
    target.models.extend(source.models);
}

fn merged_symbol_component_name(
    item: &SearchItem,
    lcsc_id: &str,
    used_names: &mut HashSet<String>,
) -> String {
    let base = item.display_name().trim();
    let base = if base.is_empty() { lcsc_id } else { base };
    let normalized_base = base.to_ascii_lowercase();
    if used_names.insert(normalized_base) {
        return base.to_string();
    }

    let with_id = format!("{base}_{lcsc_id}");
    let normalized_with_id = with_id.to_ascii_lowercase();
    if used_names.insert(normalized_with_id) {
        return with_id;
    }

    let mut index = 2usize;
    loop {
        let candidate = format!("{base}_{lcsc_id}_{index}");
        if used_names.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
        index += 1;
    }
}

fn merged_footprint_name(
    item: &SearchItem,
    footprint_data: &serde_json::Value,
    lcsc_id: &str,
    used_names: &mut HashSet<String>,
) -> String {
    let base = resolved_footprint_name(item, footprint_data);
    let normalized_base = base.to_ascii_lowercase();
    if used_names.insert(normalized_base) {
        return base;
    }

    let with_id = format!("{base}_{lcsc_id}");
    let normalized_with_id = with_id.to_ascii_lowercase();
    if used_names.insert(normalized_with_id) {
        return with_id;
    }

    let mut index = 2usize;
    loop {
        let candidate = format!("{base}_{lcsc_id}_{index}");
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
                progress.record_failure(&id, &err);
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(err) => {
                summary.failed += 1;
                let batch_err = AppError::Other(format!("batch task join failed: {err}"));
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
        )
        .await?;
    }

    if targets.pcblib {
        let pcblib_dir = options.output.join("pcblib");
        export_pcblib(client, &item, &pcblib_dir, options.force).await?;
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
        BatchOptions, format_elapsed, format_subject, parse_lcsc_ids, resolve_library_name,
    };
    use std::path::PathBuf;

    #[test]
    fn parse_ids_deduplicates_and_preserves_order() {
        let ids = parse_lcsc_ids("C2040\nfoo C5676243 bar c2040 baz C42");
        assert_eq!(ids, vec!["C2040", "C5676243", "C42"]);
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
        };

        assert_eq!(resolve_library_name(&options), "ids");
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
