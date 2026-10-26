use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "npnp")]
#[command(version)]
#[command(
    about = "Normalize Pin Net Pad (npnp) - pure Rust EDA library exporter, @Ref42,https://github.com/ref42/npnp"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MetadataLanguage {
    /// Prefer source metadata and use English metadata when explicitly available.
    Auto,
    /// Prefer English LCSC metadata, falling back to source metadata.
    En,
    /// Use source metadata, which is typically Chinese, without fetching English metadata.
    Zh,
}

impl MetadataLanguage {
    pub fn uses_english_metadata(self) -> bool {
        matches!(self, Self::En)
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Show ready-to-run workflow commands
    #[command(name = "p")]
    Prompt,

    /// Search components by keyword
    Search {
        /// Search keyword, e.g. C8755 or TYPE-C
        keyword: String,
        /// Maximum result rows to print
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Download one or more 3D STEP models
    #[command(name = "model")]
    Model {
        /// Component ID or search keyword (use -i for multiple components)
        component: Option<String>,
        /// Read component IDs from any plain-text file
        #[arg(long, short = 'i', value_name = "FILE", conflicts_with = "component")]
        input: Option<PathBuf>,
        /// Search result index for a component keyword
        #[arg(long, default_value_t = 1)]
        index: usize,
        /// Output directory for STEP models
        #[arg(long, short = 'o', default_value = "step_models")]
        output: PathBuf,
        /// Maximum number of concurrent model downloads
        #[arg(short = 'j', default_value_t = 4)]
        parallel: usize,
        /// Replace existing STEP files
        #[arg(long)]
        force: bool,
        /// Keep the EasyEDA logo on downloaded 3D models (it is removed by default)
        #[arg(long)]
        keep_model_logo: bool,
    },
    /// Search by keyword and download OBJ/MTL by result index
    DownloadObj {
        /// Search keyword or component ID
        keyword: String,
        /// Result index to download from the search results
        #[arg(long, default_value_t = 1)]
        index: usize,
        /// Output directory for the OBJ/MTL files
        #[arg(long, short = 'o', default_value = "obj")]
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
        #[arg(long, short = 'o', default_value = "sources")]
        output: PathBuf,
        /// Replace existing source JSON files
        #[arg(long)]
        force: bool,
    },
    /// Export Altium libraries from one component or an ID file
    Altium(AltiumCommands),
    /// Export KiCad libraries from one component or an ID file
    Kicad(KicadCommands),
    /// Export Cadence (OrCAD Capture / Allegro) sources from one component or an ID file
    Cadence(CadenceCommands),
    /// Merge existing Altium SchLib/PcbLib files from a directory
    Merge {
        /// Directory containing the libraries to merge
        #[arg(long, short = 'i', value_name = "DIR")]
        input: PathBuf,
        /// Output library base path (extensions are added automatically)
        #[arg(long, short = 'o', value_name = "NAME")]
        output: PathBuf,
        /// Replace output libraries that already exist
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args)]
pub struct AltiumCommands {
    /// Component ID or search keyword (use -i for multiple components)
    pub component: Option<String>,
    /// Text file containing component IDs to export
    #[arg(long, short = 'i', value_name = "FILE", conflicts_with = "component")]
    pub input: Option<PathBuf>,
    /// Search result index for a component keyword
    #[arg(long, default_value_t = 1)]
    pub index: usize,
    /// Output directory for Altium libraries
    #[arg(long, short = 'o', default_value = "altium_libs")]
    pub output: PathBuf,
    /// Export schematic library (.SchLib) only
    #[arg(long)]
    pub schlib: bool,
    /// Export PCB footprint library (.PcbLib) only
    #[arg(long)]
    pub pcblib: bool,
    /// Export every target for this EDA tool (the default when none is given)
    #[arg(long)]
    pub full: bool,
    /// Append components to existing libraries in the output directory
    #[arg(long)]
    pub append: bool,
    /// Library base name (defaults from the output directory)
    #[arg(long = "name", value_name = "NAME")]
    pub library_name: Option<String>,
    /// Maximum number of component exports to run at once
    #[arg(short = 'j', default_value_t = 4)]
    pub parallel: usize,
    /// Metadata language: auto, en, or zh
    #[arg(long, value_enum, default_value_t = MetadataLanguage::Auto)]
    pub language: MetadataLanguage,
    /// Replace existing generated outputs
    #[arg(long)]
    pub force: bool,
    /// Keep the EasyEDA logo on downloaded 3D models (it is removed by default)
    #[arg(long)]
    pub keep_model_logo: bool,
}

#[derive(Debug, Args)]
pub struct KicadCommands {
    /// Component ID or search keyword (use -i for multiple components)
    pub component: Option<String>,
    /// Text file containing component IDs to export
    #[arg(long, short = 'i', value_name = "FILE", conflicts_with = "component")]
    pub input: Option<PathBuf>,
    /// Export symbols only
    #[arg(long)]
    pub symbol: bool,
    /// Export footprints only
    #[arg(long)]
    pub footprint: bool,
    /// Export 3D STEP models only
    #[arg(long = "3d")]
    pub model_3d: bool,
    /// Export every target for this EDA tool (the default when none is given)
    #[arg(long)]
    pub full: bool,
    /// Output directory for KiCad libraries
    #[arg(long, short = 'o', default_value = "kicad_libs")]
    pub output: PathBuf,
    /// Append components to existing libraries in the output directory
    #[arg(long)]
    pub append: bool,
    /// Library base name (defaults from the output directory)
    #[arg(long = "name", value_name = "NAME")]
    pub library_name: Option<String>,
    /// Maximum number of component exports to run at once
    #[arg(short = 'j', default_value_t = 4)]
    pub parallel: usize,
    /// Replace existing KiCad symbol/footprint/3D outputs
    #[arg(long)]
    pub force: bool,
    /// Use project-relative KIPRJMOD paths for 3D model references
    #[arg(long)]
    pub project_relative: bool,
    /// Metadata language: auto, en, or zh
    #[arg(long, value_enum, default_value_t = MetadataLanguage::Auto)]
    pub language: MetadataLanguage,
    /// Keep the EasyEDA logo on downloaded 3D models (it is removed by default)
    #[arg(long)]
    pub keep_model_logo: bool,
}

#[derive(Debug, Args)]
pub struct CadenceCommands {
    /// Component ID or search keyword (use -i for multiple components)
    pub component: Option<String>,
    /// Text file containing component IDs to export
    #[arg(long, short = 'i', value_name = "FILE", conflicts_with = "component")]
    pub input: Option<PathBuf>,
    /// Export Capture symbols only
    #[arg(long)]
    pub symbol: bool,
    /// Export Allegro footprints only
    #[arg(long)]
    pub footprint: bool,
    /// Export 3D STEP models only
    #[arg(long = "3d")]
    pub model_3d: bool,
    /// Export everything: symbols, footprints and 3D models
    #[arg(long)]
    pub full: bool,
    /// Output directory for the Cadence sources
    #[arg(long, short = 'o', default_value = "cadence_libs")]
    pub output: PathBuf,
    /// Add to the library already in the output directory
    #[arg(long)]
    pub append: bool,
    /// Library base name (defaults from the output directory)
    #[arg(long = "name", value_name = "NAME")]
    pub library_name: Option<String>,
    /// Maximum number of component exports to run at once
    #[arg(short = 'j', default_value_t = 4)]
    pub parallel: usize,
    /// Replace existing outputs
    #[arg(long)]
    pub force: bool,
    /// Keep the EasyEDA logo on downloaded 3D models (it is removed by default)
    #[arg(long)]
    pub keep_model_logo: bool,
    /// Metadata language: auto, en, or zh
    #[arg(long, value_enum, default_value_t = MetadataLanguage::Auto)]
    pub language: MetadataLanguage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_subcommand() {
        let cli = Cli::try_parse_from(["npnp", "p"]).expect("prompt command should parse");
        assert!(matches!(cli.command, Some(Commands::Prompt)));
    }

    #[test]
    fn parses_search_command_with_optional_subcommand_field() {
        let cli =
            Cli::try_parse_from(["npnp", "search", "C2040"]).expect("search command should parse");
        let Some(Commands::Search { keyword, limit }) = cli.command else {
            panic!("expected search command");
        };
        assert_eq!(keyword, "C2040");
        assert_eq!(limit, 20);
    }

    #[test]
    fn rejects_removed_prompt_flag() {
        assert!(Cli::try_parse_from(["npnp", "--prompt"]).is_err());
    }

    #[test]
    fn parses_altium_single_with_metadata_language() {
        let cli =
            Cli::try_parse_from(["npnp", "altium", "C2927505", "--schlib", "--language", "en"])
                .expect("altium export command should parse");
        let Some(Commands::Altium(AltiumCommands {
            component,
            schlib,
            language,
            input,
            ..
        })) = cli.command
        else {
            panic!("expected altium command");
        };
        assert_eq!(component.as_deref(), Some("C2927505"));
        assert!(input.is_none());
        assert!(schlib);
        assert_eq!(language, MetadataLanguage::En);
    }

    #[test]
    fn parses_kicad_single_command() {
        let cli = Cli::try_parse_from([
            "npnp",
            "kicad",
            "C2040",
            "-o",
            "out",
            "--name",
            "MyParts",
            "--language",
            "en",
        ])
        .expect("kicad export command should parse");
        let Some(Commands::Kicad(KicadCommands {
            component,
            output,
            library_name,
            language,
            ..
        })) = cli.command
        else {
            panic!("expected kicad command");
        };
        assert_eq!(component.as_deref(), Some("C2040"));
        assert_eq!(output, PathBuf::from("out"));
        assert_eq!(library_name.as_deref(), Some("MyParts"));
        assert_eq!(language, MetadataLanguage::En);
    }

    #[test]
    fn parses_kicad_input_file_command() {
        let cli = Cli::try_parse_from(["npnp", "kicad", "-i", "ids.txt", "--3d", "-j", "42"])
            .expect("kicad input command should parse");
        let Some(Commands::Kicad(KicadCommands {
            input,
            model_3d,
            parallel,
            component,
            ..
        })) = cli.command
        else {
            panic!("expected kicad command");
        };
        assert_eq!(input, Some(PathBuf::from("ids.txt")));
        assert!(component.is_none());
        assert!(model_3d);
        assert_eq!(parallel, 42);
    }

    #[test]
    fn parses_merge_directory_command() {
        let cli = Cli::try_parse_from([
            "npnp",
            "merge",
            "-i",
            "libraries",
            "-o",
            "MyParts",
            "--force",
        ])
        .expect("merge command should parse");
        let Some(Commands::Merge {
            input,
            output,
            force,
        }) = cli.command
        else {
            panic!("expected merge command");
        };
        assert_eq!(input, PathBuf::from("libraries"));
        assert_eq!(output, PathBuf::from("MyParts"));
        assert!(force);
    }

    #[test]
    fn rejects_parallel_long_option() {
        assert!(
            Cli::try_parse_from(["npnp", "kicad", "-i", "ids.txt", "--3d", "--parallel", "8",])
                .is_err()
        );
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
