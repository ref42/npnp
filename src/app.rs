use std::ffi::OsStr;
use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clap::{CommandFactory, Parser};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::altium::workflow::{
    download_obj, download_step, export_easyeda_sources, export_pcblib, export_schlib_with_options,
};
use crate::batch::{BatchOptions, discover_altium_library_name, export_batch, parse_lcsc_ids};
use crate::cadence::{CadenceBatchOptions, CadenceExportOptions, export_cadence_batch};
use crate::cli::{AltiumCommands, CadenceCommands, Cli, Commands, KicadCommands};
use crate::error::Result;
use crate::kicad::{
    KicadBatchOptions, KicadExportOptions, KicadLibraryOptions, KicadSingleOptions,
    export_kicad_batch, export_kicad_single,
};
use crate::lceda::LcedaClient;
use crate::util::display_path;

pub async fn run_from_env() -> i32 {
    let invoked_as = std::env::args_os()
        .next()
        .as_deref()
        .map(display_invocation_name)
        .unwrap_or_else(|| "npnp".to_string());

    match run_cli(Cli::parse(), &invoked_as).await {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("Error: {err}");
            2
        }
    }
}

pub async fn run_cli(cli: Cli, invoked_as: &str) -> Result<()> {
    let Some(command) = cli.command else {
        let mut help = Cli::command();
        help.print_help()?;
        println!();
        return Ok(());
    };

    let client = LcedaClient::new();

    match command {
        Commands::Prompt => {
            println!("{}", prompt_examples(invoked_as));
        }
        Commands::Search { keyword, limit } => {
            let items = client.search_components(&keyword).await?;
            if items.is_empty() {
                println!("No results.");
                return Ok(());
            }

            let count = items.len().min(limit);
            println!("Found {} result(s), showing first {}:", items.len(), count);
            for item in items.iter().take(count) {
                let model_flag = if item.model_uuid.is_some() {
                    "yes"
                } else {
                    "no"
                };
                let lcsc_id = item.lcsc_id().unwrap_or_else(|| "-".to_string());
                let manufacturer = if item.manufacturer.is_empty() {
                    "-"
                } else {
                    item.manufacturer.as_str()
                };
                println!(
                    "[{:>3}] {} | LCSC ID: {} | Manufacturer: {} | 3D model: {}",
                    item.index,
                    item.display_name(),
                    lcsc_id,
                    manufacturer,
                    model_flag
                );
            }
        }
        Commands::Model {
            component,
            input,
            index,
            output,
            parallel,
            force,
            keep_model_logo,
        } => {
            let models = Arc::new(crate::step_model::ModelCleanup::new(!keep_model_logo));
            if let Some(component) = component {
                let item = client.select_item(&component, index).await?;
                let path = download_step(&client, &item, &output, force, &models).await?;
                println!("STEP saved: {}", display_path(&path));
            } else {
                let input = input.ok_or_else(|| {
                    crate::error::AppError::Other(
                        "provide a component ID or use -i <FILE> for multiple models".into(),
                    )
                })?;
                let content = std::fs::read_to_string(&input)?;
                let ids = parse_lcsc_ids(&content);
                export_models(&client, ids, output, parallel, force, models).await?;
            }
        }
        Commands::DownloadObj {
            keyword,
            index,
            output,
            force,
        } => {
            let item = client.select_item(&keyword, index).await?;
            let (obj_path, mtl_path) = download_obj(&client, &item, &output, force).await?;
            println!("OBJ saved: {}", display_path(&obj_path));
            println!("MTL saved: {}", display_path(&mtl_path));
        }
        Commands::Source {
            keyword,
            index,
            output,
            force,
        } => {
            let item = client.select_item(&keyword, index).await?;
            let result = export_easyeda_sources(&client, &item, &output, force).await?;
            if let Some(path) = result.get("symbol") {
                println!("Symbol source saved: {}", display_path(path));
            }
            if let Some(path) = result.get("footprint") {
                println!("Footprint source saved: {}", display_path(path));
            }
        }
        Commands::Altium(command) => run_altium_command(&client, command).await?,
        Commands::Kicad(command) => run_kicad_command(command).await?,
        Commands::Cadence(command) => run_cadence_command(command).await?,
    }

    Ok(())
}

async fn export_models(
    client: &LcedaClient,
    ids: Vec<String>,
    output: std::path::PathBuf,
    parallel: usize,
    force: bool,
    models: Arc<crate::step_model::ModelCleanup>,
) -> Result<()> {
    if ids.is_empty() {
        return Err(crate::error::AppError::Other(
            "no valid LCSC IDs found in model input".into(),
        ));
    }

    let progress = ProgressBar::new(ids.len() as u64);
    progress.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:24.cyan/blue}] {pos}/{len} | {msg}")
            .expect("valid model progress template")
            .progress_chars("##-"),
    );
    progress.enable_steady_tick(Duration::from_millis(250));

    let semaphore = Arc::new(Semaphore::new(parallel.max(1)));
    let mut tasks = JoinSet::new();
    for id in ids {
        let client = client.clone();
        let output = output.clone();
        let semaphore = semaphore.clone();
        let models = Arc::clone(&models);
        tasks.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .expect("model download semaphore should remain open");
            let result = match client.select_item(&id, 1).await {
                Ok(item) => download_step(&client, &item, &output, force, &models).await,
                Err(err) => Err(err),
            };
            (id, result)
        });
    }

    let mut failed_ids = Vec::new();
    let mut saved = 0usize;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((id, Ok(_path))) => {
                saved += 1;
                progress.set_message(format!("saved:{} | last:{}", saved, id));
            }
            Ok((id, Err(_err))) => {
                failed_ids.push(id.clone());
                progress.set_message(format!(
                    "saved:{} failed:{} | last:{}",
                    saved,
                    failed_ids.len(),
                    id
                ));
            }
            Err(err) => {
                failed_ids.push(format!("<task: {err}>"));
            }
        }
        progress.inc(1);
    }
    progress.finish_and_clear();

    println!(
        "STEP batch export complete. Total: {} | Saved: {} | Failed: {}",
        saved + failed_ids.len(),
        saved,
        failed_ids.len()
    );
    if !failed_ids.is_empty() {
        failed_ids.sort();
        println!("Failed IDs: {}", failed_ids.join(", "));
    }
    println!("Output directory: {}", display_path(&output));
    Ok(())
}

async fn run_altium_command(client: &LcedaClient, command: AltiumCommands) -> Result<()> {
    let AltiumCommands {
        component,
        input,
        index,
        output,
        schlib,
        pcblib,
        full,
        append,
        library_name,
        parallel,
        language,
        force,
        keep_model_logo,
    } = command;
    let models = Arc::new(crate::step_model::ModelCleanup::new(!keep_model_logo));
    if append && component.is_some() {
        return Err(crate::error::AppError::Other(
            "--append requires -i because it is intended for multi-component exports".into(),
        ));
    }
    if let Some(component) = component {
        let item = client.select_item(&component, index).await?;
        let targets = AltiumTargets::resolve(schlib, pcblib, full);
        if targets.schlib {
            let path = export_schlib_with_options(
                client,
                &item,
                &output,
                force,
                language.uses_english_metadata(),
                // The same stroke and fill the plain `export_schlib` uses.
                0x68380B,
                0xE0FFFF,
            )
            .await?;
            println!("SchLib saved: {}", display_path(&path));
        }
        if targets.pcblib {
            let path = export_pcblib(client, &item, &output, force, &models).await?;
            println!("PcbLib saved: {}", display_path(&path));
        }
        return Ok(());
    }
    let input = input.ok_or_else(|| {
        crate::error::AppError::Other(
            "provide a component ID or use -i <FILE> for multiple components".into(),
        )
    })?;
    let library_name = match (append, library_name) {
        (true, None) => Some(discover_altium_library_name(&output)?),
        (_, name) => name,
    };
    let summary = export_batch(
        client,
        BatchOptions {
            input,
            output,
            schlib,
            pcblib,
            full: full || (!schlib && !pcblib),
            merge: true,
            append,
            library_name,
            parallel,
            continue_on_error: true,
            english_metadata: language.uses_english_metadata(),
            force,
            models: Arc::clone(&models),
        },
    )
    .await?;
    print_altium_summary(&summary);

    Ok(())
}

async fn run_kicad_command(command: KicadCommands) -> Result<()> {
    let KicadCommands {
        component,
        input,
        symbol,
        footprint,
        model_3d,
        full,
        output,
        append,
        library_name,
        parallel,
        force,
        project_relative,
        language,
        keep_model_logo,
    } = command;
    if append && component.is_some() {
        return Err(crate::error::AppError::Other(
            "--append requires -i because it is intended for multi-component exports".into(),
        ));
    }
    let full = full || (!symbol && !footprint && !model_3d);
    if let Some(component_id) = component {
        let summary = export_kicad_single(KicadSingleOptions {
            lcsc_id: component_id,
            library: KicadLibraryOptions {
                output,
                library_name,
                symbol_lib: None,
                footprint_lib: None,
                model_lib: None,
            },
            export: kicad_export_options(
                symbol,
                footprint,
                model_3d,
                full,
                force,
                project_relative,
            ),
            english_metadata: language.uses_english_metadata(),
            models: Arc::new(crate::step_model::ModelCleanup::new(!keep_model_logo)),
        })
        .await?;
        print_kicad_summary("KiCad export complete", &summary);
        return Ok(());
    }
    let input = input.ok_or_else(|| {
        crate::error::AppError::Other(
            "provide a component ID or use -i <FILE> for multiple components".into(),
        )
    })?;
    let content = std::fs::read_to_string(&input)?;
    let ids = parse_lcsc_ids(&content);
    let (symbol_lib, footprint_lib, model_lib) = if append {
        crate::kicad::discover_append_libraries(&output, library_name.as_deref())?
    } else {
        (None, None, None)
    };
    let summary = export_kicad_batch(KicadBatchOptions {
        ids,
        library: KicadLibraryOptions {
            output,
            library_name,
            symbol_lib,
            footprint_lib,
            model_lib,
        },
        export: kicad_export_options(symbol, footprint, model_3d, full, force, project_relative),
        english_metadata: language.uses_english_metadata(),
        parallel,
        continue_on_error: true,
        append,
        models: Arc::new(crate::step_model::ModelCleanup::new(!keep_model_logo)),
    })
    .await?;
    print_kicad_summary("KiCad export complete", &summary);

    Ok(())
}

async fn run_cadence_command(command: CadenceCommands) -> Result<()> {
    let CadenceCommands {
        component,
        input,
        symbol,
        footprint,
        model_3d,
        full,
        output,
        append,
        library_name,
        parallel,
        force,
        keep_model_logo,
        language,
    } = command;
    if append && component.is_some() {
        return Err(crate::error::AppError::Other(
            "--append requires -i because it is intended for multi-component exports".into(),
        ));
    }
    let ids = match component {
        Some(component_id) => vec![component_id],
        None => {
            let input = input.ok_or_else(|| {
                crate::error::AppError::Other(
                    "provide a component ID or use -i <FILE> for multiple components".into(),
                )
            })?;
            parse_lcsc_ids(&std::fs::read_to_string(&input)?)
        }
    };
    let library_name = library_name.unwrap_or_else(|| {
        output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("cadence_lib")
            .to_string()
    });
    let bar = ProgressBar::new(0);
    bar.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:24.cyan/blue}] {pos}/{len} | {msg}")
            .expect("valid Cadence progress template")
            .progress_chars("##-"),
    );
    bar.enable_steady_tick(Duration::from_millis(250));
    let progress_bar = bar.clone();
    let progress: crate::progress::ExportProgressCallback =
        Arc::new(move |done, total, message| {
            if progress_bar.length() != Some(total as u64) {
                progress_bar.set_length(total as u64);
            }
            progress_bar.set_position(done as u64);
            progress_bar.set_message(message.to_string());
        });

    let summary = export_cadence_batch(CadenceBatchOptions {
        ids,
        output: output.clone(),
        library_name,
        export: CadenceExportOptions {
            symbol,
            footprint,
            model_3d,
            full: full || (!symbol && !footprint && !model_3d),
            force,
        },
        english_metadata: language.uses_english_metadata(),
        parallel,
        continue_on_error: true,
        append,
        models: Arc::new(crate::step_model::ModelCleanup::new(!keep_model_logo)),
        progress: Some(progress),
    })
    .await?;
    bar.finish_and_clear();

    println!(
        "Cadence export complete. Total: {} | Skipped: {} | Success: {} | Failed: {}",
        summary.total, summary.skipped, summary.success, summary.failed
    );
    println!(
        "Symbols: {} | Footprints: {} (from IPC-7352: {}) | 3D models: written {} skipped {} missing {}",
        summary.symbols_written,
        summary.footprints_written,
        summary.footprints_ipc,
        summary.models_written,
        summary.models_skipped,
        summary.models_missing
    );
    println!("Output directory: {}", display_path(&output));
    for file in &summary.generated_files {
        println!("Generated: {}", display_path(file));
    }
    if !summary.failed_ids.is_empty() {
        println!("Failed IDs: {}", summary.failed_ids.join(", "));
    }
    for detail in &summary.failure_details {
        println!("  {detail}");
    }
    for note in &summary.notes {
        println!("Note: {note}");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct AltiumTargets {
    schlib: bool,
    pcblib: bool,
}

impl AltiumTargets {
    fn resolve(schlib: bool, pcblib: bool, full: bool) -> Self {
        let default_full = !schlib && !pcblib && !full;
        let schlib = schlib || full || default_full;
        let pcblib = pcblib || full || default_full;
        Self { schlib, pcblib }
    }
}

fn kicad_export_options(
    symbol: bool,
    footprint: bool,
    model_3d: bool,
    full: bool,
    force: bool,
    project_relative: bool,
) -> KicadExportOptions {
    KicadExportOptions {
        symbol,
        footprint,
        model_3d,
        full,
        force,
        project_relative,
    }
}

fn print_altium_summary(summary: &crate::batch::BatchSummary) {
    println!(
        "Altium batch export complete. Total: {} | Skipped: {} | Success: {} | Failed: {}",
        summary.total, summary.skipped, summary.success, summary.failed
    );
    if !summary.failed_ids.is_empty() {
        println!("Failed IDs: {}", summary.failed_ids.join(", "));
    }
    if !summary.failure_details.is_empty() {
        println!("Failure details:");
        for detail in &summary.failure_details {
            println!("  {detail}");
        }
    }
    for path in &summary.generated_files {
        println!("Generated: {}", display_path(path));
    }
    println!("Output directory: {}", display_path(&summary.output));
}

fn print_kicad_summary(label: &str, summary: &crate::kicad::workflow::KicadBatchSummary) {
    println!(
        "{label}. Total: {} | Skipped: {} | Success: {} | Failed: {}",
        summary.total, summary.skipped, summary.success, summary.failed
    );
    if !summary.failed_ids.is_empty() {
        println!("Failed IDs: {}", summary.failed_ids.join(", "));
    }
    if !summary.failure_details.is_empty() {
        println!("Failure details:");
        for detail in &summary.failure_details {
            println!("  {detail}");
        }
    }
    println!(
        "Stage totals: fetch {} | metadata {} | symbol {} | footprint {} | 3d {}",
        format_duration(summary.timings.component_fetch),
        format_duration(summary.timings.metadata),
        format_duration(summary.timings.symbol),
        format_duration(summary.timings.footprint),
        format_duration(summary.timings.model_3d)
    );
    println!(
        "3D models: written {} | skipped {} | missing {}",
        summary.models.written, summary.models.skipped, summary.models.missing
    );
    for path in &summary.generated_files {
        println!("Generated: {}", display_path(path));
    }
    println!("Output directory: {}", display_path(&summary.output));
}

fn format_duration(duration: Duration) -> String {
    let total_ms = duration.as_millis();
    if total_ms < 60_000 {
        return format!("{}.{:02}s", total_ms / 1_000, (total_ms % 1_000) / 10);
    }

    let total_secs = duration.as_secs();
    let hours = total_secs / 3_600;
    let minutes = (total_secs % 3_600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn display_invocation_name(raw: &OsStr) -> String {
    let file_name = Path::new(raw).file_name().unwrap_or(raw).to_string_lossy();
    let leaf = file_name.rsplit(['/', '\\']).next().unwrap_or(&file_name);

    Path::new(leaf)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("npnp")
        .to_string()
}

fn prompt_examples(invoked_as: &str) -> String {
    let command = if invoked_as.trim().is_empty() {
        "npnp"
    } else {
        invoked_as.trim()
    };

    let color = std::io::stdout().is_terminal();
    let heading = |text: &str| colorize(text, "36;1", color);
    let command_line = |text: String, editable: &[&str]| {
        colorize(&highlight_values(&text, editable, color), "32", color)
    };
    let mut lines = Vec::new();

    lines.push(colorize(
        "Normalize Pin Net Pad (npnp) ready-to-run commands:",
        "34;1",
        color,
    ));
    lines.push(colorize(
        "Yellow values are examples you can change.",
        "90",
        color,
    ));
    lines.push(String::new());
    lines.push(heading("Search a component"));
    lines.push(command_line(
        format!("  {command} search C2040 --limit 5"),
        &["C2040", "5"],
    ));
    lines.push(String::new());
    lines.push(heading("Export Altium libraries"));
    lines.push(command_line(
        format!("  {command} altium C2040 -o altium_libs"),
        &["C2040", "altium_libs"],
    ));
    lines.push(String::new());
    lines.push(heading("Export KiCad libraries"));
    lines.push(command_line(
        format!("  {command} kicad C2040 -o kicad_libs --name MyParts"),
        &["C2040", "kicad_libs", "MyParts"],
    ));
    lines.push(String::new());
    lines.push(heading("Export Cadence sources (OrCAD Capture / Allegro)"));
    lines.push(command_line(
        format!("  {command} cadence C2040 -o cadence_libs --name MyParts"),
        &["C2040", "cadence_libs", "MyParts"],
    ));
    lines.push(String::new());
    lines.push(heading("Download a STEP model"));
    lines.push(command_line(
        format!("  {command} model C2040 -o step_models"),
        &["C2040", "step_models"],
    ));
    lines.push(String::new());
    lines.push(heading("Download multiple STEP models"));
    lines.push(command_line(
        format!("  {command} model -i ids.txt -o step_models -j 42"),
        &["ids.txt", "step_models", "42"],
    ));
    lines.push(String::new());
    lines.push(heading("Export multiple Altium components"));
    lines.push(command_line(
        format!("  {command} altium -i ids.txt -o altium_libs --name MyParts -j 42"),
        &["ids.txt", "altium_libs", "MyParts", "42"],
    ));
    lines.push(String::new());
    lines.push(heading("Append Altium components"));
    lines.push(command_line(
        format!("  {command} altium -i new_ids.txt -o altium_libs --name MyParts --append -j 42"),
        &["new_ids.txt", "altium_libs", "MyParts", "42"],
    ));
    lines.push(String::new());
    lines.push(heading("Export multiple KiCad components"));
    lines.push(command_line(
        format!("  {command} kicad -i ids.txt -o kicad_libs --name MyParts -j 42"),
        &["ids.txt", "kicad_libs", "MyParts", "42"],
    ));
    lines.push(String::new());
    lines.push(heading("Append KiCad components"));
    lines.push(command_line(
        format!("  {command} kicad -i new_ids.txt -o kicad_libs --name MyParts --append -j 42"),
        &["new_ids.txt", "kicad_libs", "MyParts", "42"],
    ));
    lines.push(String::new());
    lines.push(heading("Export multiple Cadence components"));
    lines.push(command_line(
        format!("  {command} cadence -i ids.txt -o cadence_libs --name MyParts -j 42"),
        &["ids.txt", "cadence_libs", "MyParts", "42"],
    ));
    lines.push(String::new());
    lines.push(heading("Append Cadence components"));
    lines.push(command_line(
        format!("  {command} cadence -i new_ids.txt -o cadence_libs --name MyParts --append -j 42"),
        &["new_ids.txt", "cadence_libs", "MyParts", "42"],
    ));
    lines.push(String::new());
    lines.push(heading("Keep the EasyEDA logo on the 3D models"));
    lines.push(command_line(
        format!("  {command} kicad C2040 -o kicad_libs --keep-model-logo"),
        &["C2040", "kicad_libs"],
    ));
    lines.push(String::new());

    lines.join("\n")
}

fn colorize(text: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn highlight_values(text: &str, values: &[&str], enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }

    let mut values = values.to_vec();
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));

    let mut highlighted = String::with_capacity(text.len());
    let mut remaining = text;
    while !remaining.is_empty() {
        let Some(value) = values.iter().find(|value| remaining.starts_with(**value)) else {
            let character = remaining.chars().next().unwrap();
            let next = character.len_utf8();
            highlighted.push(character);
            remaining = &remaining[next..];
            continue;
        };

        highlighted.push_str("\x1b[33;1m");
        highlighted.push_str(value);
        highlighted.push_str("\x1b[32m");
        remaining = &remaining[value.len()..];
    }
    highlighted
}

#[cfg(test)]
mod tests {
    use super::{AltiumTargets, display_invocation_name, highlight_values, prompt_examples};
    use std::ffi::OsStr;

    #[test]
    fn prompt_examples_use_requested_command_name() {
        let text = prompt_examples("npnp");
        assert!(text.contains("Yellow values are examples you can change."));
        assert!(text.contains("npnp search C2040 --limit 5"));
        assert!(text.contains("npnp altium C2040 -o altium_libs"));
        assert!(text.contains("npnp kicad C2040 -o kicad_libs --name MyParts"));
        assert!(text.contains("npnp model -i ids.txt -o step_models -j 42"));
        assert!(text.contains("npnp altium -i ids.txt -o altium_libs --name MyParts -j 42"));
        assert!(
            text.contains(
                "npnp altium -i new_ids.txt -o altium_libs --name MyParts --append -j 42"
            )
        );
        assert!(text.contains("npnp kicad -i ids.txt -o kicad_libs --name MyParts -j 42"));
        assert!(
            text.contains("npnp kicad -i new_ids.txt -o kicad_libs --name MyParts --append -j 42")
        );
        assert!(!text.contains("export-kicad"));
        assert!(!text.contains("batch-kicad"));
    }

    #[test]
    fn highlights_values_without_corrupting_ansi_sequences() {
        let highlighted = highlight_values(
            "model C2040 -o step_models",
            &["C2040", "step_models"],
            true,
        );
        assert_eq!(
            highlighted,
            "model \u{1b}[33;1mC2040\u{1b}[32m -o \u{1b}[33;1mstep_models\u{1b}[32m"
        );
    }

    #[test]
    fn invocation_name_strips_exe_extension() {
        assert_eq!(
            display_invocation_name(OsStr::new(r"C:\tools\npnp.exe")),
            "npnp"
        );
        assert_eq!(
            display_invocation_name(OsStr::new("/usr/local/bin/npnp")),
            "npnp"
        );
    }

    #[test]
    fn altium_single_export_defaults_to_full_target() {
        let targets = AltiumTargets::resolve(false, false, false);
        assert!(targets.schlib);
        assert!(targets.pcblib);
    }

    #[test]
    fn altium_single_export_respects_explicit_target() {
        let targets = AltiumTargets::resolve(true, false, false);
        assert!(targets.schlib);
        assert!(!targets.pcblib);
    }
}
