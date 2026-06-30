use std::ffi::OsStr;
use std::path::Path;

use clap::{CommandFactory, Parser};

use crate::altium::workflow::{
    download_obj, download_step, export_bundle, export_easyeda_sources, export_pcblib,
    export_schlib_with_options,
};
use crate::batch::{BatchOptions, export_batch, parse_lcsc_ids};
use crate::cli::{AltiumCommands, Cli, Commands, KicadCommands};
use crate::error::Result;
use crate::kicad::{
    KicadBatchOptions, KicadExportOptions, KicadLibraryOptions, KicadSingleOptions,
    export_kicad_batch, export_kicad_single,
};
use crate::lceda::LcedaClient;

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
    if cli.prompt {
        println!("{}", prompt_examples(invoked_as));
        return Ok(());
    }

    let Some(command) = cli.command else {
        let mut help = Cli::command();
        help.print_help()?;
        println!();
        return Ok(());
    };

    let client = LcedaClient::new();

    match command {
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
        Commands::DownloadStep {
            keyword,
            index,
            output,
            force,
        } => {
            let item = client.select_item(&keyword, index).await?;
            let path = download_step(&client, &item, &output, force).await?;
            println!("STEP saved: {}", path.display());
        }
        Commands::DownloadObj {
            keyword,
            index,
            output,
            force,
        } => {
            let item = client.select_item(&keyword, index).await?;
            let (obj_path, mtl_path) = download_obj(&client, &item, &output, force).await?;
            println!("OBJ saved: {}", obj_path.display());
            println!("MTL saved: {}", mtl_path.display());
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
                println!("Symbol source saved: {}", path.display());
            }
            if let Some(path) = result.get("footprint") {
                println!("Footprint source saved: {}", path.display());
            }
        }
        Commands::Bundle {
            keyword,
            index,
            output,
            force,
        } => {
            let item = client.select_item(&keyword, index).await?;
            let result = export_bundle(&client, &item, &output, force).await?;
            if let Some(path) = result.get("manifest") {
                println!("Bundle manifest saved: {}", path.display());
            }
            if let Some(path) = result.get("symbol") {
                println!("Symbol source saved: {}", path.display());
            }
            if let Some(path) = result.get("footprint") {
                println!("Footprint source saved: {}", path.display());
            }
            if let Some(path) = result.get("step") {
                println!("STEP saved: {}", path.display());
            }
        }
        Commands::Altium { command } => run_altium_command(&client, command).await?,
        Commands::Kicad { command } => run_kicad_command(command).await?,
    }

    Ok(())
}

async fn run_altium_command(client: &LcedaClient, command: AltiumCommands) -> Result<()> {
    match command {
        AltiumCommands::Export {
            component,
            index,
            output,
            schlib,
            pcblib,
            full,
            english_metadata,
            force,
        } => {
            let item = client.select_item(&component, index).await?;
            let targets = AltiumTargets::resolve(schlib, pcblib, full);
            if targets.schlib {
                let path =
                    export_schlib_with_options(client, &item, &output, force, english_metadata)
                        .await?;
                println!("SchLib saved: {}", path.display());
            }
            if targets.pcblib {
                let path = export_pcblib(client, &item, &output, force).await?;
                println!("PcbLib saved: {}", path.display());
            }
        }
        AltiumCommands::Batch {
            input,
            output,
            schlib,
            pcblib,
            full,
            merge,
            append,
            library_name,
            parallel,
            continue_on_error,
            english_metadata,
            force,
        } => {
            let summary = export_batch(
                client,
                BatchOptions {
                    input,
                    output,
                    schlib,
                    pcblib,
                    full,
                    merge,
                    append,
                    library_name,
                    parallel,
                    continue_on_error,
                    english_metadata,
                    force,
                },
            )
            .await?;
            print_altium_summary(&summary);
        }
    }

    Ok(())
}

async fn run_kicad_command(command: KicadCommands) -> Result<()> {
    match command {
        KicadCommands::Export {
            component_id,
            symbol,
            footprint,
            model_3d,
            full,
            output,
            library_name,
            symbol_lib,
            footprint_lib,
            model_lib,
            force,
            project_relative,
            english_metadata,
        } => {
            let summary = export_kicad_single(KicadSingleOptions {
                lcsc_id: component_id,
                library: KicadLibraryOptions {
                    output,
                    library_name,
                    symbol_lib,
                    footprint_lib,
                    model_lib,
                },
                export: kicad_export_options(
                    symbol,
                    footprint,
                    model_3d,
                    full,
                    force,
                    project_relative,
                ),
                english_metadata,
            })
            .await?;
            print_kicad_summary("KiCad export complete", &summary);
        }
        KicadCommands::Batch {
            input,
            symbol,
            footprint,
            model_3d,
            full,
            output,
            library_name,
            symbol_lib,
            footprint_lib,
            model_lib,
            parallel,
            continue_on_error,
            force,
            project_relative,
            english_metadata,
        } => {
            let content = std::fs::read_to_string(&input)?;
            let ids = parse_lcsc_ids(&content);
            let summary = export_kicad_batch(KicadBatchOptions {
                ids,
                library: KicadLibraryOptions {
                    output,
                    library_name,
                    symbol_lib,
                    footprint_lib,
                    model_lib,
                },
                export: kicad_export_options(
                    symbol,
                    footprint,
                    model_3d,
                    full,
                    force,
                    project_relative,
                ),
                english_metadata,
                parallel,
                continue_on_error,
            })
            .await?;
            print_kicad_summary("KiCad batch export complete", &summary);
        }
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
        kicad_version: crate::cli::KicadVersion::V6,
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
    for path in &summary.generated_files {
        println!("Generated: {}", path.display());
    }
    println!("Output directory: {}", summary.output.display());
}

fn print_kicad_summary(label: &str, summary: &crate::kicad::workflow::KicadBatchSummary) {
    println!(
        "{label}. Total: {} | Success: {} | Failed: {}",
        summary.total, summary.success, summary.failed
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
        println!("Generated: {}", path.display());
    }
    println!("Output directory: {}", summary.output.display());
}

fn display_invocation_name(raw: &OsStr) -> String {
    Path::new(raw)
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

    [
        "Normalize Pin Net Pad (npnp) ready-to-run commands:",
        "",
        "Search a component",
        &format!("  {command} search C2040 --limit 5"),
        "",
        "Export one Altium library set",
        &format!("  {command} altium export C2040 --full --output altium-libs --force"),
        "",
        "Export one KiCad library set",
        &format!(
            "  {command} kicad export C2040 --full --output kicad-libs --library-name MyParts --force"
        ),
        "",
        "Export source JSON plus STEP bundle",
        &format!("  {command} bundle C2040 --index 1 --output bundle --force"),
        "",
        "Batch export Altium merged libraries",
        &format!(
            "  {command} altium batch --input ids.txt --output generated\\altium --merge --library-name MyLib --full --force --continue-on-error"
        ),
        "",
        "Append new parts into an existing Altium merged library",
        &format!(
            "  {command} altium batch --input new_ids.txt --output generated\\altium --merge --append --library-name MyLib --full --force --continue-on-error"
        ),
        "",
        "Batch export KiCad libraries",
        &format!(
            "  {command} kicad batch --input ids.txt --output generated\\kicad --library-name MyParts --full --force --parallel 4 --continue-on-error"
        ),
        "",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{AltiumTargets, display_invocation_name, prompt_examples};
    use std::ffi::OsStr;

    #[test]
    fn prompt_examples_use_requested_command_name() {
        let text = prompt_examples("npnp");
        assert!(text.contains("npnp search C2040 --limit 5"));
        assert!(text.contains("npnp altium export C2040 --full --output altium-libs --force"));
        assert!(text.contains("npnp kicad batch --input ids.txt"));
        assert!(!text.contains("export-kicad"));
        assert!(!text.contains("batch-kicad"));
    }

    #[test]
    fn invocation_name_strips_exe_extension() {
        assert_eq!(
            display_invocation_name(OsStr::new(r"C:\tools\npnp.exe")),
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
