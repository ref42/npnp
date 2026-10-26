use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use npnp::kicad::merge::merge_library_directory;

static ID: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "npnp-merge-test-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("input")).unwrap();
        Self(path)
    }
    fn input(&self) -> PathBuf {
        self.0.join("input")
    }
    fn output(&self) -> PathBuf {
        self.0.join("output/Parts.v2")
    }
    fn write(&self, file: &str, content: &str) {
        let path = self.input().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    fn libraries(&self) {
        self.write(
            "A.kicad_sym",
            r#"(kicad_symbol_lib (version 20211014) (generator test)
          (symbol "电阻" (property "Footprint" "FootA:R")
            (property "Description" "Parentheses () and escaped \"quotes\"; FootA:R")
            (symbol "电阻_0_1" (text "unicode (电阻)")))
          (symbol "Child" (extends "电阻")))"#,
        );
        self.write(
            "B.kicad_sym",
            r#"(kicad_symbol_lib (version 20250114) (generator test)
          (symbol "Other" (property "Footprint" "External:Keep")))"#,
        );
        self.write(
            "FootA.pretty/R.kicad_mod",
            r#"(footprint "R" (descr "Models.3dshapes/R.step")
          (model "${Models}/Models.3dshapes/R.step" (offset (xyz 1 2 3)))
          (model "${KICAD9_3DMODEL_DIR}/External/R.wrl"))"#,
        );
        self.write(
            "FootB.pretty/S.kicad_mod",
            r#"(footprint "S"
          (model "${KIPRJMOD}/Other.3dshapes/nested/S.wrl"))"#,
        );
        self.write("Models.3dshapes/R.step", "STEP payload (unchanged)");
        self.write("Other.3dshapes/nested/S.wrl", "VRML payload (unchanged)");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn merges_two_complete_sets_with_unicode_units_links_and_models() {
    let f = Fixture::new();
    f.libraries();
    let result = merge_library_directory(&f.input(), &f.output(), false).unwrap();
    assert_eq!((result.symbol_files, result.symbols), (2, 3));
    assert_eq!((result.footprint_libraries, result.footprints), (2, 2));
    assert_eq!((result.model_libraries, result.models), (2, 2));
    let out = f.0.join("output");
    let symbols = fs::read_to_string(out.join("Parts.v2.kicad_sym")).unwrap();
    assert!(symbols.contains("(version 20250114)"));
    assert!(symbols.contains("(symbol \"电阻_0_1\""));
    assert!(symbols.contains("(extends \"电阻\")"));
    assert!(symbols.contains("(property \"Footprint\" \"Parts.v2:R\")"));
    assert!(symbols.contains("FootA:R\")")); // description not rewritten
    assert!(symbols.contains("External:Keep"));
    let footprint = fs::read_to_string(out.join("Parts.v2.pretty/R.kicad_mod")).unwrap();
    assert!(footprint.contains("(offset (xyz 1 2 3))"));
    assert!(footprint.contains("(descr \"Models.3dshapes/R.step\")"));
    assert!(footprint.contains("${KICAD9_3DMODEL_DIR}/External/R.wrl"));
    assert!(!footprint.contains("${Models}"));
    assert!(!footprint.contains("//?/"));
    assert!(footprint.contains("/Parts.v2.3dshapes/R.step\""));
    assert_eq!(
        fs::read(out.join("Parts.v2.3dshapes/R.step")).unwrap(),
        b"STEP payload (unchanged)"
    );
    assert_eq!(
        fs::read(out.join("Parts.v2.3dshapes/nested/S.wrl")).unwrap(),
        b"VRML payload (unchanged)"
    );
    let again = merge_library_directory(&out, &f.0.join("roundtrip/Roundtrip"), false).unwrap();
    assert_eq!((again.symbols, again.footprints, again.models), (3, 2, 2));
    let roundtrip = fs::read_to_string(f.0.join("roundtrip/Roundtrip.pretty/R.kicad_mod")).unwrap();
    assert!(roundtrip.contains("/Roundtrip.3dshapes/R.step\""));
}

#[test]
fn conflicts_are_detected_before_any_existing_outputs_change() {
    for (file, contents) in [
        (
            "B.kicad_sym",
            r#"(kicad_symbol_lib (version 20250114) (symbol "电阻"))"#,
        ),
        ("FootB.pretty/r.kicad_mod", r#"(footprint "r")"#),
        ("Other.3dshapes/r.step", "conflicting STEP"),
        (
            "B.kicad_sym",
            r#"(kicad_symbol_lib (version 20250114) (symbol "Broken")"#,
        ),
        (
            "B.kicad_sym",
            r#"(not_a_library (version 20250114) (symbol "Wrong"))"#,
        ),
        (
            "B.kicad_sym",
            r#"(kicad_symbol_lib (version 20250114) (embedded_files (data "x")))"#,
        ),
        (
            "FootB.pretty/S.kicad_mod",
            r#"(footprint "S" (model "unfinished)"#,
        ),
    ] {
        let f = Fixture::new();
        f.libraries();
        merge_library_directory(&f.input(), &f.output(), false).unwrap();
        let symbol_path = f.0.join("output/Parts.v2.kicad_sym");
        let before = fs::read(&symbol_path).unwrap();
        f.write(file, contents);
        assert!(
            merge_library_directory(&f.input(), &f.output(), true).is_err(),
            "accepted {file}"
        );
        assert_eq!(fs::read(&symbol_path).unwrap(), before);
        assert_eq!(
            fs::read(f.0.join("output/Parts.v2.3dshapes/R.step")).unwrap(),
            b"STEP payload (unchanged)"
        );
    }
}

#[test]
fn force_replaces_outputs_and_removes_stale_files_but_never_inputs() {
    let f = Fixture::new();
    f.libraries();
    merge_library_directory(&f.input(), &f.output(), false).unwrap();
    let stale = f.0.join("output/Parts.v2.pretty/stale.kicad_mod");
    fs::write(&stale, "old").unwrap();
    assert!(
        merge_library_directory(&f.input(), &f.output(), false)
            .unwrap_err()
            .to_string()
            .contains("--force")
    );
    merge_library_directory(&f.input(), &f.output(), true).unwrap();
    assert!(!stale.exists());
    assert!(
        merge_library_directory(&f.input(), &f.input().join("FootA.pretty/nested/New"), true)
            .is_err()
    );
}

#[test]
fn reruns_inside_input_and_merges_previous_outputs_without_duplicates() {
    let f = Fixture::new();
    f.libraries();
    let output = f.input().join("merged");
    merge_library_directory(&f.input(), &output, false).unwrap();
    assert!(
        merge_library_directory(&f.input(), &output, false)
            .unwrap_err()
            .to_string()
            .contains("--force")
    );
    fs::write(f.input().join("merged.pretty/stale.kicad_mod"), "stale").unwrap();
    let repeated = merge_library_directory(&f.input(), &output, true).unwrap();
    assert_eq!(
        (repeated.symbols, repeated.footprints, repeated.models),
        (3, 2, 2)
    );
    assert!(!f.input().join("merged.pretty/stale.kicad_mod").exists());
    let new = merge_library_directory(&f.input(), &f.input().join("merged_new"), false).unwrap();
    assert_eq!((new.symbols, new.footprints, new.models), (3, 2, 2));
    // Both old outputs can coexist with the originals on subsequent runs.
    let again = merge_library_directory(&f.input(), &output.canonicalize().unwrap_or(output), true)
        .unwrap();
    assert_eq!((again.symbols, again.footprints, again.models), (3, 2, 2));
    let footprint = fs::read_to_string(f.input().join("merged_new.pretty/R.kicad_mod")).unwrap();
    assert!(footprint.contains("/merged_new.3dshapes/R.step"));
}

#[test]
fn output_only_directory_cannot_erase_its_only_library() {
    let f = Fixture::new();
    f.write(
        "Only.kicad_sym",
        "(kicad_symbol_lib (version 20211014) (symbol \"A\"))",
    );
    let before = fs::read(f.input().join("Only.kicad_sym")).unwrap();
    assert!(
        merge_library_directory(&f.input(), &f.input().join("Only"), true)
            .unwrap_err()
            .to_string()
            .contains("no source")
    );
    assert_eq!(fs::read(f.input().join("Only.kicad_sym")).unwrap(), before);
}

#[test]
fn changed_previous_merge_still_reports_conflicts_without_changing_outputs() {
    let f = Fixture::new();
    f.libraries();
    merge_library_directory(&f.input(), &f.input().join("merged"), false).unwrap();
    fs::write(f.input().join("merged.3dshapes/R.step"), "edited model").unwrap();
    let error = merge_library_directory(&f.input(), &f.input().join("new"), true).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("conflicting duplicate model path")
    );
    assert!(!f.input().join("new.kicad_sym").exists());
    assert_eq!(
        fs::read(f.input().join("merged.3dshapes/R.step")).unwrap(),
        b"edited model"
    );
}

#[test]
fn supports_each_library_type_independently_and_empty_symbol_libraries() {
    for (file, data) in [
        ("Only.kicad_sym", "(kicad_symbol_lib (version 20211014))"),
        ("Only.pretty/P.kicad_mod", "(footprint \"P\")"),
        ("Only.3dshapes/M.step", "model"),
    ] {
        let f = Fixture::new();
        f.write(file, data);
        let result = merge_library_directory(&f.input(), &f.output(), false).unwrap();
        assert_eq!(result.outputs.len(), 1);
        assert!(result.outputs[0].exists());
    }
}

#[test]
fn missing_or_cyclic_inheritance_is_rejected() {
    for definitions in [
        r#"(symbol "A" (extends "Missing"))"#,
        r#"(symbol "A" (extends "B")) (symbol "B" (extends "A"))"#,
    ] {
        let f = Fixture::new();
        f.write(
            "Bad.kicad_sym",
            &format!("(kicad_symbol_lib (version 20211014) {definitions})"),
        );
        assert!(merge_library_directory(&f.input(), &f.output(), false).is_err());
        assert!(!f.0.join("output").exists());
    }
}

#[test]
fn repairs_legacy_npnp_model_references_only_when_the_model_is_included() {
    let f = Fixture::new();
    f.write(
        "A.pretty/R.kicad_mod",
        r#"(footprint "R"
      (model "${A}/A.3dshapes/R_C123.step")
      (model "${A}/A.3dshapes/Missing_C456.step"))"#,
    );
    f.write("A.3dshapes/R__C123.step", "step");
    merge_library_directory(&f.input(), &f.output(), false).unwrap();
    let text = fs::read_to_string(f.0.join("output/Parts.v2.pretty/R.kicad_mod")).unwrap();
    assert!(text.contains("/Parts.v2.3dshapes/R__C123.step\""));
    assert!(text.contains("${A}/A.3dshapes/Missing_C456.step"));
}

fn cli(input: &Path, output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_npnp"))
        .arg("merge")
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(output)
        .output()
        .unwrap()
}

#[test]
fn cli_detects_kicad_and_rejects_mixed_or_empty_inputs() {
    let f = Fixture::new();
    assert!(!cli(&f.input(), &f.output()).status.success());
    f.libraries();
    let result = cli(&f.input(), &f.output());
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("KiCad merge complete"));
    f.write(
        "Other.SchLib",
        "not parsed because mixed formats fail discovery",
    );
    let result = cli(&f.input(), &f.0.join("mixed/Output"));
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("separate directory"));
    assert!(!f.0.join("mixed").exists());
}
