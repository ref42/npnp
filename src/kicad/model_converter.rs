use super::converter::sanitize_name;
use super::step_sanitizer::sanitize_step_model;
use crate::easyeda::{ComponentData, EasyedaApi};
use crate::error::Result;
use crate::kicad::{KicadExportOptions, LibraryManager};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Default)]
pub struct ModelStageStatus {
    step_downloads: AtomicUsize,
    active_steps: Mutex<HashMap<String, Instant>>,
}

impl ModelStageStatus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn summary(&self) -> String {
        let active_count = self.step_downloads.load(Ordering::Relaxed);
        if active_count == 0 {
            return "step:0".to_string();
        }

        let slowest = self.active_steps.lock().ok().and_then(|active_steps| {
            active_steps
                .iter()
                .max_by_key(|(_, started)| started.elapsed())
                .map(|(id, started)| (id.clone(), started.elapsed().as_secs()))
        });

        match slowest {
            Some((id, elapsed)) => format!("step:{} slow:{} {}s", active_count, id, elapsed),
            None => format!("step:{}", active_count),
        }
    }

    fn start_step(&self, lcsc_id: &str) {
        self.step_downloads.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut active_steps) = self.active_steps.lock() {
            active_steps.insert(lcsc_id.to_string(), Instant::now());
        }
    }

    fn finish_step(&self, lcsc_id: &str) {
        decrement(&self.step_downloads);
        if let Ok(mut active_steps) = self.active_steps.lock() {
            active_steps.remove(lcsc_id);
        }
    }
}

pub async fn convert_3d_model(
    args: &KicadExportOptions,
    api: &EasyedaApi,
    component_data: &ComponentData,
    lib_manager: &LibraryManager,
    lcsc_id: &str,
    model_status: &ModelStageStatus,
) -> Result<()> {
    let Some(model_info) = &component_data.model_3d else {
        log::warn!("No 3D model metadata available for this component");
        return Ok(());
    };

    let model_name = format!("{}_{}", sanitize_name(&model_info.title), lcsc_id);
    let step_path = lib_manager.get_step_path(&model_name);
    let need_step = args.force || !step_path.exists();

    if !need_step {
        log::debug!("Skipped existing STEP model: {}", model_name);
        return Ok(());
    }

    model_status.start_step(lcsc_id);
    let result = api.download_3d_step(&model_info.uuid).await;
    model_status.finish_step(lcsc_id);

    match result {
        Ok(step_data) => {
            let (sanitized_step_data, stats) = sanitize_step_model(&step_data)?;
            if stats.removed_solids > 0 || stats.restyled_faces > 0 {
                let mut details = Vec::new();
                if stats.removed_solids > 0 {
                    details.push(format!("{} shallow top solids", stats.removed_solids));
                }
                if stats.restyled_faces > 0 {
                    details.push(format!(
                        "{} styled faces across {} logo groups",
                        stats.restyled_faces, stats.restyled_groups
                    ));
                }
                log::info!(
                    "Removed {} from STEP model {}",
                    details.join(" and "),
                    model_name
                );
            }
            lib_manager.write_step_model(&model_name, &sanitized_step_data)?;
            log::debug!("Wrote STEP model: {}", model_name);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn decrement(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_sub(1))
    });
}
