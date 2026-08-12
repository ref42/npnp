use crate::error::{AppError, KicadError, Result};
use crate::kicad::KicadLibraryOptions;
use crate::util::display_path;
use regex::Regex;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static SYMBOL_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn discover_append_libraries(
    directory: &Path,
    library_name: Option<&str>,
) -> Result<(Option<PathBuf>, Option<PathBuf>, Option<PathBuf>)> {
    if !directory.is_dir() {
        return Err(AppError::Other(format!(
            "KiCad append directory does not exist: {}",
            display_path(directory)
        )));
    }

    let requested = library_name.map(str::trim).filter(|name| !name.is_empty());
    let mut symbols = Vec::new();
    let mut footprints = Vec::new();
    let mut models = Vec::new();
    for entry in fs::read_dir(directory).map_err(KicadError::Io)? {
        let path = entry.map_err(KicadError::Io)?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".kicad_sym") {
            if requested.is_none_or(|value| value == stem) {
                symbols.push(path);
            }
        } else if let Some(stem) = name.strip_suffix(".pretty") {
            if path.is_dir() && requested.is_none_or(|value| value == stem) {
                footprints.push(path);
            }
        } else if let Some(stem) = name.strip_suffix(".3dshapes")
            && path.is_dir()
            && requested.is_none_or(|value| value == stem)
        {
            models.push(path);
        }
    }

    fn select_one(kind: &str, paths: &mut Vec<PathBuf>) -> Result<Option<PathBuf>> {
        match paths.len() {
            0 => Ok(None),
            1 => Ok(paths.pop()),
            _ => Err(AppError::Other(format!(
                "KiCad append directory contains multiple {kind} libraries: {}. Use --name to select one.",
                paths
                    .iter()
                    .map(|path| display_path(path))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    let symbol = select_one("symbol", &mut symbols)?;
    let footprint = select_one("footprint", &mut footprints)?;
    let model = select_one("3D model", &mut models)?;
    if symbol.is_none() && footprint.is_none() && model.is_none() {
        return Err(AppError::Other(format!(
            "KiCad append directory contains no .kicad_sym, .pretty, or .3dshapes libraries: {}",
            display_path(directory)
        )));
    }
    Ok((symbol, footprint, model))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolWriteStatus {
    Added,
    Updated,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileWriteStatus {
    Written,
    Skipped,
}

pub struct LibraryManager {
    output_path: PathBuf,
    lib_name: String,
    symbol_lib_override: Option<PathBuf>,
    footprint_lib_dir: PathBuf,
    footprint_lib_name: String,
    model_lib_dir: PathBuf,
    model_lib_name: String,
    model_dir_name: String,
}

impl LibraryManager {
    pub fn new(output_path: &Path) -> Self {
        let lib_name = output_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("npnp")
            .to_string();
        let footprint_lib_dir = output_path.join(format!("{}.pretty", lib_name));
        let model_dir_name = format!("{}.3dshapes", lib_name);
        let model_lib_dir = output_path.join(&model_dir_name);
        Self {
            output_path: output_path.to_path_buf(),
            lib_name: lib_name.clone(),
            symbol_lib_override: None,
            footprint_lib_dir,
            footprint_lib_name: lib_name.clone(),
            model_lib_dir,
            model_lib_name: lib_name,
            model_dir_name,
        }
    }

    pub fn from_options(args: &KicadLibraryOptions) -> Result<Self> {
        let lib_name = resolved_lib_name(args);
        let footprint_lib_dir = args
            .footprint_lib
            .clone()
            .unwrap_or_else(|| args.output.join(format!("{}.pretty", lib_name)));
        let model_lib_dir = args
            .model_lib
            .clone()
            .unwrap_or_else(|| args.output.join(format!("{}.3dshapes", lib_name)));
        let footprint_lib_name = Self::library_name_from_path(&footprint_lib_dir, ".pretty")?;
        let model_dir_name = Self::path_name(&model_lib_dir)?;
        let model_lib_name = Self::strip_required_suffix(&model_dir_name, ".3dshapes")?.to_string();

        Ok(Self {
            output_path: args.output.clone(),
            lib_name,
            symbol_lib_override: args.symbol_lib.clone(),
            footprint_lib_dir,
            footprint_lib_name,
            model_lib_dir,
            model_lib_name,
            model_dir_name,
        })
    }

    pub fn lib_name(&self) -> &str {
        &self.lib_name
    }

    pub fn footprint_lib_name(&self) -> &str {
        &self.footprint_lib_name
    }

    pub fn model_lib_name(&self) -> &str {
        &self.model_lib_name
    }

    pub fn model_dir_name(&self) -> &str {
        &self.model_dir_name
    }

    pub fn footprint_lib_dir(&self) -> &Path {
        &self.footprint_lib_dir
    }

    pub fn model_lib_dir(&self) -> &Path {
        &self.model_lib_dir
    }

    /// Create necessary output directories
    pub fn create_directories(&self) -> Result<()> {
        if let Some(symbol_lib_path) = &self.symbol_lib_override {
            if let Some(parent) = symbol_lib_path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(KicadError::Io)?;
            }
        } else {
            fs::create_dir_all(&self.output_path).map_err(KicadError::Io)?;
        }

        fs::create_dir_all(&self.footprint_lib_dir).map_err(KicadError::Io)?;

        fs::create_dir_all(&self.model_lib_dir).map_err(KicadError::Io)?;

        Ok(())
    }

    /// Check if a component exists in the library file
    /// Note: This should only be called within a lock if used for write decisions
    pub fn component_exists(&self, lib_path: &Path, component_name: &str) -> Result<bool> {
        if !lib_path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(lib_path).map_err(KicadError::Io)?;

        // Check for v6 format
        let v6_pattern = format!(r#"\(symbol\s+"{}""#, regex::escape(component_name));
        if let Ok(re) = Regex::new(&v6_pattern)
            && re.is_match(&content)
        {
            return Ok(true);
        }

        // Check for v5 format
        let v5_pattern = format!(r"DEF\s+{}\s+", regex::escape(component_name));
        if let Ok(re) = Regex::new(&v5_pattern)
            && re.is_match(&content)
        {
            return Ok(true);
        }

        Ok(false)
    }

    /// Add or update a component in the library file (thread-safe)
    pub fn add_or_update_component(
        &self,
        lib_path: &Path,
        component_name: &str,
        component_data: &str,
        overwrite: bool,
    ) -> Result<SymbolWriteStatus> {
        // Lock to prevent concurrent writes and check-then-act race conditions
        let _lock = SYMBOL_WRITE_LOCK.lock().unwrap();

        // Check if component exists (within lock to prevent TOCTOU)
        let exists = if lib_path.exists() {
            let content = fs::read_to_string(lib_path).map_err(KicadError::Io)?;

            let v6_pattern = format!(r#"\(symbol\s+"{}""#, regex::escape(component_name));
            if let Ok(re) = Regex::new(&v6_pattern) {
                re.is_match(&content)
            } else {
                false
            }
        } else {
            false
        };

        if exists && overwrite {
            // Update existing component
            self.update_component_internal(lib_path, component_name, component_data)?;
            return Ok(SymbolWriteStatus::Updated);
        } else if !exists {
            // Add new component
            self.add_component_internal(lib_path, component_data)?;
            return Ok(SymbolWriteStatus::Added);
        }
        // If exists and !overwrite, do nothing

        Ok(SymbolWriteStatus::Skipped)
    }

    /// Internal add component (assumes lock is held)
    fn add_component_internal(&self, lib_path: &Path, component_data: &str) -> Result<()> {
        let mut content = if lib_path.exists() {
            let existing = fs::read_to_string(lib_path).map_err(KicadError::Io)?;
            existing.trim_end().trim_end_matches(')').to_string()
        } else {
            if component_data.contains("(symbol") {
                String::from("(kicad_symbol_lib\n  (version 20211014)\n  (generator npnp)")
            } else {
                String::from("EESchema-LIBRARY Version 2.4\n#encoding utf-8")
            }
        };

        content.push('\n');
        content.push_str(component_data);

        if component_data.contains("(symbol") {
            content.push('\n');
            content.push(')');
        }
        content.push('\n');

        fs::write(lib_path, content).map_err(KicadError::Io)?;

        Ok(())
    }

    /// Internal update component (assumes lock is held)
    fn update_component_internal(
        &self,
        lib_path: &Path,
        component_name: &str,
        new_data: &str,
    ) -> Result<()> {
        let content = fs::read_to_string(lib_path).map_err(KicadError::Io)?;

        // Try v6 format: find symbol block by matching parentheses
        let search = format!(r#"(symbol "{}""#, component_name);
        if let Some(start) = content.find(&search) {
            // Walk back to consume leading whitespace/newline before (symbol
            let mut block_start = start;
            while block_start > 0 && content.as_bytes()[block_start - 1] == b' ' {
                block_start -= 1;
            }
            if block_start > 0 && content.as_bytes()[block_start - 1] == b'\n' {
                block_start -= 1;
            }

            // Count parentheses from start to find the matching close
            let mut depth = 0;
            let mut block_end = start;
            for (i, ch) in content[start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            block_end = start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if block_end > start {
                let mut new_content = String::with_capacity(content.len());
                new_content.push_str(&content[..block_start]);
                new_content.push('\n');
                new_content.push_str(new_data);
                new_content.push_str(&content[block_end..]);
                fs::write(lib_path, &new_content).map_err(KicadError::Io)?;
                return Ok(());
            }
        }

        // Try v5 format
        let v5_start = format!("DEF {} ", component_name);
        if let Some(start) = content.find(&v5_start)
            && let Some(end_offset) = content[start..].find("ENDDEF")
        {
            let block_end = start + end_offset + "ENDDEF".len();
            // Skip trailing newline
            let block_end = if content[block_end..].starts_with('\n') {
                block_end + 1
            } else {
                block_end
            };
            let mut new_content = String::with_capacity(content.len());
            new_content.push_str(&content[..start]);
            new_content.push_str(new_data);
            new_content.push_str(&content[block_end..]);
            fs::write(lib_path, &new_content).map_err(KicadError::Io)?;
            return Ok(());
        }

        Err(
            KicadError::SymbolExport(format!("Component {} not found in library", component_name))
                .into(),
        )
    }

    /// Add a component to the library file
    pub fn add_component(&self, lib_path: &Path, component_data: &str) -> Result<()> {
        // Lock to prevent concurrent writes to the same symbol library file
        let _lock = SYMBOL_WRITE_LOCK.lock().unwrap();

        let mut content = if lib_path.exists() {
            // Read existing file and remove the closing parenthesis
            let existing = fs::read_to_string(lib_path).map_err(KicadError::Io)?;
            // Remove trailing ')' and whitespace
            existing.trim_end().trim_end_matches(')').to_string()
        } else {
            // Create new library file with header (v6 format with proper formatting)
            if component_data.contains("(symbol") {
                // v6 format - match Python's formatting exactly
                String::from("(kicad_symbol_lib\n  (version 20211014)\n  (generator npnp)")
            } else {
                // v5 format
                String::from("EESchema-LIBRARY Version 2.4\n#encoding utf-8")
            }
        };

        // Append component
        content.push('\n');
        content.push_str(component_data);

        // Add closing parenthesis for v6 format
        if component_data.contains("(symbol") {
            content.push('\n');
            content.push(')');
        }
        content.push('\n');

        fs::write(lib_path, content).map_err(KicadError::Io)?;

        Ok(())
    }

    /// Update an existing component in the library file
    pub fn update_component(
        &self,
        lib_path: &Path,
        component_name: &str,
        new_data: &str,
    ) -> Result<()> {
        // Lock to prevent concurrent writes to the same symbol library file
        let _lock = SYMBOL_WRITE_LOCK.lock().unwrap();

        let content = fs::read_to_string(lib_path).map_err(KicadError::Io)?;

        // Try v6 format: find symbol block by matching parentheses
        let search = format!(r#"(symbol "{}""#, component_name);
        if let Some(start) = content.find(&search) {
            let block_start = content[..start].rfind('(').unwrap_or(start);
            let mut depth = 0;
            let mut block_end = block_start;
            for (i, ch) in content[block_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            block_end = block_start + i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if block_end > block_start {
                let mut new_content = String::with_capacity(content.len());
                new_content.push_str(&content[..block_start]);
                new_content.push_str(new_data);
                new_content.push_str(&content[block_end..]);
                fs::write(lib_path, &new_content).map_err(KicadError::Io)?;
                return Ok(());
            }
        }

        // Try v5 format
        let v5_start = format!("DEF {} ", component_name);
        if let Some(start) = content.find(&v5_start)
            && let Some(end_offset) = content[start..].find("ENDDEF")
        {
            let block_end = start + end_offset + "ENDDEF".len();
            let block_end = if content[block_end..].starts_with('\n') {
                block_end + 1
            } else {
                block_end
            };
            let mut new_content = String::with_capacity(content.len());
            new_content.push_str(&content[..start]);
            new_content.push_str(new_data);
            new_content.push_str(&content[block_end..]);
            fs::write(lib_path, &new_content).map_err(KicadError::Io)?;
            return Ok(());
        }

        Err(
            KicadError::SymbolExport(format!("Component {} not found in library", component_name))
                .into(),
        )
    }

    /// Atomic write: write to temp file with buffered I/O, then rename
    fn atomic_write(
        path: &Path,
        data: &[u8],
        buf_size: usize,
    ) -> std::result::Result<(), std::io::Error> {
        let tmp_path = path.with_extension("tmp");
        {
            let file = fs::File::create(&tmp_path)?;
            let mut writer = BufWriter::with_capacity(buf_size, file);
            writer.write_all(data)?;
            writer.flush()?;
        }
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn get_footprint_path(&self, footprint_name: &str) -> PathBuf {
        self.footprint_lib_dir
            .join(format!("{}.kicad_mod", footprint_name))
    }

    /// Write a footprint file
    pub fn write_footprint(&self, footprint_name: &str, data: &str) -> Result<PathBuf> {
        let (footprint_path, _) = self.write_footprint_if_needed(footprint_name, data, true)?;
        Ok(footprint_path)
    }

    pub fn write_footprint_if_needed(
        &self,
        footprint_name: &str,
        data: &str,
        overwrite: bool,
    ) -> Result<(PathBuf, FileWriteStatus)> {
        let footprint_path = self.get_footprint_path(footprint_name);
        if footprint_path.exists() && !overwrite {
            return Ok((footprint_path, FileWriteStatus::Skipped));
        }

        Self::atomic_write(&footprint_path, data.as_bytes(), 32 * 1024).map_err(KicadError::Io)?;

        log::info!("Wrote footprint: {}", display_path(&footprint_path));
        Ok((footprint_path, FileWriteStatus::Written))
    }

    /// Write only STEP model
    pub fn write_step_model(&self, model_name: &str, step_data: &[u8]) -> Result<PathBuf> {
        let step_path = self.get_step_path(model_name);

        Self::atomic_write(&step_path, step_data, 256 * 1024).map_err(KicadError::Io)?;

        log::info!("Wrote STEP model: {}", display_path(&step_path));
        Ok(step_path)
    }

    /// Get the path for a STEP model file
    pub fn get_step_path(&self, model_name: &str) -> PathBuf {
        self.model_lib_dir.join(format!("{}.step", model_name))
    }

    /// Get the symbol library path
    pub fn get_symbol_lib_path(&self) -> PathBuf {
        if let Some(path) = &self.symbol_lib_override {
            path.clone()
        } else {
            self.output_path
                .join(format!("{}.kicad_sym", self.lib_name))
        }
    }

    fn library_name_from_path(path: &Path, suffix: &str) -> Result<String> {
        let name = Self::path_name(path)?;
        Ok(Self::strip_required_suffix(&name, suffix)?.to_string())
    }

    fn path_name(path: &Path) -> Result<String> {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .ok_or_else(|| AppError::Other(format!("Invalid library path: {}", display_path(path))))
    }

    fn strip_required_suffix<'a>(name: &'a str, suffix: &str) -> Result<&'a str> {
        name.strip_suffix(suffix).ok_or_else(|| {
            AppError::Other(format!("Library target must end with {}: {}", suffix, name))
        })
    }
}

fn resolved_lib_name(args: &KicadLibraryOptions) -> String {
    args.library_name.clone().unwrap_or_else(|| {
        args.output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("npnp")
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "npnp-kicad-{}-{}-{}",
            name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn resolves_default_library_targets_with_custom_lib_name() {
        let options = KicadLibraryOptions {
            output: PathBuf::from("out"),
            library_name: Some("custom".to_string()),
            symbol_lib: None,
            footprint_lib: None,
            model_lib: None,
        };

        let manager = LibraryManager::from_options(&options).unwrap();
        assert_eq!(
            manager.get_symbol_lib_path(),
            PathBuf::from("out").join("custom.kicad_sym")
        );
        assert_eq!(manager.footprint_lib_name(), "custom");
        assert_eq!(manager.model_lib_name(), "custom");
        assert_eq!(manager.model_dir_name(), "custom.3dshapes");
    }

    #[test]
    fn discovers_single_append_library_set() {
        let root = temp_dir("discover_append");
        fs::create_dir_all(root.join("Parts.pretty")).unwrap();
        fs::create_dir_all(root.join("Parts.3dshapes")).unwrap();
        fs::write(root.join("Parts.kicad_sym"), "").unwrap();

        let (symbol, footprint, model) = discover_append_libraries(&root, None).unwrap();
        assert_eq!(symbol, Some(root.join("Parts.kicad_sym")));
        assert_eq!(footprint, Some(root.join("Parts.pretty")));
        assert_eq!(model, Some(root.join("Parts.3dshapes")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_explicit_library_targets() {
        let options = KicadLibraryOptions {
            output: PathBuf::from("out"),
            library_name: None,
            symbol_lib: Some(PathBuf::from("libs/custom.kicad_sym")),
            footprint_lib: Some(PathBuf::from("pcb/board.pretty")),
            model_lib: Some(PathBuf::from("assets/parts.3dshapes")),
        };

        let manager = LibraryManager::from_options(&options).unwrap();
        assert_eq!(
            manager.get_symbol_lib_path(),
            PathBuf::from("libs/custom.kicad_sym")
        );
        assert_eq!(manager.footprint_lib_name(), "board");
        assert_eq!(manager.model_lib_name(), "parts");
        assert_eq!(manager.model_dir_name(), "parts.3dshapes");
    }

    #[test]
    fn creates_explicit_targets_and_appends_symbol() {
        let root = temp_dir("existing-libs");
        let symbol_lib = root.join("symbols").join("custom.kicad_sym");
        let footprint_lib = root.join("footprints").join("board.pretty");
        let model_lib = root.join("models").join("parts.3dshapes");

        let options = KicadLibraryOptions {
            output: root.clone(),
            library_name: None,
            symbol_lib: Some(symbol_lib.clone()),
            footprint_lib: Some(footprint_lib.clone()),
            model_lib: Some(model_lib.clone()),
        };

        let manager = LibraryManager::from_options(&options).unwrap();
        manager.create_directories().unwrap();

        assert!(symbol_lib.parent().unwrap().exists());
        assert!(footprint_lib.is_dir());
        assert!(model_lib.is_dir());

        manager
            .add_or_update_component(
                &manager.get_symbol_lib_path(),
                "Part_C1",
                r#"  (symbol "Part_C1")"#,
                false,
            )
            .unwrap();
        manager
            .add_or_update_component(
                &manager.get_symbol_lib_path(),
                "Part_C2",
                r#"  (symbol "Part_C2")"#,
                false,
            )
            .unwrap();

        let content = fs::read_to_string(&symbol_lib).unwrap();
        assert!(content.contains(r#"(symbol "Part_C1")"#));
        assert!(content.contains(r#"(symbol "Part_C2")"#));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn skips_existing_symbol_without_overwrite() {
        let root = temp_dir("symbol-skip");
        let manager = LibraryManager::new(&root);
        manager.create_directories().unwrap();
        let lib_path = manager.get_symbol_lib_path();

        let first = manager
            .add_or_update_component(&lib_path, "Part_C1", r#"  (symbol "Part_C1")"#, false)
            .unwrap();
        let second = manager
            .add_or_update_component(&lib_path, "Part_C1", r#"  (symbol "Part_C1_new")"#, false)
            .unwrap();

        assert_eq!(first, SymbolWriteStatus::Added);
        assert_eq!(second, SymbolWriteStatus::Skipped);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn skips_existing_footprint_without_overwrite() {
        let root = temp_dir("footprint-skip");
        let manager = LibraryManager::new(&root);
        manager.create_directories().unwrap();

        let first = manager
            .write_footprint_if_needed("Part_C1", "(footprint \"Part_C1\")", false)
            .unwrap();
        let second = manager
            .write_footprint_if_needed("Part_C1", "(footprint \"Part_C1_new\")", false)
            .unwrap();

        assert_eq!(first.1, FileWriteStatus::Written);
        assert_eq!(second.1, FileWriteStatus::Skipped);

        let _ = fs::remove_dir_all(root);
    }
}
