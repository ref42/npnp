use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "npnp")]
#[command(version)]
#[command(about = "Normalize Pin Net Pad (npnp) - pure Rust EDA library exporter")]
pub struct Cli {
    /// Show ready-to-run example commands
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub prompt: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Search components by keyword
    Search {
        /// Search keyword, e.g. C8755 or TYPE-C
        keyword: String,
        /// Maximum result rows to print
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Search by keyword and download STEP by result index
    DownloadStep {
        /// Search keyword or component ID
        keyword: String,
        /// Result index to download from the search results
        #[arg(long, default_value_t = 1)]
        index: usize,
        /// Output directory for the STEP model
        #[arg(long, default_value = "step")]
        output: PathBuf,
        /// Replace an existing downloaded file
        #[arg(long)]
        force: bool,
    },
    /// Search by keyword and download OBJ/MTL by result index
    DownloadObj {
        /// Search keyword or component ID
        keyword: String,
        /// Result index to download from the search results
        #[arg(long, default_value_t = 1)]
        index: usize,
        /// Output directory for the OBJ/MTL files
        #[arg(long, default_value = "obj")]
        output: PathBuf,
        /// Replace existing downloaded files
        #[arg(long)]
        force: bool,
    },
    /// Export upstream symbol / footprint JSON sources only
    Source {
        /// Search keyword or component ID
        keyword: String,
        /// Result index to export from the search results
        #[arg(long, default_value_t = 1)]
        index: usize,
        /// Output directory for source JSON files
        #[arg(long, default_value = "sources")]
        output: PathBuf,
        /// Replace existing source JSON files
        #[arg(long)]
        force: bool,
    },
    /// Export source JSON plus STEP plus manifest
    Bundle {
        /// Search keyword or component ID
        keyword: String,
        /// Result index to export from the search results
        #[arg(long, default_value_t = 1)]
        index: usize,
        /// Output directory for the bundle
        #[arg(long, default_value = "bundle")]
        output: PathBuf,
        /// Replace existing bundle files
        #[arg(long)]
        force: bool,
    },
    /// Export Altium libraries
    Altium {
        #[command(subcommand)]
        command: AltiumCommands,
    },
    /// Export KiCad libraries
    Kicad {
        #[command(subcommand)]
        command: KicadCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum AltiumCommands {
    /// Export one Altium library set
    Export {
        /// Search keyword or component ID
        component: String,
        #[arg(long, default_value_t = 1)]
        index: usize,
        #[arg(long, short = 'o', default_value = "altium")]
        output: PathBuf,
        /// Export schematic library (.SchLib)
        #[arg(long)]
        schlib: bool,
        /// Export PCB footprint library (.PcbLib)
        #[arg(long)]
        pcblib: bool,
        /// Export schematic and PCB libraries
        #[arg(long)]
        full: bool,
        /// Prefer English metadata when available
        #[arg(long)]
        english_metadata: bool,
        /// Replace existing generated outputs
        #[arg(long)]
        force: bool,
    },
    /// Batch export Altium libraries from a text file of component IDs
    Batch {
        /// Text file containing component IDs to export
        #[arg(long, short = 'i', value_name = "FILE")]
        input: PathBuf,
        /// Output directory for generated Altium libraries
        #[arg(long, short = 'o', default_value = "altium")]
        output: PathBuf,
        /// Export schematic libraries (.SchLib)
        #[arg(long)]
        schlib: bool,
        /// Export PCB footprint libraries (.PcbLib)
        #[arg(long)]
        pcblib: bool,
        /// Export schematic and PCB libraries
        #[arg(long)]
        full: bool,
        /// Write one merged library pair instead of one file per component
        #[arg(long)]
        merge: bool,
        /// Append missing components to an existing merged library
        #[arg(long)]
        append: bool,
        /// Merged library name
        #[arg(long, value_name = "NAME")]
        library_name: Option<String>,
        /// Maximum number of non-merged exports to run at once
        #[arg(long, default_value_t = 4)]
        parallel: usize,
        /// Keep exporting remaining components after a failed component
        #[arg(long)]
        continue_on_error: bool,
        /// Prefer English metadata when available
        #[arg(long)]
        english_metadata: bool,
        /// Replace existing generated outputs
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum KicadCommands {
    /// Export one KiCad library set
    Export {
        /// Component ID, e.g. C2040
        component_id: String,
        /// Export symbol only
        #[arg(long)]
        symbol: bool,
        /// Export footprint only
        #[arg(long)]
        footprint: bool,
        /// Export 3D STEP model only
        #[arg(long = "3d")]
        model_3d: bool,
        /// Export symbol, footprint, and 3D model
        #[arg(long)]
        full: bool,
        #[arg(long, short = 'o', default_value = ".")]
        output: PathBuf,
        /// Base library name used under --output when explicit library targets are not provided
        #[arg(long, value_name = "NAME")]
        library_name: Option<String>,
        /// Existing symbol library file to append/update (.kicad_sym)
        #[arg(long, value_name = "FILE")]
        symbol_lib: Option<PathBuf>,
        /// Existing footprint library directory to append/update (.pretty)
        #[arg(long, value_name = "DIR")]
        footprint_lib: Option<PathBuf>,
        /// Existing 3D model library directory to append/update (.3dshapes)
        #[arg(long, value_name = "DIR")]
        model_lib: Option<PathBuf>,
        /// Replace existing KiCad symbol/footprint/3D outputs
        #[arg(long)]
        force: bool,
        /// Use project-relative KIPRJMOD paths for 3D model references
        #[arg(long)]
        project_relative: bool,
        /// Prefer English metadata when available
        #[arg(long)]
        english_metadata: bool,
    },
    /// Batch export KiCad libraries from a text file of component IDs
    Batch {
        /// Text file containing component IDs to export
        #[arg(long, short = 'i', value_name = "FILE")]
        input: PathBuf,
        /// Export symbols only
        #[arg(long)]
        symbol: bool,
        /// Export footprints only
        #[arg(long)]
        footprint: bool,
        /// Export 3D STEP models only
        #[arg(long = "3d")]
        model_3d: bool,
        /// Export symbols, footprints, and 3D models
        #[arg(long)]
        full: bool,
        /// Output directory for generated KiCad libraries
        #[arg(long, short = 'o', default_value = ".")]
        output: PathBuf,
        /// Base library name used under --output when explicit library targets are not provided
        #[arg(long, value_name = "NAME")]
        library_name: Option<String>,
        /// Existing symbol library file to append/update (.kicad_sym)
        #[arg(long, value_name = "FILE")]
        symbol_lib: Option<PathBuf>,
        /// Existing footprint library directory to append/update (.pretty)
        #[arg(long, value_name = "DIR")]
        footprint_lib: Option<PathBuf>,
        /// Existing 3D model library directory to append/update (.3dshapes)
        #[arg(long, value_name = "DIR")]
        model_lib: Option<PathBuf>,
        /// Maximum number of exports to run at once
        #[arg(long, default_value_t = 4)]
        parallel: usize,
        /// Keep exporting remaining components after a failed component
        #[arg(long)]
        continue_on_error: bool,
        /// Replace existing KiCad symbol/footprint/3D outputs
        #[arg(long)]
        force: bool,
        /// Use project-relative KIPRJMOD paths for 3D model references
        #[arg(long)]
        project_relative: bool,
        /// Prefer English metadata when available
        #[arg(long)]
        english_metadata: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KicadVersion {
    V5,
    V6,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_without_subcommand() {
        let cli = Cli::try_parse_from(["npnp", "--prompt"]).expect("prompt flag should parse");
        assert!(cli.prompt);
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_search_command_with_optional_subcommand_field() {
        let cli =
            Cli::try_parse_from(["npnp", "search", "C2040"]).expect("search command should parse");
        assert!(!cli.prompt);
        let Some(Commands::Search { keyword, limit }) = cli.command else {
            panic!("expected search command");
        };
        assert_eq!(keyword, "C2040");
        assert_eq!(limit, 20);
    }

    #[test]
    fn parses_altium_export_with_english_metadata() {
        let cli = Cli::try_parse_from([
            "npnp",
            "altium",
            "export",
            "C2927505",
            "--schlib",
            "--english-metadata",
        ])
        .expect("altium export command should parse");
        let Some(Commands::Altium {
            command:
                AltiumCommands::Export {
                    component,
                    schlib,
                    english_metadata,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected altium export command");
        };
        assert_eq!(component, "C2927505");
        assert!(schlib);
        assert!(english_metadata);
    }

    #[test]
    fn parses_kicad_export_full_command() {
        let cli = Cli::try_parse_from([
            "npnp",
            "kicad",
            "export",
            "C2040",
            "--full",
            "--output",
            "out",
            "--library-name",
            "MyParts",
            "--english-metadata",
        ])
        .expect("kicad export command should parse");
        let Some(Commands::Kicad {
            command:
                KicadCommands::Export {
                    component_id,
                    full,
                    output,
                    library_name,
                    english_metadata,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected kicad export command");
        };
        assert_eq!(component_id, "C2040");
        assert!(full);
        assert_eq!(output, PathBuf::from("out"));
        assert_eq!(library_name.as_deref(), Some("MyParts"));
        assert!(english_metadata);
    }

    #[test]
    fn parses_kicad_batch_3d_command() {
        let cli = Cli::try_parse_from([
            "npnp",
            "kicad",
            "batch",
            "--input",
            "ids.txt",
            "--3d",
            "--parallel",
            "8",
        ])
        .expect("kicad batch command should parse");
        let Some(Commands::Kicad {
            command:
                KicadCommands::Batch {
                    input,
                    model_3d,
                    parallel,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected kicad batch command");
        };
        assert_eq!(input, PathBuf::from("ids.txt"));
        assert!(model_3d);
        assert_eq!(parallel, 8);
    }

    #[test]
    fn rejects_removed_flat_export_commands() {
        assert!(Cli::try_parse_from(["npnp", "export-kicad", "C2040", "--full"]).is_err());
        assert!(Cli::try_parse_from(["npnp", "export-schlib", "C2040"]).is_err());
        assert!(Cli::try_parse_from(["npnp", "batch-kicad", "--input", "ids.txt"]).is_err());
    }

    #[test]
    fn rejects_removed_option_aliases() {
        assert!(
            Cli::try_parse_from([
                "npnp",
                "kicad",
                "export",
                "C2040",
                "--full",
                "--lib-name",
                "Old",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from(["npnp", "kicad", "export", "C2040", "--full", "--overwrite",])
                .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "npnp",
                "altium",
                "export",
                "C2040",
                "--schlib",
                "--lcsc-english",
            ])
            .is_err()
        );
    }
}
