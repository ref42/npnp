use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

const SHALLOW_TOP_SOLID_HEIGHT_LIMIT_MM: f64 = 0.01;
const TOP_SURFACE_TOLERANCE_MM: f64 = 0.02;
const EMBEDDED_SHALLOW_SOLID_SURFACE_OFFSET_MM: f64 = 1.0;
const OUTER_FACE_TOLERANCE_MM: f64 = 0.05;
const PLANAR_FACE_AXIS_ALIGNMENT_MIN: f64 = 0.95;
const LOGO_STYLE_MIN_FACES: usize = 5;
const LOGO_STYLE_SMALL_MIN_FACES: usize = 3;
const LOGO_STYLE_MAX_AREA_RATIO: f64 = 0.2;
const LOGO_STYLE_SMALL_MAX_AREA_RATIO: f64 = 0.08;
const LOGO_STYLE_MIN_CONTRAST: f64 = 0.4;
const LOGO_STYLE_PLANE_TOLERANCE_MM: f64 = 0.02;
const LOGO_STYLE_CLUSTER_GAP_MM: f64 = 0.2;
const LOGO_SIDE_FACE_DEPTH_MM: f64 = 0.25;
const LOGO_SIDE_FACE_GAP_MM: f64 = 0.08;
const LOGO_SIDE_FACE_COLOR_TOLERANCE: f64 = 0.08;
const GEOMETRY_LOGO_MIN_FACES: usize = 5;
const GEOMETRY_LOGO_MAX_AREA_RATIO: f64 = 0.12;
const GEOMETRY_LOGO_OUTER_TOLERANCE_MM: f64 = 0.08;
const GEOMETRY_LOGO_BACKGROUND_DEPTH_MM: f64 = 0.7;
const GEOMETRY_LOGO_BACKGROUND_TOLERANCE_MM: f64 = 0.2;
const GEOMETRY_LOGO_SOLID_FALLBACK_MIN_FACES: usize = 5;
const FACE_LOGO_SOLID_FALLBACK_MIN_FACES: usize = 3;
const BOTTOM_PAD_FEATURE_HEIGHT_LIMIT_MM: f64 = 0.25;
const BOTTOM_LOGO_EDGE_CLEARANCE_MM: f64 = 0.4;
const BOTTOM_LOGO_EDGE_CLEARANCE_RATIO: f64 = 0.12;
const LOGO_COVER_PATCH_MARGIN_MM: f64 = 0.04;
const LOGO_COVER_PATCH_OFFSET_MM: f64 = 0.006;
const LOGO_COVER_PATCH_THICKNESS_MM: f64 = 0.04;
const LOGO_COVER_PATCH_DEDUP_TOLERANCE_MM: f64 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq)]
struct BoundingBox {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    min_z: f64,
    max_z: f64,
}

impl BoundingBox {
    fn from_point(point: [f64; 3]) -> Self {
        Self {
            min_x: point[0],
            max_x: point[0],
            min_y: point[1],
            max_y: point[1],
            min_z: point[2],
            max_z: point[2],
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            max_x: self.max_x.max(other.max_x),
            min_y: self.min_y.min(other.min_y),
            max_y: self.max_y.max(other.max_y),
            min_z: self.min_z.min(other.min_z),
            max_z: self.max_z.max(other.max_z),
        }
    }

    fn height_z(self) -> f64 {
        self.max_z - self.min_z
    }

    fn axis_min(self, axis: usize) -> f64 {
        match axis {
            0 => self.min_x,
            1 => self.min_y,
            _ => self.min_z,
        }
    }

    fn axis_max(self, axis: usize) -> f64 {
        match axis {
            0 => self.max_x,
            1 => self.max_y,
            _ => self.max_z,
        }
    }
}

#[derive(Debug)]
struct StepEntity {
    id: usize,
    body: Vec<u8>,
    span: std::ops::Range<usize>,
    refs: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepSanitizeStats {
    pub total_solids: usize,
    pub removed_solids: usize,
    pub restyled_faces: usize,
    pub restyled_groups: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OuterSide {
    Min,
    Max,
}

#[derive(Debug, Clone, Copy)]
struct PlanarFaceInfo {
    axis: usize,
    coordinate: f64,
}

#[derive(Debug, Clone)]
struct FaceStyleInfo {
    styled_item_id: usize,
    style_id: usize,
    color: [f64; 3],
    bbox: BoundingBox,
    face: PlanarFaceInfo,
    outer_side: Option<OuterSide>,
}

#[derive(Debug, Clone)]
struct FaceStyleGroup {
    representative_style_id: usize,
    color: [f64; 3],
    axis: usize,
    side: OuterSide,
    bbox: BoundingBox,
    min_coordinate: f64,
    max_coordinate: f64,
    face_count: usize,
    member_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
struct AnyFaceStyleInfo {
    styled_item_id: usize,
    style_id: usize,
    color: [f64; 3],
    bbox: BoundingBox,
}

#[derive(Debug, Clone)]
struct GeometryLogoCluster {
    color: [f64; 3],
    bbox: BoundingBox,
    member_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
struct SolidStyleInfo {
    style_id: usize,
    color: [f64; 3],
}

#[derive(Debug, Clone, Copy)]
struct CoverPatch {
    bbox: BoundingBox,
    style_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ColorKey([i32; 3]);

#[derive(Debug, Default)]
struct StyleRewritePlan {
    replacements: HashMap<usize, usize>,
    groups_restyled: usize,
    cover_patches: Vec<CoverPatch>,
}

pub fn sanitize_step_model(step_data: &[u8]) -> Result<(Vec<u8>, StepSanitizeStats)> {
    let entities = parse_entities(step_data);
    let entity_by_id = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity.id, index))
        .collect::<HashMap<_, _>>();

    let solid_ids = entities
        .iter()
        .filter(|entity| starts_with_ascii(&entity.body, b"MANIFOLD_SOLID_BREP"))
        .map(|entity| entity.id)
        .collect::<Vec<_>>();

    if solid_ids.is_empty() {
        return Ok((
            step_data.to_vec(),
            StepSanitizeStats {
                total_solids: 0,
                removed_solids: 0,
                restyled_faces: 0,
                restyled_groups: 0,
            },
        ));
    }

    let mut bbox_cache = HashMap::new();
    let mut solids = Vec::new();

    for solid_id in solid_ids {
        let Some(bounds) = bounds_for_entity(
            solid_id,
            &entities,
            &entity_by_id,
            &mut bbox_cache,
            &mut HashSet::new(),
        ) else {
            continue;
        };
        solids.push((solid_id, bounds));
    }

    let removable_solids = if solids.len() >= 2 {
        let overall_bounds = solids
            .iter()
            .map(|(_, bounds)| *bounds)
            .reduce(BoundingBox::union)
            .expect("at least one solid must exist");

        solids
            .iter()
            .filter_map(|(solid_id, bounds)| {
                is_shallow_top_marking(*solid_id, *bounds, overall_bounds, &solids)
                    .then_some(*solid_id)
            })
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    log::debug!(
        "STEP sanitizer inspected {} solids and marked {} shallow top solids for removal",
        solids.len(),
        removable_solids.len()
    );

    let style_rewrite_plan =
        collect_logo_style_replacements(&solids, &entities, &entity_by_id, &mut bbox_cache);

    log::debug!(
        "STEP sanitizer marked {} styled faces across {} logo groups for restyling",
        style_rewrite_plan.replacements.len(),
        style_rewrite_plan.groups_restyled
    );

    if removable_solids.len() >= solids.len() {
        return Ok((
            step_data.to_vec(),
            StepSanitizeStats {
                total_solids: solids.len(),
                removed_solids: 0,
                restyled_faces: 0,
                restyled_groups: 0,
            },
        ));
    }

    let (cover_brep_ids, cover_entities) = build_cover_patch_entities(
        &style_rewrite_plan.cover_patches,
        max_entity_id(&entities) + 1,
    );

    log::debug!(
        "STEP sanitizer added {} logo cover patches",
        style_rewrite_plan.cover_patches.len()
    );

    let mut replacements =
        collect_representation_replacements(&entities, &removable_solids, &cover_brep_ids);
    replacements.extend(collect_styled_item_replacements(
        &entities,
        &style_rewrite_plan.replacements,
    ));

    let updated = append_step_entities(
        &apply_replacements(step_data, replacements),
        &cover_entities,
    );
    let changed = updated != step_data;

    Ok((
        if changed { updated } else { step_data.to_vec() },
        StepSanitizeStats {
            total_solids: solids.len(),
            removed_solids: changed.then_some(removable_solids.len()).unwrap_or(0),
            restyled_faces: changed
                .then_some(style_rewrite_plan.replacements.len())
                .unwrap_or(0),
            restyled_groups: changed
                .then_some(style_rewrite_plan.groups_restyled)
                .unwrap_or(0),
        },
    ))
}

fn parse_entities(step_data: &[u8]) -> Vec<StepEntity> {
    let mut entities = Vec::new();
    let mut index = 0usize;

    while index < step_data.len() {
        let Some(hash_offset) = step_data[index..].iter().position(|&byte| byte == b'#') else {
            break;
        };
        let hash_index = index + hash_offset;
        let mut cursor = hash_index + 1;

        while cursor < step_data.len() && step_data[cursor].is_ascii_digit() {
            cursor += 1;
        }

        if cursor == hash_index + 1 {
            index = hash_index + 1;
            continue;
        }

        let Ok(id) = std::str::from_utf8(&step_data[hash_index + 1..cursor])
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(())
        else {
            index = cursor;
            continue;
        };

        while cursor < step_data.len() && step_data[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        if cursor >= step_data.len() || step_data[cursor] != b'=' {
            index = cursor;
            continue;
        }
        cursor += 1;

        while cursor < step_data.len() && step_data[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        let body_start = cursor;
        let Some(semicolon_offset) = step_data[body_start..]
            .iter()
            .position(|&byte| byte == b';')
        else {
            break;
        };
        let body_end = body_start + semicolon_offset;
        let body = step_data[body_start..body_end].to_vec();

        entities.push(StepEntity {
            id,
            refs: parse_refs(&body),
            span: body_start..body_end,
            body,
        });

        index = body_end + 1;
    }

    entities
}

fn parse_refs(body: &[u8]) -> Vec<usize> {
    let mut refs = Vec::new();
    let mut index = 0usize;

    while index < body.len() {
        let Some(hash_offset) = body[index..].iter().position(|&byte| byte == b'#') else {
            break;
        };
        let hash_index = index + hash_offset;
        let mut cursor = hash_index + 1;
        while cursor < body.len() && body[cursor].is_ascii_digit() {
            cursor += 1;
        }

        if cursor > hash_index + 1
            && let Ok(reference) = std::str::from_utf8(&body[hash_index + 1..cursor])
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or(())
        {
            refs.push(reference);
        }

        index = cursor;
    }

    refs
}

fn bounds_for_entity(
    entity_id: usize,
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
    bbox_cache: &mut HashMap<usize, Option<BoundingBox>>,
    visiting: &mut HashSet<usize>,
) -> Option<BoundingBox> {
    if let Some(cached) = bbox_cache.get(&entity_id) {
        return *cached;
    }

    if !visiting.insert(entity_id) {
        return None;
    }

    let result = entity_by_id
        .get(&entity_id)
        .and_then(|entity_index| entities.get(*entity_index))
        .and_then(|entity| {
            if starts_with_ascii(&entity.body, b"CARTESIAN_POINT") {
                parse_cartesian_point(&entity.body).map(BoundingBox::from_point)
            } else {
                entity.refs.iter().fold(None, |bounds, reference| {
                    let next =
                        bounds_for_entity(*reference, entities, entity_by_id, bbox_cache, visiting);
                    match (bounds, next) {
                        (Some(current), Some(next_bounds)) => Some(current.union(next_bounds)),
                        (None, Some(next_bounds)) => Some(next_bounds),
                        (Some(current), None) => Some(current),
                        (None, None) => None,
                    }
                })
            }
        });

    visiting.remove(&entity_id);
    bbox_cache.insert(entity_id, result);
    result
}

fn parse_cartesian_point(body: &[u8]) -> Option<[f64; 3]> {
    let comma_index = body.iter().position(|&byte| byte == b',')?;
    let open_index = body[comma_index + 1..]
        .iter()
        .position(|&byte| byte == b'(')
        .map(|offset| comma_index + 1 + offset)?;
    let close_index = body[open_index + 1..]
        .iter()
        .position(|&byte| byte == b')')
        .map(|offset| open_index + 1 + offset)?;

    let coords = std::str::from_utf8(&body[open_index + 1..close_index]).ok()?;
    let values = coords
        .split(',')
        .map(|value| value.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;

    (values.len() >= 3).then_some([values[0], values[1], values[2]])
}

fn is_shallow_top_marking(
    solid_id: usize,
    bounds: BoundingBox,
    overall_bounds: BoundingBox,
    solids: &[(usize, BoundingBox)],
) -> bool {
    if bounds.height_z() > SHALLOW_TOP_SOLID_HEIGHT_LIMIT_MM {
        return false;
    }

    if bounds.min_z >= overall_bounds.max_z - TOP_SURFACE_TOLERANCE_MM {
        return true;
    }

    solids.iter().any(|(other_id, other)| {
        *other_id != solid_id
            && other.height_z() > SHALLOW_TOP_SOLID_HEIGHT_LIMIT_MM
            && other.max_z >= bounds.max_z
            && other.max_z - bounds.max_z <= EMBEDDED_SHALLOW_SOLID_SURFACE_OFFSET_MM
            && contains_xy(*other, bounds, 0.05)
    })
}

fn contains_xy(outer: BoundingBox, inner: BoundingBox, tolerance: f64) -> bool {
    inner.min_x >= outer.min_x - tolerance
        && inner.max_x <= outer.max_x + tolerance
        && inner.min_y >= outer.min_y - tolerance
        && inner.max_y <= outer.max_y + tolerance
}

fn collect_logo_style_replacements(
    solids: &[(usize, BoundingBox)],
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
    bbox_cache: &mut HashMap<usize, Option<BoundingBox>>,
) -> StyleRewritePlan {
    let mut plan = StyleRewritePlan::default();
    let solid_styles = collect_solid_style_infos(solids, entities, entity_by_id);

    for (solid_id, solid_bounds) in solids {
        let face_ids = descendant_faces(*solid_id, entities, entity_by_id)
            .into_iter()
            .collect::<HashSet<_>>();

        if face_ids.is_empty() {
            continue;
        }

        let face_styles =
            collect_face_style_infos(&face_ids, *solid_bounds, entities, entity_by_id, bbox_cache);
        if face_styles.is_empty() {
            continue;
        }

        let groups = build_outer_style_groups(&face_styles);
        let aggregated_groups = build_disconnected_style_group_aggregates(&groups);
        for candidate in groups.iter().chain(aggregated_groups.iter()) {
            if !is_logo_candidate_group(candidate, *solid_bounds) {
                continue;
            }

            let replacement_style_id =
                if let Some(replacement) = choose_replacement_group(candidate, &groups) {
                    replacement.representative_style_id
                } else if let Some(replacement_style_id) =
                    choose_face_group_solid_style_fallback(candidate, &solid_styles)
                {
                    replacement_style_id
                } else {
                    continue;
                };

            let mut local_changes = 0usize;
            for member_index in
                expanded_logo_member_indices(candidate, replacement_style_id, &face_styles)
            {
                let face_style = &face_styles[member_index];
                if face_style.style_id != replacement_style_id
                    && plan
                        .replacements
                        .insert(face_style.styled_item_id, replacement_style_id)
                        .is_none()
                {
                    local_changes += 1;
                }
            }

            if local_changes > 0 {
                plan.groups_restyled += 1;
            }
        }

        let surface_groups = build_coplanar_style_groups(&face_styles, *solid_bounds);
        let aggregated_surface_groups = build_disconnected_style_group_aggregates(&surface_groups);
        for candidate in surface_groups
            .iter()
            .chain(aggregated_surface_groups.iter())
        {
            let Some(replacement) =
                choose_coplanar_surface_replacement_group(candidate, &surface_groups)
            else {
                continue;
            };
            if !is_coplanar_surface_logo_candidate_group(candidate, replacement, *solid_bounds) {
                continue;
            }

            let mut local_changes = 0usize;
            for member_index in expanded_logo_member_indices(
                candidate,
                replacement.representative_style_id,
                &face_styles,
            ) {
                let face_style = &face_styles[member_index];
                if face_style.style_id != replacement.representative_style_id
                    && plan
                        .replacements
                        .insert(
                            face_style.styled_item_id,
                            replacement.representative_style_id,
                        )
                        .is_none()
                {
                    local_changes += 1;
                }
            }

            if local_changes > 0 {
                plan.groups_restyled += 1;
            }
        }

        collect_geometry_logo_replacements(
            &face_ids,
            *solid_bounds,
            &solid_styles,
            entities,
            entity_by_id,
            bbox_cache,
            &mut plan,
        );
    }

    plan
}

fn is_logo_candidate_group(candidate: &FaceStyleGroup, solid_bounds: BoundingBox) -> bool {
    if !is_supported_logo_side(candidate.axis, candidate.side, candidate.bbox, solid_bounds) {
        return false;
    }

    if !is_logo_candidate_color(candidate.color) {
        return false;
    }

    let solid_side_area = projected_area(solid_bounds, candidate.axis);
    if solid_side_area <= 0.0 {
        return false;
    }

    let area_ratio = projected_area(candidate.bbox, candidate.axis) / solid_side_area;
    candidate.face_count >= LOGO_STYLE_MIN_FACES && area_ratio <= LOGO_STYLE_MAX_AREA_RATIO
        || candidate.face_count >= LOGO_STYLE_SMALL_MIN_FACES
            && area_ratio <= LOGO_STYLE_SMALL_MAX_AREA_RATIO
}

fn is_coplanar_surface_logo_candidate_group(
    candidate: &FaceStyleGroup,
    replacement: &FaceStyleGroup,
    solid_bounds: BoundingBox,
) -> bool {
    if !is_supported_logo_side(candidate.axis, candidate.side, candidate.bbox, solid_bounds) {
        return false;
    }

    if !is_logo_candidate_color(candidate.color)
        || color_distance(candidate.color, replacement.color) < LOGO_STYLE_MIN_CONTRAST
    {
        return false;
    }

    let solid_side_area = projected_area(solid_bounds, candidate.axis);
    if solid_side_area <= 0.0 {
        return false;
    }

    let candidate_area = projected_area(candidate.bbox, candidate.axis);
    let replacement_area = projected_area(replacement.bbox, replacement.axis);
    let area_ratio = candidate_area / solid_side_area;

    replacement_area > candidate_area
        && ((candidate.face_count >= LOGO_STYLE_MIN_FACES
            && area_ratio <= LOGO_STYLE_MAX_AREA_RATIO)
            || (candidate.face_count >= LOGO_STYLE_SMALL_MIN_FACES
                && area_ratio <= LOGO_STYLE_SMALL_MAX_AREA_RATIO))
}

fn expanded_logo_member_indices(
    candidate: &FaceStyleGroup,
    replacement_style_id: usize,
    face_styles: &[FaceStyleInfo],
) -> Vec<usize> {
    let mut selected = candidate
        .member_indices
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut stack = candidate.member_indices.clone();

    while let Some(current_index) = stack.pop() {
        let current_face = &face_styles[current_index];

        for (neighbor_index, neighbor) in face_styles.iter().enumerate() {
            if selected.contains(&neighbor_index)
                || neighbor.style_id == replacement_style_id
                || color_distance(neighbor.color, candidate.color) > LOGO_SIDE_FACE_COLOR_TOLERANCE
                || !is_connected_logo_neighbor(candidate, current_face, neighbor)
            {
                continue;
            }

            selected.insert(neighbor_index);
            stack.push(neighbor_index);
        }
    }

    let mut indices = selected.into_iter().collect::<Vec<_>>();
    indices.sort_unstable();
    indices
}

fn is_connected_logo_neighbor(
    candidate: &FaceStyleGroup,
    current_face: &FaceStyleInfo,
    neighbor: &FaceStyleInfo,
) -> bool {
    projected_bboxes_touch(
        current_face.bbox,
        neighbor.bbox,
        candidate.axis,
        LOGO_SIDE_FACE_GAP_MM,
    ) && bboxes_touch_3d(current_face.bbox, neighbor.bbox, LOGO_SIDE_FACE_GAP_MM)
        && is_within_logo_depth(candidate, neighbor.bbox)
}

fn is_within_logo_depth(candidate: &FaceStyleGroup, bbox: BoundingBox) -> bool {
    let bbox_min = bbox.axis_min(candidate.axis);
    let bbox_max = bbox.axis_max(candidate.axis);

    match candidate.side {
        OuterSide::Max => {
            bbox_max <= candidate.max_coordinate + LOGO_STYLE_PLANE_TOLERANCE_MM
                && bbox_min
                    >= candidate.max_coordinate
                        - LOGO_SIDE_FACE_DEPTH_MM
                        - LOGO_STYLE_PLANE_TOLERANCE_MM
        }
        OuterSide::Min => {
            bbox_min >= candidate.min_coordinate - LOGO_STYLE_PLANE_TOLERANCE_MM
                && bbox_max
                    <= candidate.min_coordinate
                        + LOGO_SIDE_FACE_DEPTH_MM
                        + LOGO_STYLE_PLANE_TOLERANCE_MM
        }
    }
}

fn collect_geometry_logo_replacements(
    face_ids: &HashSet<usize>,
    solid_bounds: BoundingBox,
    solid_styles: &[SolidStyleInfo],
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
    bbox_cache: &mut HashMap<usize, Option<BoundingBox>>,
    plan: &mut StyleRewritePlan,
) {
    let face_styles = collect_any_face_style_infos(face_ids, entities, entity_by_id, bbox_cache);
    if face_styles.is_empty() {
        return;
    }

    let clusters = build_geometry_logo_clusters(&face_styles);
    let aggregated_clusters = build_disconnected_geometry_logo_aggregates(&clusters, solid_bounds);

    for (cluster, axis, side) in clusters
        .into_iter()
        .filter_map(|cluster| {
            geometry_logo_side(&cluster, solid_bounds).map(|(axis, side)| (cluster, axis, side))
        })
        .chain(aggregated_clusters.into_iter())
    {
        if cluster.member_indices.len() < GEOMETRY_LOGO_MIN_FACES
            || !is_logo_candidate_color(cluster.color)
        {
            continue;
        }

        if !is_supported_logo_side(axis, side, cluster.bbox, solid_bounds) {
            continue;
        }

        let solid_side_area = projected_area(solid_bounds, axis);
        if solid_side_area <= 0.0
            || projected_area(cluster.bbox, axis) / solid_side_area > GEOMETRY_LOGO_MAX_AREA_RATIO
        {
            continue;
        }

        let (replacement_style_id, member_indices, needs_cover) =
            if let Some(replacement_style_id) =
                choose_geometry_logo_replacement(&cluster, axis, side, &face_styles)
            {
                (
                    replacement_style_id,
                    expanded_geometry_logo_member_indices(&cluster, axis, &face_styles),
                    false,
                )
            } else if let Some(replacement_style_id) =
                choose_solid_style_fallback(&cluster, solid_styles)
            {
                (replacement_style_id, cluster.member_indices.clone(), true)
            } else {
                continue;
            };

        if member_indices.is_empty() {
            continue;
        };

        let mut local_changes = 0usize;
        for member_index in member_indices {
            let face_style = &face_styles[member_index];
            if face_style.style_id != replacement_style_id
                && plan
                    .replacements
                    .insert(face_style.styled_item_id, replacement_style_id)
                    .is_none()
            {
                local_changes += 1;
            }
        }

        if local_changes > 0 {
            plan.groups_restyled += 1;
        }
        if needs_cover {
            push_cover_patch(
                plan,
                cover_patch_for_bbox(cluster.bbox, solid_bounds, axis, side, replacement_style_id),
            );
        }
    }
}

fn build_disconnected_style_group_aggregates(groups: &[FaceStyleGroup]) -> Vec<FaceStyleGroup> {
    let mut grouped = HashMap::<(usize, OuterSide, ColorKey, i32, i32), Vec<usize>>::new();

    for (index, group) in groups.iter().enumerate() {
        grouped
            .entry((
                group.axis,
                group.side,
                quantize_color(group.color),
                quantize_scalar(group.min_coordinate, LOGO_STYLE_PLANE_TOLERANCE_MM),
                quantize_scalar(group.max_coordinate, LOGO_STYLE_PLANE_TOLERANCE_MM),
            ))
            .or_default()
            .push(index);
    }

    let mut aggregates = Vec::new();

    for indices in grouped.into_values() {
        if indices.len() < 2 {
            continue;
        }

        let first = &groups[indices[0]];
        let mut bbox = first.bbox;
        let mut min_coordinate = first.min_coordinate;
        let mut max_coordinate = first.max_coordinate;
        let mut member_indices = Vec::new();

        for index in indices {
            let group = &groups[index];
            bbox = bbox.union(group.bbox);
            min_coordinate = min_coordinate.min(group.min_coordinate);
            max_coordinate = max_coordinate.max(group.max_coordinate);
            member_indices.extend(group.member_indices.iter().copied());
        }

        member_indices.sort_unstable();
        member_indices.dedup();
        aggregates.push(FaceStyleGroup {
            representative_style_id: first.representative_style_id,
            color: first.color,
            axis: first.axis,
            side: first.side,
            bbox,
            min_coordinate,
            max_coordinate,
            face_count: member_indices.len(),
            member_indices,
        });
    }

    aggregates
}

fn build_coplanar_style_groups(
    face_styles: &[FaceStyleInfo],
    solid_bounds: BoundingBox,
) -> Vec<FaceStyleGroup> {
    let mut grouped = HashMap::<(usize, OuterSide, ColorKey), Vec<usize>>::new();

    for (index, face_style) in face_styles.iter().enumerate() {
        let side = face_style.outer_side.unwrap_or_else(|| {
            nearest_side_for_axis(
                face_style.face.axis,
                face_style.face.coordinate,
                solid_bounds,
            )
        });
        let color_key = quantize_color(face_style.color);
        grouped
            .entry((face_style.face.axis, side, color_key))
            .or_default()
            .push(index);
    }

    let mut groups = Vec::new();

    for ((axis, side, _), indices) in grouped {
        let mut unvisited = indices.into_iter().collect::<HashSet<_>>();

        while let Some(seed_index) = unvisited.iter().next().copied() {
            unvisited.remove(&seed_index);

            let mut member_indices = vec![seed_index];
            let mut stack = vec![seed_index];
            let mut bbox = face_styles[seed_index].bbox;
            let mut min_coordinate = face_styles[seed_index].face.coordinate;
            let mut max_coordinate = face_styles[seed_index].face.coordinate;

            while let Some(current_index) = stack.pop() {
                let current_bbox = face_styles[current_index].bbox;
                let current_coordinate = face_styles[current_index].face.coordinate;
                let remaining = unvisited.iter().copied().collect::<Vec<_>>();

                for candidate_index in remaining {
                    let candidate = &face_styles[candidate_index];
                    if current_coordinate_distance(current_coordinate, candidate.face.coordinate)
                        > LOGO_STYLE_PLANE_TOLERANCE_MM
                        || !projected_bboxes_touch(
                            current_bbox,
                            candidate.bbox,
                            axis,
                            LOGO_STYLE_CLUSTER_GAP_MM,
                        )
                    {
                        continue;
                    }

                    unvisited.remove(&candidate_index);
                    stack.push(candidate_index);
                    member_indices.push(candidate_index);
                    bbox = bbox.union(candidate.bbox);
                    min_coordinate = min_coordinate.min(candidate.face.coordinate);
                    max_coordinate = max_coordinate.max(candidate.face.coordinate);
                }
            }

            let representative = &face_styles[seed_index];
            groups.push(FaceStyleGroup {
                representative_style_id: representative.style_id,
                color: representative.color,
                axis,
                side,
                bbox,
                min_coordinate,
                max_coordinate,
                face_count: member_indices.len(),
                member_indices,
            });
        }
    }

    groups
}

fn build_disconnected_geometry_logo_aggregates(
    clusters: &[GeometryLogoCluster],
    solid_bounds: BoundingBox,
) -> Vec<(GeometryLogoCluster, usize, OuterSide)> {
    let mut grouped = HashMap::<(usize, OuterSide, ColorKey, i32), Vec<usize>>::new();

    for (index, cluster) in clusters.iter().enumerate() {
        let Some((axis, side)) = geometry_logo_side(cluster, solid_bounds) else {
            continue;
        };
        let outer_coordinate = match side {
            OuterSide::Max => cluster.bbox.axis_max(axis),
            OuterSide::Min => cluster.bbox.axis_min(axis),
        };
        grouped
            .entry((
                axis,
                side,
                quantize_color(cluster.color),
                quantize_scalar(outer_coordinate, GEOMETRY_LOGO_OUTER_TOLERANCE_MM),
            ))
            .or_default()
            .push(index);
    }

    let mut aggregates = Vec::new();

    for ((axis, side, _, _), indices) in grouped {
        if indices.len() < 2 {
            continue;
        }

        let first = &clusters[indices[0]];
        let mut bbox = first.bbox;
        let mut member_indices = Vec::new();

        for index in indices {
            let cluster = &clusters[index];
            bbox = bbox.union(cluster.bbox);
            member_indices.extend(cluster.member_indices.iter().copied());
        }

        member_indices.sort_unstable();
        member_indices.dedup();
        aggregates.push((
            GeometryLogoCluster {
                color: first.color,
                bbox,
                member_indices,
            },
            axis,
            side,
        ));
    }

    aggregates
}

fn is_supported_logo_side(
    axis: usize,
    side: OuterSide,
    candidate_bbox: BoundingBox,
    solid_bounds: BoundingBox,
) -> bool {
    let shallow_underside_feature = solid_bounds.height_z() > 0.2
        && candidate_bbox.min_z <= solid_bounds.min_z + TOP_SURFACE_TOLERANCE_MM
        && candidate_bbox.max_z <= solid_bounds.min_z + BOTTOM_PAD_FEATURE_HEIGHT_LIMIT_MM;

    if !shallow_underside_feature {
        return true;
    }

    axis == 2
        && side == OuterSide::Min
        && is_central_bottom_logo_candidate(candidate_bbox, solid_bounds)
}

fn nearest_side_for_axis(axis: usize, coordinate: f64, solid_bounds: BoundingBox) -> OuterSide {
    let min_delta = (coordinate - solid_bounds.axis_min(axis)).abs();
    let max_delta = (coordinate - solid_bounds.axis_max(axis)).abs();

    if min_delta <= max_delta {
        OuterSide::Min
    } else {
        OuterSide::Max
    }
}

fn is_central_bottom_logo_candidate(
    candidate_bbox: BoundingBox,
    solid_bounds: BoundingBox,
) -> bool {
    [0usize, 1usize].into_iter().all(|axis| {
        let solid_span = solid_bounds.axis_max(axis) - solid_bounds.axis_min(axis);
        let edge_clearance =
            BOTTOM_LOGO_EDGE_CLEARANCE_MM.max(solid_span * BOTTOM_LOGO_EDGE_CLEARANCE_RATIO);
        candidate_bbox.axis_min(axis) >= solid_bounds.axis_min(axis) + edge_clearance
            && candidate_bbox.axis_max(axis) <= solid_bounds.axis_max(axis) - edge_clearance
    })
}

fn cover_patch_for_bbox(
    bbox: BoundingBox,
    solid_bounds: BoundingBox,
    axis: usize,
    side: OuterSide,
    style_id: usize,
) -> CoverPatch {
    let mut min = [bbox.min_x, bbox.min_y, bbox.min_z];
    let mut max = [bbox.max_x, bbox.max_y, bbox.max_z];

    for plane_axis in [0usize, 1usize, 2usize] {
        if plane_axis == axis {
            continue;
        }
        min[plane_axis] -= LOGO_COVER_PATCH_MARGIN_MM;
        max[plane_axis] += LOGO_COVER_PATCH_MARGIN_MM;
    }

    match side {
        OuterSide::Max => {
            min[axis] = max[axis].max(solid_bounds.axis_max(axis)) + LOGO_COVER_PATCH_OFFSET_MM;
            max[axis] = min[axis] + LOGO_COVER_PATCH_THICKNESS_MM;
        }
        OuterSide::Min => {
            max[axis] = min[axis].min(solid_bounds.axis_min(axis)) - LOGO_COVER_PATCH_OFFSET_MM;
            min[axis] = max[axis] - LOGO_COVER_PATCH_THICKNESS_MM;
        }
    }

    CoverPatch {
        bbox: BoundingBox {
            min_x: min[0],
            max_x: max[0],
            min_y: min[1],
            max_y: max[1],
            min_z: min[2],
            max_z: max[2],
        },
        style_id,
    }
}

fn push_cover_patch(plan: &mut StyleRewritePlan, patch: CoverPatch) {
    if plan
        .cover_patches
        .iter()
        .any(|existing| cover_patches_match(*existing, patch))
    {
        return;
    }

    plan.cover_patches.push(patch);
}

fn cover_patches_match(left: CoverPatch, right: CoverPatch) -> bool {
    let left_axis = thinnest_axis(left.bbox);
    let right_axis = thinnest_axis(right.bbox);
    if left_axis != right_axis {
        return false;
    }

    projected_bounds_match(
        left.bbox,
        right.bbox,
        left_axis,
        LOGO_COVER_PATCH_DEDUP_TOLERANCE_MM,
    ) && axis_ranges_touch(
        left.bbox,
        right.bbox,
        left_axis,
        LOGO_COVER_PATCH_THICKNESS_MM,
    )
}

fn thinnest_axis(bounds: BoundingBox) -> usize {
    [0usize, 1usize, 2usize]
        .into_iter()
        .min_by(|left, right| {
            axis_span(bounds, *left)
                .partial_cmp(&axis_span(bounds, *right))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(2)
}

fn axis_span(bounds: BoundingBox, axis: usize) -> f64 {
    bounds.axis_max(axis) - bounds.axis_min(axis)
}

fn projected_bounds_match(
    left: BoundingBox,
    right: BoundingBox,
    axis: usize,
    tolerance: f64,
) -> bool {
    plane_axes(axis).into_iter().all(|plane_axis| {
        (left.axis_min(plane_axis) - right.axis_min(plane_axis)).abs() <= tolerance
            && (left.axis_max(plane_axis) - right.axis_max(plane_axis)).abs() <= tolerance
    })
}

fn axis_ranges_touch(left: BoundingBox, right: BoundingBox, axis: usize, tolerance: f64) -> bool {
    left.axis_max(axis) >= right.axis_min(axis) - tolerance
        && right.axis_max(axis) >= left.axis_min(axis) - tolerance
}

fn expanded_geometry_logo_member_indices(
    cluster: &GeometryLogoCluster,
    axis: usize,
    face_styles: &[AnyFaceStyleInfo],
) -> Vec<usize> {
    let mut selected = cluster
        .member_indices
        .iter()
        .copied()
        .collect::<HashSet<_>>();

    for (index, face_style) in face_styles.iter().enumerate() {
        if selected.contains(&index)
            || !colors_match(face_style.color, cluster.color)
            || !projected_bboxes_touch(
                cluster.bbox,
                face_style.bbox,
                axis,
                GEOMETRY_LOGO_BACKGROUND_TOLERANCE_MM,
            )
        {
            continue;
        }

        selected.insert(index);
    }

    let mut indices = selected.into_iter().collect::<Vec<_>>();
    indices.sort_unstable();
    indices
}

fn collect_any_face_style_infos(
    face_ids: &HashSet<usize>,
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
    bbox_cache: &mut HashMap<usize, Option<BoundingBox>>,
) -> Vec<AnyFaceStyleInfo> {
    let mut infos = Vec::new();

    for entity in entities {
        if !starts_with_ascii(&entity.body, b"STYLED_ITEM") || entity.refs.len() < 2 {
            continue;
        }

        let style_id = entity.refs[0];
        let face_id = entity.refs[1];
        if !face_ids.contains(&face_id) {
            continue;
        }

        let Some(face) = entity_by_id
            .get(&face_id)
            .and_then(|index| entities.get(*index))
        else {
            continue;
        };
        if !starts_with_ascii(&face.body, b"ADVANCED_FACE") {
            continue;
        }

        let Some(color) = resolve_colour_rgb(style_id, entities, entity_by_id, &mut HashSet::new())
        else {
            continue;
        };
        let Some(bbox) = bounds_for_entity(
            face_id,
            entities,
            entity_by_id,
            bbox_cache,
            &mut HashSet::new(),
        ) else {
            continue;
        };

        infos.push(AnyFaceStyleInfo {
            styled_item_id: entity.id,
            style_id,
            color,
            bbox,
        });
    }

    infos
}

fn build_geometry_logo_clusters(face_styles: &[AnyFaceStyleInfo]) -> Vec<GeometryLogoCluster> {
    let mut grouped = HashMap::<ColorKey, Vec<usize>>::new();

    for (index, face_style) in face_styles.iter().enumerate() {
        grouped
            .entry(quantize_color(face_style.color))
            .or_default()
            .push(index);
    }

    let mut clusters = Vec::new();

    for (_, indices) in grouped {
        let mut unvisited = indices.into_iter().collect::<HashSet<_>>();

        while let Some(seed_index) = unvisited.iter().next().copied() {
            unvisited.remove(&seed_index);

            let mut member_indices = vec![seed_index];
            let mut stack = vec![seed_index];
            let mut bbox = face_styles[seed_index].bbox;

            while let Some(current_index) = stack.pop() {
                let current_bbox = face_styles[current_index].bbox;
                let remaining = unvisited.iter().copied().collect::<Vec<_>>();

                for candidate_index in remaining {
                    let candidate = &face_styles[candidate_index];
                    if !bboxes_touch_3d(current_bbox, candidate.bbox, LOGO_STYLE_CLUSTER_GAP_MM) {
                        continue;
                    }

                    unvisited.remove(&candidate_index);
                    stack.push(candidate_index);
                    member_indices.push(candidate_index);
                    bbox = bbox.union(candidate.bbox);
                }
            }

            clusters.push(GeometryLogoCluster {
                color: face_styles[seed_index].color,
                bbox,
                member_indices,
            });
        }
    }

    clusters
}

fn geometry_logo_side(
    cluster: &GeometryLogoCluster,
    solid_bounds: BoundingBox,
) -> Option<(usize, OuterSide)> {
    [0usize, 1usize, 2usize]
        .into_iter()
        .flat_map(|axis| {
            let min_delta = (cluster.bbox.axis_min(axis) - solid_bounds.axis_min(axis)).abs();
            let max_delta = (cluster.bbox.axis_max(axis) - solid_bounds.axis_max(axis)).abs();

            match (
                min_delta <= GEOMETRY_LOGO_OUTER_TOLERANCE_MM,
                max_delta <= GEOMETRY_LOGO_OUTER_TOLERANCE_MM,
            ) {
                (true, false) => vec![(min_delta, axis, OuterSide::Min)],
                (false, true) => vec![(max_delta, axis, OuterSide::Max)],
                _ => Vec::new(),
            }
        })
        .min_by(|left, right| {
            left.0
                .partial_cmp(&right.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, axis, side)| (axis, side))
}

fn choose_geometry_logo_replacement(
    cluster: &GeometryLogoCluster,
    axis: usize,
    side: OuterSide,
    face_styles: &[AnyFaceStyleInfo],
) -> Option<usize> {
    let cluster_members = cluster
        .member_indices
        .iter()
        .copied()
        .collect::<HashSet<_>>();

    face_styles
        .iter()
        .enumerate()
        .filter(|(index, replacement)| {
            !cluster_members.contains(index)
                && !colors_match(replacement.color, cluster.color)
                && color_distance(replacement.color, cluster.color) >= LOGO_STYLE_MIN_CONTRAST
                && projected_contains(
                    replacement.bbox,
                    cluster.bbox,
                    axis,
                    GEOMETRY_LOGO_BACKGROUND_TOLERANCE_MM,
                )
                && is_behind_geometry_logo(cluster, replacement.bbox, axis, side)
        })
        .max_by(|(_, left), (_, right)| {
            projected_area(left.bbox, axis)
                .partial_cmp(&projected_area(right.bbox, axis))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, replacement)| replacement.style_id)
}

fn is_behind_geometry_logo(
    cluster: &GeometryLogoCluster,
    bbox: BoundingBox,
    axis: usize,
    side: OuterSide,
) -> bool {
    match side {
        OuterSide::Max => {
            bbox.axis_max(axis) <= cluster.bbox.axis_max(axis) + LOGO_STYLE_PLANE_TOLERANCE_MM
                && bbox.axis_max(axis)
                    >= cluster.bbox.axis_max(axis)
                        - GEOMETRY_LOGO_BACKGROUND_DEPTH_MM
                        - LOGO_STYLE_PLANE_TOLERANCE_MM
        }
        OuterSide::Min => {
            bbox.axis_min(axis) >= cluster.bbox.axis_min(axis) - LOGO_STYLE_PLANE_TOLERANCE_MM
                && bbox.axis_min(axis)
                    <= cluster.bbox.axis_min(axis)
                        + GEOMETRY_LOGO_BACKGROUND_DEPTH_MM
                        + LOGO_STYLE_PLANE_TOLERANCE_MM
        }
    }
}

fn choose_solid_style_fallback(
    cluster: &GeometryLogoCluster,
    solid_styles: &[SolidStyleInfo],
) -> Option<usize> {
    if cluster.member_indices.len() < GEOMETRY_LOGO_SOLID_FALLBACK_MIN_FACES
        || color_luminance(cluster.color) < 0.7
    {
        return None;
    }

    solid_styles
        .iter()
        .filter(|style| {
            color_luminance(style.color) < 0.5
                && color_distance(style.color, cluster.color) >= LOGO_STYLE_MIN_CONTRAST
        })
        .min_by(|left, right| {
            color_luminance(left.color)
                .partial_cmp(&color_luminance(right.color))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|style| style.style_id)
}

fn choose_face_group_solid_style_fallback(
    candidate: &FaceStyleGroup,
    solid_styles: &[SolidStyleInfo],
) -> Option<usize> {
    if candidate.face_count < FACE_LOGO_SOLID_FALLBACK_MIN_FACES
        || color_luminance(candidate.color) < 0.7
    {
        return None;
    }

    solid_styles
        .iter()
        .filter(|style| {
            color_luminance(style.color) < 0.5
                && color_distance(style.color, candidate.color) >= LOGO_STYLE_MIN_CONTRAST
        })
        .min_by(|left, right| {
            color_luminance(left.color)
                .partial_cmp(&color_luminance(right.color))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|style| style.style_id)
}

fn collect_solid_style_infos(
    solids: &[(usize, BoundingBox)],
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
) -> Vec<SolidStyleInfo> {
    let solid_ids = solids
        .iter()
        .map(|(solid_id, _)| *solid_id)
        .collect::<HashSet<_>>();
    let mut infos = Vec::new();

    for entity in entities {
        if !starts_with_ascii(&entity.body, b"STYLED_ITEM") || entity.refs.len() < 2 {
            continue;
        }

        let style_id = entity.refs[0];
        let target_id = entity.refs[1];
        if !solid_ids.contains(&target_id) {
            continue;
        }

        let Some(target) = entity_by_id
            .get(&target_id)
            .and_then(|index| entities.get(*index))
        else {
            continue;
        };
        if !starts_with_ascii(&target.body, b"MANIFOLD_SOLID_BREP") {
            continue;
        }

        let Some(color) = resolve_colour_rgb(style_id, entities, entity_by_id, &mut HashSet::new())
        else {
            continue;
        };

        infos.push(SolidStyleInfo { style_id, color });
    }

    infos
}

fn collect_face_style_infos(
    face_ids: &HashSet<usize>,
    solid_bounds: BoundingBox,
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
    bbox_cache: &mut HashMap<usize, Option<BoundingBox>>,
) -> Vec<FaceStyleInfo> {
    let mut infos = Vec::new();

    for entity in entities {
        if !starts_with_ascii(&entity.body, b"STYLED_ITEM") || entity.refs.len() < 2 {
            continue;
        }

        let style_id = entity.refs[0];
        let face_id = entity.refs[1];
        if !face_ids.contains(&face_id) {
            continue;
        }

        let Some(face) = entity_by_id
            .get(&face_id)
            .and_then(|index| entities.get(*index))
        else {
            continue;
        };
        if !starts_with_ascii(&face.body, b"ADVANCED_FACE") {
            continue;
        }

        let Some(face_info) = parse_planar_face(face, entities, entity_by_id) else {
            continue;
        };
        let Some(color) = resolve_colour_rgb(style_id, entities, entity_by_id, &mut HashSet::new())
        else {
            continue;
        };
        let Some(bbox) = bounds_for_entity(
            face_id,
            entities,
            entity_by_id,
            bbox_cache,
            &mut HashSet::new(),
        ) else {
            continue;
        };

        infos.push(FaceStyleInfo {
            styled_item_id: entity.id,
            style_id,
            color,
            bbox,
            outer_side: classify_outer_side(face_info, solid_bounds),
            face: face_info,
        });
    }

    infos
}

fn build_outer_style_groups(face_styles: &[FaceStyleInfo]) -> Vec<FaceStyleGroup> {
    let mut grouped = HashMap::<(usize, OuterSide, ColorKey), Vec<usize>>::new();

    for (index, face_style) in face_styles.iter().enumerate() {
        let Some(side) = face_style.outer_side else {
            continue;
        };
        let color_key = quantize_color(face_style.color);
        grouped
            .entry((face_style.face.axis, side, color_key))
            .or_default()
            .push(index);
    }

    let mut groups = Vec::new();

    for ((axis, side, _), indices) in grouped {
        let mut unvisited = indices.into_iter().collect::<HashSet<_>>();

        while let Some(seed_index) = unvisited.iter().next().copied() {
            unvisited.remove(&seed_index);

            let mut member_indices = vec![seed_index];
            let mut stack = vec![seed_index];
            let mut bbox = face_styles[seed_index].bbox;
            let mut min_coordinate = face_styles[seed_index].face.coordinate;
            let mut max_coordinate = face_styles[seed_index].face.coordinate;

            while let Some(current_index) = stack.pop() {
                let current_bbox = face_styles[current_index].bbox;
                let current_coordinate = face_styles[current_index].face.coordinate;
                let remaining = unvisited.iter().copied().collect::<Vec<_>>();

                for candidate_index in remaining {
                    let candidate = &face_styles[candidate_index];
                    if current_coordinate_distance(current_coordinate, candidate.face.coordinate)
                        > LOGO_STYLE_PLANE_TOLERANCE_MM
                        || !projected_bboxes_touch(
                            current_bbox,
                            candidate.bbox,
                            axis,
                            LOGO_STYLE_CLUSTER_GAP_MM,
                        )
                    {
                        continue;
                    }

                    unvisited.remove(&candidate_index);
                    stack.push(candidate_index);
                    member_indices.push(candidate_index);
                    bbox = bbox.union(candidate.bbox);
                    min_coordinate = min_coordinate.min(candidate.face.coordinate);
                    max_coordinate = max_coordinate.max(candidate.face.coordinate);
                }
            }

            let representative = &face_styles[seed_index];
            groups.push(FaceStyleGroup {
                representative_style_id: representative.style_id,
                color: representative.color,
                axis,
                side,
                bbox,
                min_coordinate,
                max_coordinate,
                face_count: member_indices.len(),
                member_indices,
            });
        }
    }

    groups
}

fn choose_replacement_group<'a>(
    candidate: &FaceStyleGroup,
    groups: &'a [FaceStyleGroup],
) -> Option<&'a FaceStyleGroup> {
    groups
        .iter()
        .filter(|replacement| {
            replacement.axis == candidate.axis
                && replacement.side == candidate.side
                && !colors_match(replacement.color, candidate.color)
                && projected_area(replacement.bbox, replacement.axis)
                    > projected_area(candidate.bbox, candidate.axis)
                && (replacement.min_coordinate - candidate.min_coordinate).abs()
                    <= LOGO_STYLE_PLANE_TOLERANCE_MM
                && (replacement.max_coordinate - candidate.max_coordinate).abs()
                    <= LOGO_STYLE_PLANE_TOLERANCE_MM
                && projected_contains(replacement.bbox, candidate.bbox, candidate.axis, 0.05)
                && color_distance(candidate.color, replacement.color) >= LOGO_STYLE_MIN_CONTRAST
        })
        .max_by(|left, right| {
            projected_area(left.bbox, left.axis)
                .partial_cmp(&projected_area(right.bbox, right.axis))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn choose_coplanar_surface_replacement_group<'a>(
    candidate: &FaceStyleGroup,
    groups: &'a [FaceStyleGroup],
) -> Option<&'a FaceStyleGroup> {
    groups
        .iter()
        .filter(|replacement| {
            replacement.axis == candidate.axis
                && replacement.side == candidate.side
                && !colors_match(replacement.color, candidate.color)
                && projected_area(replacement.bbox, replacement.axis)
                    > projected_area(candidate.bbox, candidate.axis)
                && (replacement.min_coordinate - candidate.min_coordinate).abs()
                    <= LOGO_STYLE_PLANE_TOLERANCE_MM
                && (replacement.max_coordinate - candidate.max_coordinate).abs()
                    <= LOGO_STYLE_PLANE_TOLERANCE_MM
                && projected_contains(replacement.bbox, candidate.bbox, candidate.axis, 0.05)
                && color_distance(candidate.color, replacement.color) >= LOGO_STYLE_MIN_CONTRAST
        })
        .max_by(|left, right| {
            projected_area(left.bbox, left.axis)
                .partial_cmp(&projected_area(right.bbox, right.axis))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn descendant_faces(
    root_id: usize,
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
) -> Vec<usize> {
    let mut visited = HashSet::new();
    let mut faces = Vec::new();
    collect_descendant_faces(root_id, entities, entity_by_id, &mut visited, &mut faces);
    faces
}

fn collect_descendant_faces(
    entity_id: usize,
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
    visited: &mut HashSet<usize>,
    faces: &mut Vec<usize>,
) {
    if !visited.insert(entity_id) {
        return;
    }

    let Some(entity) = entity_by_id
        .get(&entity_id)
        .and_then(|index| entities.get(*index))
    else {
        return;
    };

    if starts_with_ascii(&entity.body, b"ADVANCED_FACE") {
        faces.push(entity.id);
    }

    for reference in &entity.refs {
        collect_descendant_faces(*reference, entities, entity_by_id, visited, faces);
    }
}

fn parse_planar_face(
    face: &StepEntity,
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
) -> Option<PlanarFaceInfo> {
    let surface_id = *face.refs.last()?;
    let surface = entity_by_id
        .get(&surface_id)
        .and_then(|index| entities.get(*index))?;
    if !starts_with_ascii(&surface.body, b"PLANE") {
        return None;
    }

    let placement_id = *surface.refs.first()?;
    let placement = entity_by_id
        .get(&placement_id)
        .and_then(|index| entities.get(*index))?;
    if !starts_with_ascii(&placement.body, b"AXIS2_PLACEMENT_3D") || placement.refs.len() < 2 {
        return None;
    }

    let origin = entity_by_id
        .get(&placement.refs[0])
        .and_then(|index| entities.get(*index))
        .and_then(|entity| {
            starts_with_ascii(&entity.body, b"CARTESIAN_POINT")
                .then(|| parse_coordinate_tuple(&entity.body))
                .flatten()
        })?;

    let normal = entity_by_id
        .get(&placement.refs[1])
        .and_then(|index| entities.get(*index))
        .and_then(|entity| {
            starts_with_ascii(&entity.body, b"DIRECTION")
                .then(|| parse_coordinate_tuple(&entity.body))
                .flatten()
        })?;

    let (axis, alignment) = dominant_axis(normal)?;
    if alignment < PLANAR_FACE_AXIS_ALIGNMENT_MIN {
        return None;
    }

    Some(PlanarFaceInfo {
        axis,
        coordinate: origin[axis],
    })
}

fn dominant_axis(vector: [f64; 3]) -> Option<(usize, f64)> {
    let candidates = [
        (0usize, vector[0].abs()),
        (1usize, vector[1].abs()),
        (2usize, vector[2].abs()),
    ];

    candidates.into_iter().max_by(|left, right| {
        left.1
            .partial_cmp(&right.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn classify_outer_side(face: PlanarFaceInfo, solid_bounds: BoundingBox) -> Option<OuterSide> {
    let min_delta = (face.coordinate - solid_bounds.axis_min(face.axis)).abs();
    let max_delta = (face.coordinate - solid_bounds.axis_max(face.axis)).abs();

    if min_delta <= OUTER_FACE_TOLERANCE_MM || max_delta <= OUTER_FACE_TOLERANCE_MM {
        Some(if min_delta <= max_delta {
            OuterSide::Min
        } else {
            OuterSide::Max
        })
    } else {
        None
    }
}

fn resolve_colour_rgb(
    entity_id: usize,
    entities: &[StepEntity],
    entity_by_id: &HashMap<usize, usize>,
    visiting: &mut HashSet<usize>,
) -> Option<[f64; 3]> {
    if !visiting.insert(entity_id) {
        return None;
    }

    let result = entity_by_id
        .get(&entity_id)
        .and_then(|index| entities.get(*index))
        .and_then(|entity| {
            if starts_with_ascii(&entity.body, b"COLOUR_RGB") {
                parse_colour_rgb(&entity.body)
            } else {
                entity.refs.iter().find_map(|reference| {
                    resolve_colour_rgb(*reference, entities, entity_by_id, visiting)
                })
            }
        });

    visiting.remove(&entity_id);
    result
}

fn parse_colour_rgb(body: &[u8]) -> Option<[f64; 3]> {
    let open_index = body.iter().position(|&byte| byte == b'(')?;
    let first_comma = body[open_index + 1..]
        .iter()
        .position(|&byte| byte == b',')
        .map(|offset| open_index + 1 + offset)?;
    let close_index = body[first_comma + 1..]
        .iter()
        .rposition(|&byte| byte == b')')
        .map(|offset| first_comma + 1 + offset)?;

    let coords = std::str::from_utf8(&body[first_comma + 1..close_index]).ok()?;
    let values = coords
        .split(',')
        .map(|value| value.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;

    (values.len() >= 3).then_some([values[0], values[1], values[2]])
}

fn plane_axes(axis: usize) -> [usize; 2] {
    match axis {
        0 => [1usize, 2usize],
        1 => [0usize, 2usize],
        _ => [0usize, 1usize],
    }
}

fn projected_area(bounds: BoundingBox, axis: usize) -> f64 {
    projected_span(bounds, axis, 0) * projected_span(bounds, axis, 1)
}

fn projected_span(bounds: BoundingBox, axis: usize, plane_component: usize) -> f64 {
    let plane_axes = match axis {
        0 => [1usize, 2usize],
        1 => [0usize, 2usize],
        _ => [0usize, 1usize],
    };
    let component_axis = plane_axes[plane_component];
    bounds.axis_max(component_axis) - bounds.axis_min(component_axis)
}

fn projected_contains(outer: BoundingBox, inner: BoundingBox, axis: usize, tolerance: f64) -> bool {
    let plane_axes = match axis {
        0 => [1usize, 2usize],
        1 => [0usize, 2usize],
        _ => [0usize, 1usize],
    };

    plane_axes.into_iter().all(|plane_axis| {
        inner.axis_min(plane_axis) >= outer.axis_min(plane_axis) - tolerance
            && inner.axis_max(plane_axis) <= outer.axis_max(plane_axis) + tolerance
    })
}

fn color_distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    ((left[0] - right[0]).powi(2) + (left[1] - right[1]).powi(2) + (left[2] - right[2]).powi(2))
        .sqrt()
}

fn color_luminance(color: [f64; 3]) -> f64 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

fn colors_match(left: [f64; 3], right: [f64; 3]) -> bool {
    quantize_color(left) == quantize_color(right)
}

fn quantize_color(color: [f64; 3]) -> ColorKey {
    ColorKey(color.map(|component| (component * 1000.0).round() as i32))
}

fn quantize_scalar(value: f64, tolerance: f64) -> i32 {
    (value / tolerance.max(f64::EPSILON)).round() as i32
}

fn is_logo_candidate_color(color: [f64; 3]) -> bool {
    let min = color[0].min(color[1]).min(color[2]);
    let max = color[0].max(color[1]).max(color[2]);

    (max - min) <= 0.1
}

fn current_coordinate_distance(left: f64, right: f64) -> f64 {
    (left - right).abs()
}

fn projected_bboxes_touch(
    left: BoundingBox,
    right: BoundingBox,
    axis: usize,
    tolerance: f64,
) -> bool {
    let plane_axes = match axis {
        0 => [1usize, 2usize],
        1 => [0usize, 2usize],
        _ => [0usize, 1usize],
    };

    plane_axes.into_iter().all(|plane_axis| {
        left.axis_max(plane_axis) >= right.axis_min(plane_axis) - tolerance
            && right.axis_max(plane_axis) >= left.axis_min(plane_axis) - tolerance
    })
}

fn bboxes_touch_3d(left: BoundingBox, right: BoundingBox, tolerance: f64) -> bool {
    [0usize, 1usize, 2usize].into_iter().all(|axis| {
        left.axis_max(axis) >= right.axis_min(axis) - tolerance
            && right.axis_max(axis) >= left.axis_min(axis) - tolerance
    })
}

fn collect_representation_replacements(
    entities: &[StepEntity],
    removable_solids: &HashSet<usize>,
    added_representation_items: &[usize],
) -> Vec<(std::ops::Range<usize>, Vec<u8>)> {
    if removable_solids.is_empty() && added_representation_items.is_empty() {
        return Vec::new();
    }

    let mut replacements = Vec::new();

    for entity in entities {
        if !starts_with_ascii(&entity.body, b"ADVANCED_BREP_SHAPE_REPRESENTATION") {
            continue;
        }

        let Some((list_start, list_end)) = top_level_ref_list_range(&entity.body) else {
            continue;
        };

        let list_refs = parse_refs(&entity.body[list_start + 1..list_end]);
        let mut kept_refs = list_refs
            .iter()
            .copied()
            .filter(|reference| !removable_solids.contains(reference))
            .map(|reference| format!("#{}", reference))
            .collect::<Vec<_>>();
        kept_refs.extend(
            added_representation_items
                .iter()
                .map(|reference| format!("#{}", reference)),
        );

        if (removable_solids.is_empty() && added_representation_items.is_empty())
            || kept_refs.is_empty()
            || (added_representation_items.is_empty() && kept_refs.len() == list_refs.len())
        {
            continue;
        }

        let mut updated_body = entity.body.clone();
        updated_body.splice(
            list_start + 1..list_end,
            kept_refs.join(", ").as_bytes().iter().copied(),
        );
        replacements.push((entity.span.clone(), updated_body));
    }

    replacements
}

fn max_entity_id(entities: &[StepEntity]) -> usize {
    entities
        .iter()
        .map(|entity| entity.id)
        .max()
        .unwrap_or_default()
}

fn build_cover_patch_entities(patches: &[CoverPatch], mut next_id: usize) -> (Vec<usize>, Vec<u8>) {
    if patches.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let mut brep_ids = Vec::new();
    let mut output = String::new();

    for patch in patches {
        let min = [patch.bbox.min_x, patch.bbox.min_y, patch.bbox.min_z];
        let max = [patch.bbox.max_x, patch.bbox.max_y, patch.bbox.max_z];
        let points = [
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], max[1], min[2]],
            [min[0], max[1], min[2]],
            [min[0], min[1], max[2]],
            [max[0], min[1], max[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ];

        let mut point_ids = [0usize; 8];
        for (index, point) in points.iter().enumerate() {
            point_ids[index] = next_id;
            next_id += 1;
            let _ = writeln!(
                output,
                "#{} = CARTESIAN_POINT ( 'logo cover', ( {:.6}, {:.6}, {:.6} ) ) ;",
                point_ids[index], point[0], point[1], point[2]
            );
        }

        let mut vertex_ids = [0usize; 8];
        for (index, point_id) in point_ids.iter().enumerate() {
            vertex_ids[index] = next_id;
            next_id += 1;
            let _ = writeln!(
                output,
                "#{} = VERTEX_POINT ( 'logo cover', #{} ) ;",
                vertex_ids[index], point_id
            );
        }

        let edge_point_indices = [
            (0usize, 1usize),
            (1usize, 2usize),
            (2usize, 3usize),
            (3usize, 0usize),
            (4usize, 5usize),
            (5usize, 6usize),
            (6usize, 7usize),
            (7usize, 4usize),
            (0usize, 4usize),
            (1usize, 5usize),
            (2usize, 6usize),
            (3usize, 7usize),
        ];
        let mut edge_ids = Vec::new();

        for (start_index, end_index) in edge_point_indices {
            let start = points[start_index];
            let end = points[end_index];
            let delta = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
            let length = (delta[0].powi(2) + delta[1].powi(2) + delta[2].powi(2)).sqrt();
            if length <= f64::EPSILON {
                continue;
            }

            let direction_id = next_id;
            next_id += 1;
            let vector_id = next_id;
            next_id += 1;
            let line_id = next_id;
            next_id += 1;
            let edge_id = next_id;
            next_id += 1;
            edge_ids.push((start_index, end_index, edge_id));

            let _ = writeln!(
                output,
                "#{} = DIRECTION ( 'logo cover', ( {:.6}, {:.6}, {:.6} ) ) ;",
                direction_id,
                delta[0] / length,
                delta[1] / length,
                delta[2] / length
            );
            let _ = writeln!(
                output,
                "#{} = VECTOR ( 'logo cover', #{}, {:.6} ) ;",
                vector_id, direction_id, length
            );
            let _ = writeln!(
                output,
                "#{} = LINE ( 'logo cover', #{}, #{} ) ;",
                line_id, point_ids[start_index], vector_id
            );
            let _ = writeln!(
                output,
                "#{} = EDGE_CURVE ( 'logo cover', #{}, #{}, #{}, .T. ) ;",
                edge_id, vertex_ids[start_index], vertex_ids[end_index], line_id
            );
        }

        let face_point_indices = cover_patch_face_point_indices();
        let mut face_ids = Vec::new();

        for face_points in face_point_indices {
            let mut oriented_edge_ids = Vec::new();
            for index in 0..face_points.len() {
                let start = face_points[index];
                let end = face_points[(index + 1) % face_points.len()];
                let Some((edge_start, edge_end, edge_id)) =
                    edge_ids.iter().find(|(edge_start, edge_end, _)| {
                        (*edge_start == start && *edge_end == end)
                            || (*edge_start == end && *edge_end == start)
                    })
                else {
                    continue;
                };

                let oriented_edge_id = next_id;
                next_id += 1;
                oriented_edge_ids.push(oriented_edge_id);
                let _ = writeln!(
                    output,
                    "#{} = ORIENTED_EDGE ( 'logo cover', *, *, #{}, {} ) ;",
                    oriented_edge_id,
                    edge_id,
                    if *edge_start == start && *edge_end == end {
                        ".T."
                    } else {
                        ".F."
                    }
                );
            }

            if oriented_edge_ids.len() != 4 {
                continue;
            }

            let loop_id = next_id;
            next_id += 1;
            let bound_id = next_id;
            next_id += 1;
            let face_id = next_id;
            next_id += 1;
            let normal_direction_id = next_id;
            next_id += 1;
            let ref_direction_id = next_id;
            next_id += 1;
            let placement_id = next_id;
            next_id += 1;
            let plane_id = next_id;
            next_id += 1;
            face_ids.push(face_id);
            let styled_item_id = next_id;
            next_id += 1;

            let edge_refs = oriented_edge_ids
                .iter()
                .map(|edge_id| format!("#{}", edge_id))
                .collect::<Vec<_>>()
                .join(", ");
            let origin = points[face_points[0]];
            let first_edge = normalize_vector([
                points[face_points[1]][0] - origin[0],
                points[face_points[1]][1] - origin[1],
                points[face_points[1]][2] - origin[2],
            ])
            .unwrap_or([1.0, 0.0, 0.0]);
            let second_edge = [
                points[face_points[2]][0] - points[face_points[1]][0],
                points[face_points[2]][1] - points[face_points[1]][1],
                points[face_points[2]][2] - points[face_points[1]][2],
            ];
            let normal =
                normalize_vector(cross_product(first_edge, second_edge)).unwrap_or([0.0, 0.0, 1.0]);

            let _ = writeln!(
                output,
                "#{} = EDGE_LOOP ( 'logo cover', ( {} ) ) ;",
                loop_id, edge_refs
            );
            let _ = writeln!(
                output,
                "#{} = FACE_OUTER_BOUND ( 'logo cover', #{}, .T. ) ;",
                bound_id, loop_id
            );
            let _ = writeln!(
                output,
                "#{} = DIRECTION ( 'logo cover', ( {:.6}, {:.6}, {:.6} ) ) ;",
                normal_direction_id, normal[0], normal[1], normal[2]
            );
            let _ = writeln!(
                output,
                "#{} = DIRECTION ( 'logo cover', ( {:.6}, {:.6}, {:.6} ) ) ;",
                ref_direction_id, first_edge[0], first_edge[1], first_edge[2]
            );
            let _ = writeln!(
                output,
                "#{} = AXIS2_PLACEMENT_3D ( 'logo cover', #{}, #{}, #{} ) ;",
                placement_id, point_ids[face_points[0]], normal_direction_id, ref_direction_id
            );
            let _ = writeln!(
                output,
                "#{} = PLANE ( 'logo cover', #{} ) ;",
                plane_id, placement_id
            );
            let _ = writeln!(
                output,
                "#{} = ADVANCED_FACE ( 'logo cover', ( #{} ), #{}, .T. ) ;",
                face_id, bound_id, plane_id
            );
            let _ = writeln!(
                output,
                "#{} = STYLED_ITEM ( 'NONE', ( #{} ), #{} ) ;",
                styled_item_id, patch.style_id, face_id
            );
        }

        let shell_id = next_id;
        next_id += 1;
        let brep_id = next_id;
        next_id += 1;
        let styled_item_id = next_id;
        next_id += 1;
        brep_ids.push(brep_id);

        let face_refs = face_ids
            .iter()
            .map(|face_id| format!("#{}", face_id))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            output,
            "#{} = CLOSED_SHELL ( 'logo cover', ( {} ) ) ;",
            shell_id, face_refs
        );
        let _ = writeln!(
            output,
            "#{} = MANIFOLD_SOLID_BREP ( 'logo cover', #{} ) ;",
            brep_id, shell_id
        );
        let _ = writeln!(
            output,
            "#{} = STYLED_ITEM ( 'NONE', ( #{} ), #{} ) ;",
            styled_item_id, patch.style_id, brep_id
        );
    }

    (brep_ids, output.into_bytes())
}

fn cover_patch_face_point_indices() -> [[usize; 4]; 6] {
    [
        [0usize, 3usize, 2usize, 1usize],
        [4usize, 5usize, 6usize, 7usize],
        [0usize, 1usize, 5usize, 4usize],
        [1usize, 2usize, 6usize, 5usize],
        [2usize, 3usize, 7usize, 6usize],
        [3usize, 0usize, 4usize, 7usize],
    ]
}

fn normalize_vector(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = (vector[0].powi(2) + vector[1].powi(2) + vector[2].powi(2)).sqrt();
    (length > f64::EPSILON).then_some([vector[0] / length, vector[1] / length, vector[2] / length])
}

fn cross_product(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn append_step_entities(step_data: &[u8], entities: &[u8]) -> Vec<u8> {
    if entities.is_empty() {
        return step_data.to_vec();
    }

    let Some(insert_at) = find_last_subslice(step_data, b"ENDSEC") else {
        let mut updated = step_data.to_vec();
        updated.extend_from_slice(b"\n");
        updated.extend_from_slice(entities);
        return updated;
    };

    let mut updated = Vec::with_capacity(step_data.len() + entities.len() + 1);
    updated.extend_from_slice(&step_data[..insert_at]);
    if !updated.ends_with(b"\n") {
        updated.extend_from_slice(b"\n");
    }
    updated.extend_from_slice(entities);
    updated.extend_from_slice(&step_data[insert_at..]);
    updated
}

fn find_last_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn collect_styled_item_replacements(
    entities: &[StepEntity],
    style_replacements: &HashMap<usize, usize>,
) -> Vec<(std::ops::Range<usize>, Vec<u8>)> {
    if style_replacements.is_empty() {
        return Vec::new();
    }

    let mut replacements = Vec::new();

    for entity in entities {
        if !starts_with_ascii(&entity.body, b"STYLED_ITEM") {
            continue;
        }

        let Some(&replacement_style_id) = style_replacements.get(&entity.id) else {
            continue;
        };
        let Some((list_start, list_end)) = top_level_ref_list_range(&entity.body) else {
            continue;
        };

        let list_refs = parse_refs(&entity.body[list_start + 1..list_end]);
        if list_refs.len() != 1 || list_refs[0] == replacement_style_id {
            continue;
        }

        let mut updated_body = entity.body.clone();
        updated_body.splice(
            list_start + 1..list_end,
            format!("#{}", replacement_style_id)
                .as_bytes()
                .iter()
                .copied(),
        );
        replacements.push((entity.span.clone(), updated_body));
    }

    replacements
}

fn apply_replacements(
    step_data: &[u8],
    mut replacements: Vec<(std::ops::Range<usize>, Vec<u8>)>,
) -> Vec<u8> {
    if replacements.is_empty() {
        return step_data.to_vec();
    }

    replacements.sort_by_key(|(span, _)| span.start);

    let mut rewritten = Vec::with_capacity(step_data.len());
    let mut cursor = 0usize;

    for (span, replacement) in replacements {
        rewritten.extend_from_slice(&step_data[cursor..span.start]);
        rewritten.extend_from_slice(&replacement);
        cursor = span.end;
    }

    rewritten.extend_from_slice(&step_data[cursor..]);
    rewritten
}

fn top_level_ref_list_range(body: &[u8]) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut commas_at_depth_one = 0usize;
    let mut list_start = None;

    for (index, byte) in body.iter().copied().enumerate() {
        match byte {
            b'(' => {
                depth += 1;
                if depth == 2 && commas_at_depth_one >= 1 && list_start.is_none() {
                    list_start = Some(index);
                }
            }
            b')' => {
                if depth == 2 && list_start.is_some() {
                    return Some((list_start?, index));
                }
                depth = depth.saturating_sub(1);
            }
            b',' if depth == 1 => commas_at_depth_one += 1,
            _ => {}
        }
    }

    None
}

fn starts_with_ascii(body: &[u8], prefix: &[u8]) -> bool {
    body.starts_with(prefix)
}

fn parse_coordinate_tuple(body: &[u8]) -> Option<[f64; 3]> {
    let comma_index = body.iter().position(|&byte| byte == b',')?;
    let open_index = body[comma_index + 1..]
        .iter()
        .position(|&byte| byte == b'(')
        .map(|offset| comma_index + 1 + offset)?;
    let close_index = body[open_index + 1..]
        .iter()
        .position(|&byte| byte == b')')
        .map(|offset| open_index + 1 + offset)?;

    let coords = std::str::from_utf8(&body[open_index + 1..close_index]).ok()?;
    let values = coords
        .split(',')
        .map(|value| value.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;

    (values.len() >= 3).then_some([values[0], values[1], values[2]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occurrence_count(haystack: &str, needle: &str) -> usize {
        haystack.match_indices(needle).count()
    }

    #[test]
    fn removes_shallow_top_solids_from_representation() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.0 ) ) ;\n\
#2 = CARTESIAN_POINT ( 'NONE',  ( 10.0, 10.0, 1.0 ) ) ;\n\
#3 = CLOSED_SHELL ( 'NONE', ( #1, #2 ) ) ;\n\
#4 = MANIFOLD_SOLID_BREP ( 'body', #3 ) ;\n\
#5 = CARTESIAN_POINT ( 'NONE',  ( 1.0, 1.0, 1.0 ) ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 2.0, 1.001 ) ) ;\n\
#7 = CLOSED_SHELL ( 'NONE', ( #5, #6 ) ) ;\n\
#8 = MANIFOLD_SOLID_BREP ( 'mark', #7 ) ;\n\
#9 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #4, #8, #10 ), #11 ) ;\n\
#10 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #1, #1 ) ;\n\
#11 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #1, #1 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 2,
                removed_solids: 1,
                restyled_faces: 0,
                restyled_groups: 0,
            }
        );
        assert!(sanitized.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"));
        assert!(sanitized.contains("#4"));
        assert!(sanitized.contains("#10"));
        assert!(sanitized.contains("#11"));
        assert!(!sanitized.contains("( #4, #8, #10 )"));
    }

    #[test]
    fn keeps_non_shallow_top_solids() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.0 ) ) ;\n\
#2 = CARTESIAN_POINT ( 'NONE',  ( 10.0, 10.0, 1.0 ) ) ;\n\
#3 = CLOSED_SHELL ( 'NONE', ( #1, #2 ) ) ;\n\
#4 = MANIFOLD_SOLID_BREP ( 'body', #3 ) ;\n\
#5 = CARTESIAN_POINT ( 'NONE',  ( 1.0, 1.0, 0.6 ) ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 2.0, 1.0 ) ) ;\n\
#7 = CLOSED_SHELL ( 'NONE', ( #5, #6 ) ) ;\n\
#8 = MANIFOLD_SOLID_BREP ( 'feature', #7 ) ;\n\
#9 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #4, #8 ), #10 ) ;\n\
#10 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #1, #1 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 2,
                removed_solids: 0,
                restyled_faces: 0,
                restyled_groups: 0,
            }
        );
        assert_eq!(sanitized, step);
    }

    #[test]
    fn removes_shallow_top_solids_below_taller_leads() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.0 ) ) ;\n\
#2 = CARTESIAN_POINT ( 'NONE',  ( 10.0, 10.0, 1.8 ) ) ;\n\
#3 = CLOSED_SHELL ( 'NONE', ( #1, #2 ) ) ;\n\
#4 = MANIFOLD_SOLID_BREP ( 'body', #3 ) ;\n\
#5 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 2.0, 1.0 ) ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( 4.0, 4.0, 1.001 ) ) ;\n\
#7 = CLOSED_SHELL ( 'NONE', ( #5, #6 ) ) ;\n\
#8 = MANIFOLD_SOLID_BREP ( 'mark', #7 ) ;\n\
#9 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #4, #8 ), #10 ) ;\n\
#10 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #1, #1 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 2,
                removed_solids: 1,
                restyled_faces: 0,
                restyled_groups: 0,
            }
        );
        assert!(!sanitized.contains("( #4, #8 )"));
    }

    #[test]
    fn restyles_small_outer_face_logo_clusters() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.0 ) ) ;\n\
#2 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#3 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#4 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #2, #3 ) ;\n\
#5 = PLANE ( 'NONE', #4 ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 1.0 ) ) ;\n\
#7 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 1.0 ) ) ;\n\
#8 = EDGE_LOOP ( 'NONE', ( #6, #7 ) ) ;\n\
#9 = FACE_OUTER_BOUND ( 'NONE', #8, .T. ) ;\n\
#10 = ADVANCED_FACE ( 'NONE', ( #9 ), #5, .T. ) ;\n\
#11 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#12 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 0.6, 1.0 ) ) ;\n\
#13 = EDGE_LOOP ( 'NONE', ( #11, #12 ) ) ;\n\
#14 = FACE_OUTER_BOUND ( 'NONE', #13, .T. ) ;\n\
#15 = ADVANCED_FACE ( 'NONE', ( #14 ), #5, .T. ) ;\n\
#16 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.0, 1.0 ) ) ;\n\
#17 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 0.6, 1.0 ) ) ;\n\
#18 = EDGE_LOOP ( 'NONE', ( #16, #17 ) ) ;\n\
#19 = FACE_OUTER_BOUND ( 'NONE', #18, .T. ) ;\n\
#20 = ADVANCED_FACE ( 'NONE', ( #19 ), #5, .T. ) ;\n\
#21 = CARTESIAN_POINT ( 'NONE',  ( 1.4, 0.0, 1.0 ) ) ;\n\
#22 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 0.6, 1.0 ) ) ;\n\
#23 = EDGE_LOOP ( 'NONE', ( #21, #22 ) ) ;\n\
#24 = FACE_OUTER_BOUND ( 'NONE', #23, .T. ) ;\n\
#25 = ADVANCED_FACE ( 'NONE', ( #24 ), #5, .T. ) ;\n\
#26 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.7, 1.0 ) ) ;\n\
#27 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 1.3, 1.0 ) ) ;\n\
#28 = EDGE_LOOP ( 'NONE', ( #26, #27 ) ) ;\n\
#29 = FACE_OUTER_BOUND ( 'NONE', #28, .T. ) ;\n\
#30 = ADVANCED_FACE ( 'NONE', ( #29 ), #5, .T. ) ;\n\
#31 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.7, 1.0 ) ) ;\n\
#32 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 1.3, 1.0 ) ) ;\n\
#33 = EDGE_LOOP ( 'NONE', ( #31, #32 ) ) ;\n\
#34 = FACE_OUTER_BOUND ( 'NONE', #33, .T. ) ;\n\
#35 = ADVANCED_FACE ( 'NONE', ( #34 ), #5, .T. ) ;\n\
#36 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 0.0 ) ) ;\n\
#37 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 0.0 ) ) ;\n\
#38 = EDGE_LOOP ( 'NONE', ( #36, #37 ) ) ;\n\
#39 = FACE_OUTER_BOUND ( 'NONE', #38, .T. ) ;\n\
#40 = DIRECTION ( 'NONE',  ( 0.0, 0.0, -1.0 ) ) ;\n\
#41 = AXIS2_PLACEMENT_3D ( 'NONE', #36, #40, #3 ) ;\n\
#42 = PLANE ( 'NONE', #41 ) ;\n\
#43 = ADVANCED_FACE ( 'NONE', ( #39 ), #42, .T. ) ;\n\
#44 = CLOSED_SHELL ( 'NONE', ( #10, #15, #20, #25, #30, #35, #43 ) ) ;\n\
#45 = MANIFOLD_SOLID_BREP ( 'body', #44 ) ;\n\
#46 = SURFACE_STYLE_FILL_AREA ( #47 ) ;\n\
#47 = FILL_AREA_STYLE ( '', ( #48 ) ) ;\n\
#48 = FILL_AREA_STYLE_COLOUR ( '', #49 ) ;\n\
#49 = COLOUR_RGB ( '', 0.25, 0.25, 0.25 ) ;\n\
#50 = SURFACE_STYLE_USAGE ( .BOTH. , #51 ) ;\n\
#51 = SURFACE_SIDE_STYLE ( '', ( #46 ) ) ;\n\
#52 = PRESENTATION_STYLE_ASSIGNMENT (( #50 )) ;\n\
#53 = SURFACE_STYLE_FILL_AREA ( #54 ) ;\n\
#54 = FILL_AREA_STYLE ( '', ( #55 ) ) ;\n\
#55 = FILL_AREA_STYLE_COLOUR ( '', #56 ) ;\n\
#56 = COLOUR_RGB ( '', 1.0, 1.0, 1.0 ) ;\n\
#57 = SURFACE_STYLE_USAGE ( .BOTH. , #58 ) ;\n\
#58 = SURFACE_SIDE_STYLE ( '', ( #53 ) ) ;\n\
#59 = PRESENTATION_STYLE_ASSIGNMENT (( #57 )) ;\n\
#60 = STYLED_ITEM ( 'NONE', ( #52 ), #10 ) ;\n\
#61 = STYLED_ITEM ( 'NONE', ( #59 ), #15 ) ;\n\
#62 = STYLED_ITEM ( 'NONE', ( #59 ), #20 ) ;\n\
#63 = STYLED_ITEM ( 'NONE', ( #59 ), #25 ) ;\n\
#64 = STYLED_ITEM ( 'NONE', ( #59 ), #30 ) ;\n\
#65 = STYLED_ITEM ( 'NONE', ( #59 ), #35 ) ;\n\
#66 = STYLED_ITEM ( 'NONE', ( #52 ), #43 ) ;\n\
#67 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #45 ), #4 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 1,
                removed_solids: 0,
                restyled_faces: 5,
                restyled_groups: 1,
            }
        );
        assert!(sanitized.contains("#61 = STYLED_ITEM ( 'NONE', (#52), #15 )"));
        assert!(sanitized.contains("#62 = STYLED_ITEM ( 'NONE', (#52), #20 )"));
        assert!(sanitized.contains("#63 = STYLED_ITEM ( 'NONE', (#52), #25 )"));
        assert!(sanitized.contains("#64 = STYLED_ITEM ( 'NONE', (#52), #30 )"));
        assert!(sanitized.contains("#65 = STYLED_ITEM ( 'NONE', (#52), #35 )"));
        assert!(!sanitized.contains("MANIFOLD_SOLID_BREP ( 'logo cover'"));
    }

    #[test]
    fn restyles_disconnected_top_letter_groups() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.0 ) ) ;\n\
#2 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#3 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#4 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #2, #3 ) ;\n\
#5 = PLANE ( 'NONE', #4 ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 1.0 ) ) ;\n\
#7 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 1.0 ) ) ;\n\
#8 = EDGE_LOOP ( 'NONE', ( #6, #7 ) ) ;\n\
#9 = FACE_OUTER_BOUND ( 'NONE', #8, .T. ) ;\n\
#10 = ADVANCED_FACE ( 'NONE', ( #9 ), #5, .T. ) ;\n\
#11 = CARTESIAN_POINT ( 'NONE',  ( -2.0, -0.3, 1.0 ) ) ;\n\
#12 = CARTESIAN_POINT ( 'NONE',  ( -1.6, 0.3, 1.0 ) ) ;\n\
#13 = EDGE_LOOP ( 'NONE', ( #11, #12 ) ) ;\n\
#14 = FACE_OUTER_BOUND ( 'NONE', #13, .T. ) ;\n\
#15 = ADVANCED_FACE ( 'NONE', ( #14 ), #5, .T. ) ;\n\
#16 = CARTESIAN_POINT ( 'NONE',  ( -1.0, -0.3, 1.0 ) ) ;\n\
#17 = CARTESIAN_POINT ( 'NONE',  ( -0.6, 0.3, 1.0 ) ) ;\n\
#18 = EDGE_LOOP ( 'NONE', ( #16, #17 ) ) ;\n\
#19 = FACE_OUTER_BOUND ( 'NONE', #18, .T. ) ;\n\
#20 = ADVANCED_FACE ( 'NONE', ( #19 ), #5, .T. ) ;\n\
#21 = CARTESIAN_POINT ( 'NONE',  ( 0.0, -0.3, 1.0 ) ) ;\n\
#22 = CARTESIAN_POINT ( 'NONE',  ( 0.4, 0.3, 1.0 ) ) ;\n\
#23 = EDGE_LOOP ( 'NONE', ( #21, #22 ) ) ;\n\
#24 = FACE_OUTER_BOUND ( 'NONE', #23, .T. ) ;\n\
#25 = ADVANCED_FACE ( 'NONE', ( #24 ), #5, .T. ) ;\n\
#26 = CARTESIAN_POINT ( 'NONE',  ( 1.0, -0.3, 1.0 ) ) ;\n\
#27 = CARTESIAN_POINT ( 'NONE',  ( 1.4, 0.3, 1.0 ) ) ;\n\
#28 = EDGE_LOOP ( 'NONE', ( #26, #27 ) ) ;\n\
#29 = FACE_OUTER_BOUND ( 'NONE', #28, .T. ) ;\n\
#30 = ADVANCED_FACE ( 'NONE', ( #29 ), #5, .T. ) ;\n\
#31 = CARTESIAN_POINT ( 'NONE',  ( 2.0, -0.3, 1.0 ) ) ;\n\
#32 = CARTESIAN_POINT ( 'NONE',  ( 2.4, 0.3, 1.0 ) ) ;\n\
#33 = EDGE_LOOP ( 'NONE', ( #31, #32 ) ) ;\n\
#34 = FACE_OUTER_BOUND ( 'NONE', #33, .T. ) ;\n\
#35 = ADVANCED_FACE ( 'NONE', ( #34 ), #5, .T. ) ;\n\
#36 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 0.0 ) ) ;\n\
#37 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 0.0 ) ) ;\n\
#38 = EDGE_LOOP ( 'NONE', ( #36, #37 ) ) ;\n\
#39 = FACE_OUTER_BOUND ( 'NONE', #38, .T. ) ;\n\
#40 = DIRECTION ( 'NONE',  ( 0.0, 0.0, -1.0 ) ) ;\n\
#41 = AXIS2_PLACEMENT_3D ( 'NONE', #36, #40, #3 ) ;\n\
#42 = PLANE ( 'NONE', #41 ) ;\n\
#43 = ADVANCED_FACE ( 'NONE', ( #39 ), #42, .T. ) ;\n\
#44 = CLOSED_SHELL ( 'NONE', ( #10, #15, #20, #25, #30, #35, #43 ) ) ;\n\
#45 = MANIFOLD_SOLID_BREP ( 'body', #44 ) ;\n\
#46 = SURFACE_STYLE_FILL_AREA ( #47 ) ;\n\
#47 = FILL_AREA_STYLE ( '', ( #48 ) ) ;\n\
#48 = FILL_AREA_STYLE_COLOUR ( '', #49 ) ;\n\
#49 = COLOUR_RGB ( '', 0.25, 0.25, 0.25 ) ;\n\
#50 = SURFACE_STYLE_USAGE ( .BOTH. , #51 ) ;\n\
#51 = SURFACE_SIDE_STYLE ( '', ( #46 ) ) ;\n\
#52 = PRESENTATION_STYLE_ASSIGNMENT (( #50 )) ;\n\
#53 = SURFACE_STYLE_FILL_AREA ( #54 ) ;\n\
#54 = FILL_AREA_STYLE ( '', ( #55 ) ) ;\n\
#55 = FILL_AREA_STYLE_COLOUR ( '', #56 ) ;\n\
#56 = COLOUR_RGB ( '', 1.0, 1.0, 1.0 ) ;\n\
#57 = SURFACE_STYLE_USAGE ( .BOTH. , #58 ) ;\n\
#58 = SURFACE_SIDE_STYLE ( '', ( #53 ) ) ;\n\
#59 = PRESENTATION_STYLE_ASSIGNMENT (( #57 )) ;\n\
#60 = STYLED_ITEM ( 'NONE', ( #52 ), #10 ) ;\n\
#61 = STYLED_ITEM ( 'NONE', ( #59 ), #15 ) ;\n\
#62 = STYLED_ITEM ( 'NONE', ( #59 ), #20 ) ;\n\
#63 = STYLED_ITEM ( 'NONE', ( #59 ), #25 ) ;\n\
#64 = STYLED_ITEM ( 'NONE', ( #59 ), #30 ) ;\n\
#65 = STYLED_ITEM ( 'NONE', ( #59 ), #35 ) ;\n\
#66 = STYLED_ITEM ( 'NONE', ( #52 ), #43 ) ;\n\
#67 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #45 ), #4 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 1,
                removed_solids: 0,
                restyled_faces: 5,
                restyled_groups: 1,
            }
        );
        assert!(sanitized.contains("#61 = STYLED_ITEM ( 'NONE', (#52), #15 )"));
        assert!(sanitized.contains("#62 = STYLED_ITEM ( 'NONE', (#52), #20 )"));
        assert!(sanitized.contains("#63 = STYLED_ITEM ( 'NONE', (#52), #25 )"));
        assert!(sanitized.contains("#64 = STYLED_ITEM ( 'NONE', (#52), #30 )"));
        assert!(sanitized.contains("#65 = STYLED_ITEM ( 'NONE', (#52), #35 )"));
        assert!(!sanitized.contains("MANIFOLD_SOLID_BREP ( 'logo cover'"));
    }

    #[test]
    fn restyles_small_three_face_logo_clusters() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#2 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#3 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#4 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #2, #3 ) ;\n\
#5 = PLANE ( 'NONE', #4 ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 1.0 ) ) ;\n\
#7 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 1.0 ) ) ;\n\
#8 = EDGE_LOOP ( 'NONE', ( #6, #7 ) ) ;\n\
#9 = FACE_OUTER_BOUND ( 'NONE', #8, .T. ) ;\n\
#10 = ADVANCED_FACE ( 'NONE', ( #9 ), #5, .T. ) ;\n\
#11 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#12 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 0.6, 1.0 ) ) ;\n\
#13 = EDGE_LOOP ( 'NONE', ( #11, #12 ) ) ;\n\
#14 = FACE_OUTER_BOUND ( 'NONE', #13, .T. ) ;\n\
#15 = ADVANCED_FACE ( 'NONE', ( #14 ), #5, .T. ) ;\n\
#16 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.0, 1.0 ) ) ;\n\
#17 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 0.6, 1.0 ) ) ;\n\
#18 = EDGE_LOOP ( 'NONE', ( #16, #17 ) ) ;\n\
#19 = FACE_OUTER_BOUND ( 'NONE', #18, .T. ) ;\n\
#20 = ADVANCED_FACE ( 'NONE', ( #19 ), #5, .T. ) ;\n\
#21 = CARTESIAN_POINT ( 'NONE',  ( 1.4, 0.0, 1.0 ) ) ;\n\
#22 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 0.6, 1.0 ) ) ;\n\
#23 = EDGE_LOOP ( 'NONE', ( #21, #22 ) ) ;\n\
#24 = FACE_OUTER_BOUND ( 'NONE', #23, .T. ) ;\n\
#25 = ADVANCED_FACE ( 'NONE', ( #24 ), #5, .T. ) ;\n\
#26 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 0.0 ) ) ;\n\
#27 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 0.0 ) ) ;\n\
#28 = EDGE_LOOP ( 'NONE', ( #26, #27 ) ) ;\n\
#29 = FACE_OUTER_BOUND ( 'NONE', #28, .T. ) ;\n\
#30 = DIRECTION ( 'NONE',  ( 0.0, 0.0, -1.0 ) ) ;\n\
#31 = AXIS2_PLACEMENT_3D ( 'NONE', #26, #30, #3 ) ;\n\
#32 = PLANE ( 'NONE', #31 ) ;\n\
#33 = ADVANCED_FACE ( 'NONE', ( #29 ), #32, .T. ) ;\n\
#34 = CLOSED_SHELL ( 'NONE', ( #10, #15, #20, #25, #33 ) ) ;\n\
#35 = MANIFOLD_SOLID_BREP ( 'body', #34 ) ;\n\
#36 = SURFACE_STYLE_FILL_AREA ( #37 ) ;\n\
#37 = FILL_AREA_STYLE ( '', ( #38 ) ) ;\n\
#38 = FILL_AREA_STYLE_COLOUR ( '', #39 ) ;\n\
#39 = COLOUR_RGB ( '', 0.25, 0.25, 0.25 ) ;\n\
#40 = SURFACE_STYLE_USAGE ( .BOTH. , #41 ) ;\n\
#41 = SURFACE_SIDE_STYLE ( '', ( #36 ) ) ;\n\
#42 = PRESENTATION_STYLE_ASSIGNMENT (( #40 )) ;\n\
#43 = SURFACE_STYLE_FILL_AREA ( #44 ) ;\n\
#44 = FILL_AREA_STYLE ( '', ( #45 ) ) ;\n\
#45 = FILL_AREA_STYLE_COLOUR ( '', #46 ) ;\n\
#46 = COLOUR_RGB ( '', 1.0, 1.0, 1.0 ) ;\n\
#47 = SURFACE_STYLE_USAGE ( .BOTH. , #48 ) ;\n\
#48 = SURFACE_SIDE_STYLE ( '', ( #43 ) ) ;\n\
#49 = PRESENTATION_STYLE_ASSIGNMENT (( #47 )) ;\n\
#50 = STYLED_ITEM ( 'NONE', ( #42 ), #10 ) ;\n\
#51 = STYLED_ITEM ( 'NONE', ( #49 ), #15 ) ;\n\
#52 = STYLED_ITEM ( 'NONE', ( #49 ), #20 ) ;\n\
#53 = STYLED_ITEM ( 'NONE', ( #49 ), #25 ) ;\n\
#54 = STYLED_ITEM ( 'NONE', ( #42 ), #33 ) ;\n\
#55 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #35 ), #4 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 1,
                removed_solids: 0,
                restyled_faces: 3,
                restyled_groups: 1,
            }
        );
        assert!(sanitized.contains("#51 = STYLED_ITEM ( 'NONE', (#42), #15 )"));
        assert!(sanitized.contains("#52 = STYLED_ITEM ( 'NONE', (#42), #20 )"));
        assert!(sanitized.contains("#53 = STYLED_ITEM ( 'NONE', (#42), #25 )"));
    }

    #[test]
    fn handles_multiple_logo_types_in_one_model() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#2 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#3 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#4 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #2, #3 ) ;\n\
#5 = PLANE ( 'NONE', #4 ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 1.0 ) ) ;\n\
#7 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 1.0 ) ) ;\n\
#8 = EDGE_LOOP ( 'NONE', ( #6, #7 ) ) ;\n\
#9 = FACE_OUTER_BOUND ( 'NONE', #8, .T. ) ;\n\
#10 = ADVANCED_FACE ( 'NONE', ( #9 ), #5, .T. ) ;\n\
#11 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#12 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 0.6, 1.0 ) ) ;\n\
#13 = EDGE_LOOP ( 'NONE', ( #11, #12 ) ) ;\n\
#14 = FACE_OUTER_BOUND ( 'NONE', #13, .T. ) ;\n\
#15 = ADVANCED_FACE ( 'NONE', ( #14 ), #5, .T. ) ;\n\
#16 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.0, 1.0 ) ) ;\n\
#17 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 0.6, 1.0 ) ) ;\n\
#18 = EDGE_LOOP ( 'NONE', ( #16, #17 ) ) ;\n\
#19 = FACE_OUTER_BOUND ( 'NONE', #18, .T. ) ;\n\
#20 = ADVANCED_FACE ( 'NONE', ( #19 ), #5, .T. ) ;\n\
#21 = CARTESIAN_POINT ( 'NONE',  ( 1.4, 0.0, 1.0 ) ) ;\n\
#22 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 0.6, 1.0 ) ) ;\n\
#23 = EDGE_LOOP ( 'NONE', ( #21, #22 ) ) ;\n\
#24 = FACE_OUTER_BOUND ( 'NONE', #23, .T. ) ;\n\
#25 = ADVANCED_FACE ( 'NONE', ( #24 ), #5, .T. ) ;\n\
#26 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 0.0 ) ) ;\n\
#27 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 0.0 ) ) ;\n\
#28 = EDGE_LOOP ( 'NONE', ( #26, #27 ) ) ;\n\
#29 = FACE_OUTER_BOUND ( 'NONE', #28, .T. ) ;\n\
#30 = DIRECTION ( 'NONE',  ( 0.0, 0.0, -1.0 ) ) ;\n\
#31 = AXIS2_PLACEMENT_3D ( 'NONE', #26, #30, #3 ) ;\n\
#32 = PLANE ( 'NONE', #31 ) ;\n\
#33 = ADVANCED_FACE ( 'NONE', ( #29 ), #32, .T. ) ;\n\
#34 = CLOSED_SHELL ( 'NONE', ( #10, #15, #20, #25, #33 ) ) ;\n\
#35 = MANIFOLD_SOLID_BREP ( 'body', #34 ) ;\n\
#36 = SURFACE_STYLE_FILL_AREA ( #37 ) ;\n\
#37 = FILL_AREA_STYLE ( '', ( #38 ) ) ;\n\
#38 = FILL_AREA_STYLE_COLOUR ( '', #39 ) ;\n\
#39 = COLOUR_RGB ( '', 0.25, 0.25, 0.25 ) ;\n\
#40 = SURFACE_STYLE_USAGE ( .BOTH. , #41 ) ;\n\
#41 = SURFACE_SIDE_STYLE ( '', ( #36 ) ) ;\n\
#42 = PRESENTATION_STYLE_ASSIGNMENT (( #40 )) ;\n\
#43 = SURFACE_STYLE_FILL_AREA ( #44 ) ;\n\
#44 = FILL_AREA_STYLE ( '', ( #45 ) ) ;\n\
#45 = FILL_AREA_STYLE_COLOUR ( '', #46 ) ;\n\
#46 = COLOUR_RGB ( '', 1.0, 1.0, 1.0 ) ;\n\
#47 = SURFACE_STYLE_USAGE ( .BOTH. , #48 ) ;\n\
#48 = SURFACE_SIDE_STYLE ( '', ( #43 ) ) ;\n\
#49 = PRESENTATION_STYLE_ASSIGNMENT (( #47 )) ;\n\
#50 = STYLED_ITEM ( 'NONE', ( #42 ), #10 ) ;\n\
#51 = STYLED_ITEM ( 'NONE', ( #49 ), #15 ) ;\n\
#52 = STYLED_ITEM ( 'NONE', ( #49 ), #20 ) ;\n\
#53 = STYLED_ITEM ( 'NONE', ( #49 ), #25 ) ;\n\
#54 = STYLED_ITEM ( 'NONE', ( #42 ), #33 ) ;\n\
#55 = CARTESIAN_POINT ( 'NONE',  ( 3.0, 3.0, 1.0 ) ) ;\n\
#56 = CARTESIAN_POINT ( 'NONE',  ( 4.0, 4.0, 1.001 ) ) ;\n\
#57 = CLOSED_SHELL ( 'NONE', ( #55, #56 ) ) ;\n\
#58 = MANIFOLD_SOLID_BREP ( 'mark', #57 ) ;\n\
#59 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #35, #58 ), #4 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 2,
                removed_solids: 1,
                restyled_faces: 3,
                restyled_groups: 1,
            }
        );
        assert!(sanitized.contains("#51 = STYLED_ITEM ( 'NONE', (#42), #15 )"));
        assert!(sanitized.contains("#52 = STYLED_ITEM ( 'NONE', (#42), #20 )"));
        assert!(sanitized.contains("#53 = STYLED_ITEM ( 'NONE', (#42), #25 )"));
        assert!(!sanitized.contains("( #35, #58 )"));
        assert_eq!(
            occurrence_count(&sanitized, "MANIFOLD_SOLID_BREP ( 'logo cover'"),
            0
        );
    }

    #[test]
    fn restyles_top_logo_on_body_surface_below_taller_feature() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#2 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#3 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#4 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #2, #3 ) ;\n\
#5 = PLANE ( 'NONE', #4 ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 1.0 ) ) ;\n\
#7 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 1.0 ) ) ;\n\
#8 = EDGE_LOOP ( 'NONE', ( #6, #7 ) ) ;\n\
#9 = FACE_OUTER_BOUND ( 'NONE', #8, .T. ) ;\n\
#10 = ADVANCED_FACE ( 'NONE', ( #9 ), #5, .T. ) ;\n\
#11 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.001 ) ) ;\n\
#12 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#13 = AXIS2_PLACEMENT_3D ( 'NONE', #11, #12, #3 ) ;\n\
#14 = PLANE ( 'NONE', #13 ) ;\n\
#15 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.001 ) ) ;\n\
#16 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 0.6, 1.001 ) ) ;\n\
#17 = EDGE_LOOP ( 'NONE', ( #15, #16 ) ) ;\n\
#18 = FACE_OUTER_BOUND ( 'NONE', #17, .T. ) ;\n\
#19 = ADVANCED_FACE ( 'NONE', ( #18 ), #14, .T. ) ;\n\
#20 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.0, 1.001 ) ) ;\n\
#21 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 0.6, 1.001 ) ) ;\n\
#22 = EDGE_LOOP ( 'NONE', ( #20, #21 ) ) ;\n\
#23 = FACE_OUTER_BOUND ( 'NONE', #22, .T. ) ;\n\
#24 = ADVANCED_FACE ( 'NONE', ( #23 ), #14, .T. ) ;\n\
#25 = CARTESIAN_POINT ( 'NONE',  ( 1.4, 0.0, 1.001 ) ) ;\n\
#26 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 0.6, 1.001 ) ) ;\n\
#27 = EDGE_LOOP ( 'NONE', ( #25, #26 ) ) ;\n\
#28 = FACE_OUTER_BOUND ( 'NONE', #27, .T. ) ;\n\
#29 = ADVANCED_FACE ( 'NONE', ( #28 ), #14, .T. ) ;\n\
#30 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.7, 1.001 ) ) ;\n\
#31 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 1.3, 1.001 ) ) ;\n\
#32 = EDGE_LOOP ( 'NONE', ( #30, #31 ) ) ;\n\
#33 = FACE_OUTER_BOUND ( 'NONE', #32, .T. ) ;\n\
#34 = ADVANCED_FACE ( 'NONE', ( #33 ), #14, .T. ) ;\n\
#35 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.7, 1.001 ) ) ;\n\
#36 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 1.3, 1.001 ) ) ;\n\
#37 = EDGE_LOOP ( 'NONE', ( #35, #36 ) ) ;\n\
#38 = FACE_OUTER_BOUND ( 'NONE', #37, .T. ) ;\n\
#39 = ADVANCED_FACE ( 'NONE', ( #38 ), #14, .T. ) ;\n\
#40 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 0.0 ) ) ;\n\
#41 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 0.0 ) ) ;\n\
#42 = EDGE_LOOP ( 'NONE', ( #40, #41 ) ) ;\n\
#43 = FACE_OUTER_BOUND ( 'NONE', #42, .T. ) ;\n\
#44 = DIRECTION ( 'NONE',  ( 0.0, 0.0, -1.0 ) ) ;\n\
#45 = AXIS2_PLACEMENT_3D ( 'NONE', #40, #44, #3 ) ;\n\
#46 = PLANE ( 'NONE', #45 ) ;\n\
#47 = ADVANCED_FACE ( 'NONE', ( #43 ), #46, .T. ) ;\n\
#48 = CARTESIAN_POINT ( 'NONE',  ( 4.0, 4.0, 1.6 ) ) ;\n\
#49 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#50 = AXIS2_PLACEMENT_3D ( 'NONE', #48, #49, #3 ) ;\n\
#51 = PLANE ( 'NONE', #50 ) ;\n\
#52 = CARTESIAN_POINT ( 'NONE',  ( 4.0, 4.0, 1.6 ) ) ;\n\
#53 = CARTESIAN_POINT ( 'NONE',  ( 4.4, 4.4, 1.6 ) ) ;\n\
#54 = EDGE_LOOP ( 'NONE', ( #52, #53 ) ) ;\n\
#55 = FACE_OUTER_BOUND ( 'NONE', #54, .T. ) ;\n\
#56 = ADVANCED_FACE ( 'NONE', ( #55 ), #51, .T. ) ;\n\
#57 = CLOSED_SHELL ( 'NONE', ( #10, #19, #24, #29, #34, #39, #47, #56 ) ) ;\n\
#58 = MANIFOLD_SOLID_BREP ( 'body', #57 ) ;\n\
#59 = SURFACE_STYLE_FILL_AREA ( #60 ) ;\n\
#60 = FILL_AREA_STYLE ( '', ( #61 ) ) ;\n\
#61 = FILL_AREA_STYLE_COLOUR ( '', #62 ) ;\n\
#62 = COLOUR_RGB ( '', 0.25, 0.25, 0.25 ) ;\n\
#63 = SURFACE_STYLE_USAGE ( .BOTH. , #64 ) ;\n\
#64 = SURFACE_SIDE_STYLE ( '', ( #59 ) ) ;\n\
#65 = PRESENTATION_STYLE_ASSIGNMENT (( #63 )) ;\n\
#66 = SURFACE_STYLE_FILL_AREA ( #67 ) ;\n\
#67 = FILL_AREA_STYLE ( '', ( #68 ) ) ;\n\
#68 = FILL_AREA_STYLE_COLOUR ( '', #69 ) ;\n\
#69 = COLOUR_RGB ( '', 1.0, 1.0, 1.0 ) ;\n\
#70 = SURFACE_STYLE_USAGE ( .BOTH. , #71 ) ;\n\
#71 = SURFACE_SIDE_STYLE ( '', ( #66 ) ) ;\n\
#72 = PRESENTATION_STYLE_ASSIGNMENT (( #70 )) ;\n\
#73 = STYLED_ITEM ( 'NONE', ( #65 ), #10 ) ;\n\
#74 = STYLED_ITEM ( 'NONE', ( #72 ), #19 ) ;\n\
#75 = STYLED_ITEM ( 'NONE', ( #72 ), #24 ) ;\n\
#76 = STYLED_ITEM ( 'NONE', ( #72 ), #29 ) ;\n\
#77 = STYLED_ITEM ( 'NONE', ( #72 ), #34 ) ;\n\
#78 = STYLED_ITEM ( 'NONE', ( #72 ), #39 ) ;\n\
#79 = STYLED_ITEM ( 'NONE', ( #65 ), #47 ) ;\n\
#80 = STYLED_ITEM ( 'NONE', ( #65 ), #56 ) ;\n\
#81 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #58 ), #4 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 1,
                removed_solids: 0,
                restyled_faces: 5,
                restyled_groups: 1,
            }
        );
        assert!(sanitized.contains("#74 = STYLED_ITEM ( 'NONE', (#65), #19 )"));
        assert!(sanitized.contains("#75 = STYLED_ITEM ( 'NONE', (#65), #24 )"));
        assert!(sanitized.contains("#76 = STYLED_ITEM ( 'NONE', (#65), #29 )"));
        assert!(sanitized.contains("#77 = STYLED_ITEM ( 'NONE', (#65), #34 )"));
        assert!(sanitized.contains("#78 = STYLED_ITEM ( 'NONE', (#65), #39 )"));
    }

    #[test]
    fn restyles_shallow_logo_side_faces_with_top_cluster() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#2 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#3 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#4 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #2, #3 ) ;\n\
#5 = PLANE ( 'NONE', #4 ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 1.0 ) ) ;\n\
#7 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 1.0 ) ) ;\n\
#8 = EDGE_LOOP ( 'NONE', ( #6, #7 ) ) ;\n\
#9 = FACE_OUTER_BOUND ( 'NONE', #8, .T. ) ;\n\
#10 = ADVANCED_FACE ( 'NONE', ( #9 ), #5, .T. ) ;\n\
#11 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#12 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 0.6, 1.0 ) ) ;\n\
#13 = EDGE_LOOP ( 'NONE', ( #11, #12 ) ) ;\n\
#14 = FACE_OUTER_BOUND ( 'NONE', #13, .T. ) ;\n\
#15 = ADVANCED_FACE ( 'NONE', ( #14 ), #5, .T. ) ;\n\
#16 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.0, 1.0 ) ) ;\n\
#17 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 0.6, 1.0 ) ) ;\n\
#18 = EDGE_LOOP ( 'NONE', ( #16, #17 ) ) ;\n\
#19 = FACE_OUTER_BOUND ( 'NONE', #18, .T. ) ;\n\
#20 = ADVANCED_FACE ( 'NONE', ( #19 ), #5, .T. ) ;\n\
#21 = CARTESIAN_POINT ( 'NONE',  ( 1.4, 0.0, 1.0 ) ) ;\n\
#22 = CARTESIAN_POINT ( 'NONE',  ( 2.0, 0.6, 1.0 ) ) ;\n\
#23 = EDGE_LOOP ( 'NONE', ( #21, #22 ) ) ;\n\
#24 = FACE_OUTER_BOUND ( 'NONE', #23, .T. ) ;\n\
#25 = ADVANCED_FACE ( 'NONE', ( #24 ), #5, .T. ) ;\n\
#26 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.7, 1.0 ) ) ;\n\
#27 = CARTESIAN_POINT ( 'NONE',  ( 0.6, 1.3, 1.0 ) ) ;\n\
#28 = EDGE_LOOP ( 'NONE', ( #26, #27 ) ) ;\n\
#29 = FACE_OUTER_BOUND ( 'NONE', #28, .T. ) ;\n\
#30 = ADVANCED_FACE ( 'NONE', ( #29 ), #5, .T. ) ;\n\
#31 = CARTESIAN_POINT ( 'NONE',  ( 0.7, 0.7, 1.0 ) ) ;\n\
#32 = CARTESIAN_POINT ( 'NONE',  ( 1.3, 1.3, 1.0 ) ) ;\n\
#33 = EDGE_LOOP ( 'NONE', ( #31, #32 ) ) ;\n\
#34 = FACE_OUTER_BOUND ( 'NONE', #33, .T. ) ;\n\
#35 = ADVANCED_FACE ( 'NONE', ( #34 ), #5, .T. ) ;\n\
#36 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.9 ) ) ;\n\
#37 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#38 = AXIS2_PLACEMENT_3D ( 'NONE', #36, #37, #2 ) ;\n\
#39 = PLANE ( 'NONE', #38 ) ;\n\
#40 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.9 ) ) ;\n\
#41 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.6, 1.0 ) ) ;\n\
#42 = EDGE_LOOP ( 'NONE', ( #40, #41 ) ) ;\n\
#43 = FACE_OUTER_BOUND ( 'NONE', #42, .T. ) ;\n\
#44 = ADVANCED_FACE ( 'NONE', ( #43 ), #39, .T. ) ;\n\
#45 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 0.0 ) ) ;\n\
#46 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 0.0 ) ) ;\n\
#47 = EDGE_LOOP ( 'NONE', ( #45, #46 ) ) ;\n\
#48 = FACE_OUTER_BOUND ( 'NONE', #47, .T. ) ;\n\
#49 = DIRECTION ( 'NONE',  ( 0.0, 0.0, -1.0 ) ) ;\n\
#50 = AXIS2_PLACEMENT_3D ( 'NONE', #45, #49, #3 ) ;\n\
#51 = PLANE ( 'NONE', #50 ) ;\n\
#52 = ADVANCED_FACE ( 'NONE', ( #48 ), #51, .T. ) ;\n\
#53 = CLOSED_SHELL ( 'NONE', ( #10, #15, #20, #25, #30, #35, #44, #52 ) ) ;\n\
#54 = MANIFOLD_SOLID_BREP ( 'body', #53 ) ;\n\
#55 = SURFACE_STYLE_FILL_AREA ( #56 ) ;\n\
#56 = FILL_AREA_STYLE ( '', ( #57 ) ) ;\n\
#57 = FILL_AREA_STYLE_COLOUR ( '', #58 ) ;\n\
#58 = COLOUR_RGB ( '', 0.25, 0.25, 0.25 ) ;\n\
#59 = SURFACE_STYLE_USAGE ( .BOTH. , #60 ) ;\n\
#60 = SURFACE_SIDE_STYLE ( '', ( #55 ) ) ;\n\
#61 = PRESENTATION_STYLE_ASSIGNMENT (( #59 )) ;\n\
#62 = SURFACE_STYLE_FILL_AREA ( #63 ) ;\n\
#63 = FILL_AREA_STYLE ( '', ( #64 ) ) ;\n\
#64 = FILL_AREA_STYLE_COLOUR ( '', #65 ) ;\n\
#65 = COLOUR_RGB ( '', 1.0, 1.0, 1.0 ) ;\n\
#66 = SURFACE_STYLE_USAGE ( .BOTH. , #67 ) ;\n\
#67 = SURFACE_SIDE_STYLE ( '', ( #62 ) ) ;\n\
#68 = PRESENTATION_STYLE_ASSIGNMENT (( #66 )) ;\n\
#69 = STYLED_ITEM ( 'NONE', ( #61 ), #10 ) ;\n\
#70 = STYLED_ITEM ( 'NONE', ( #68 ), #15 ) ;\n\
#71 = STYLED_ITEM ( 'NONE', ( #68 ), #20 ) ;\n\
#72 = STYLED_ITEM ( 'NONE', ( #68 ), #25 ) ;\n\
#73 = STYLED_ITEM ( 'NONE', ( #68 ), #30 ) ;\n\
#74 = STYLED_ITEM ( 'NONE', ( #68 ), #35 ) ;\n\
#75 = STYLED_ITEM ( 'NONE', ( #68 ), #44 ) ;\n\
#76 = STYLED_ITEM ( 'NONE', ( #61 ), #52 ) ;\n\
#77 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #54 ), #4 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();
        let sanitized = String::from_utf8(sanitized).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 1,
                removed_solids: 0,
                restyled_faces: 6,
                restyled_groups: 1,
            }
        );
        assert!(sanitized.contains("#75 = STYLED_ITEM ( 'NONE', (#61), #44 )"));
    }

    #[test]
    fn chooses_geometry_logo_background_style_behind_cluster() {
        let face_styles = vec![
            AnyFaceStyleInfo {
                styled_item_id: 1,
                style_id: 20,
                color: [0.1, 0.1, 0.1],
                bbox: BoundingBox {
                    min_x: -0.5,
                    max_x: 0.5,
                    min_y: -0.1,
                    max_y: 0.1,
                    min_z: 0.8,
                    max_z: 1.0,
                },
            },
            AnyFaceStyleInfo {
                styled_item_id: 2,
                style_id: 10,
                color: [0.8, 0.76, 0.7],
                bbox: BoundingBox {
                    min_x: -2.0,
                    max_x: 2.0,
                    min_y: -1.0,
                    max_y: 1.0,
                    min_z: 0.55,
                    max_z: 0.7,
                },
            },
        ];
        let cluster = GeometryLogoCluster {
            color: face_styles[0].color,
            bbox: face_styles[0].bbox,
            member_indices: vec![0],
        };

        assert_eq!(
            choose_geometry_logo_replacement(&cluster, 2, OuterSide::Max, &face_styles),
            Some(10)
        );
    }

    #[test]
    fn chooses_dark_solid_style_for_light_geometry_logo_without_background() {
        let cluster = GeometryLogoCluster {
            color: [1.0, 1.0, 1.0],
            bbox: BoundingBox {
                min_x: -1.0,
                max_x: 1.0,
                min_y: 3.0,
                max_y: 3.0,
                min_z: 0.0,
                max_z: 1.5,
            },
            member_indices: (0..GEOMETRY_LOGO_SOLID_FALLBACK_MIN_FACES).collect(),
        };
        let solid_styles = vec![
            SolidStyleInfo {
                style_id: 10,
                color: [0.32, 0.32, 0.32],
            },
            SolidStyleInfo {
                style_id: 20,
                color: [0.9, 0.9, 0.9],
            },
        ];

        assert_eq!(
            choose_solid_style_fallback(&cluster, &solid_styles),
            Some(10)
        );
    }

    #[test]
    fn chooses_dark_solid_style_for_light_face_logo_without_background() {
        let candidate = FaceStyleGroup {
            representative_style_id: 20,
            color: [1.0, 1.0, 1.0],
            axis: 2,
            side: OuterSide::Max,
            bbox: BoundingBox {
                min_x: -1.0,
                max_x: 1.0,
                min_y: 3.0,
                max_y: 3.4,
                min_z: 1.0,
                max_z: 1.0,
            },
            min_coordinate: 1.0,
            max_coordinate: 1.0,
            face_count: FACE_LOGO_SOLID_FALLBACK_MIN_FACES,
            member_indices: (0..FACE_LOGO_SOLID_FALLBACK_MIN_FACES).collect(),
        };
        let solid_styles = vec![
            SolidStyleInfo {
                style_id: 10,
                color: [0.32, 0.32, 0.32],
            },
            SolidStyleInfo {
                style_id: 30,
                color: [0.9, 0.9, 0.9],
            },
        ];

        assert_eq!(
            choose_face_group_solid_style_fallback(&candidate, &solid_styles),
            Some(10)
        );
    }

    #[test]
    fn rejects_edge_pads_but_allows_central_bottom_logo_candidates() {
        let solid_bounds = BoundingBox {
            min_x: -3.0,
            max_x: 3.0,
            min_y: -3.0,
            max_y: 3.0,
            min_z: 0.0,
            max_z: 1.0,
        };
        let bottom_candidate = BoundingBox {
            min_x: -2.3,
            max_x: 2.3,
            min_y: 2.7,
            max_y: 3.0,
            min_z: 0.0,
            max_z: 0.2,
        };
        let central_bottom_candidate = BoundingBox {
            min_x: -1.0,
            max_x: 1.0,
            min_y: -0.4,
            max_y: 0.4,
            min_z: 0.0,
            max_z: 0.2,
        };
        let top_candidate = BoundingBox {
            min_x: -1.0,
            max_x: 1.0,
            min_y: -1.0,
            max_y: 1.0,
            min_z: 0.9,
            max_z: 1.0,
        };
        let side_logo_candidate = BoundingBox {
            min_x: -1.0,
            max_x: 1.0,
            min_y: 3.0,
            max_y: 3.0,
            min_z: 0.0,
            max_z: 0.8,
        };

        assert!(!is_supported_logo_side(
            2,
            OuterSide::Min,
            bottom_candidate,
            solid_bounds
        ));
        assert!(is_supported_logo_side(
            2,
            OuterSide::Min,
            central_bottom_candidate,
            solid_bounds
        ));
        assert!(!is_supported_logo_side(
            1,
            OuterSide::Max,
            bottom_candidate,
            solid_bounds
        ));
        assert!(is_supported_logo_side(
            2,
            OuterSide::Max,
            top_candidate,
            solid_bounds
        ));
        assert!(is_supported_logo_side(
            1,
            OuterSide::Max,
            side_logo_candidate,
            solid_bounds
        ));
    }

    #[test]
    fn deduplicates_identical_cover_patches() {
        let mut plan = StyleRewritePlan::default();
        let bbox = BoundingBox {
            min_x: -0.04,
            max_x: 2.74,
            min_y: -0.04,
            max_y: 2.74,
            min_z: 0.923,
            max_z: 0.938,
        };

        push_cover_patch(&mut plan, CoverPatch { bbox, style_id: 10 });
        push_cover_patch(&mut plan, CoverPatch { bbox, style_id: 20 });

        assert_eq!(plan.cover_patches.len(), 1);
    }

    #[test]
    fn deduplicates_stacked_cover_patches_with_same_projection() {
        let mut plan = StyleRewritePlan::default();
        let lower = BoundingBox {
            min_x: -2.5654,
            max_x: 2.5948,
            min_y: -0.3003,
            max_y: 0.3808,
            min_z: 1.442,
            max_z: 1.457,
        };
        let upper = BoundingBox {
            min_x: -2.5654,
            max_x: 2.5948,
            min_y: -0.3003,
            max_y: 0.3808,
            min_z: 1.452,
            max_z: 1.467,
        };

        push_cover_patch(
            &mut plan,
            CoverPatch {
                bbox: lower,
                style_id: 10,
            },
        );
        push_cover_patch(
            &mut plan,
            CoverPatch {
                bbox: upper,
                style_id: 20,
            },
        );

        assert_eq!(plan.cover_patches.len(), 1);
    }

    #[test]
    fn cover_patch_faces_use_outward_normals() {
        let points = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let expected_normals = [
            [0.0, 0.0, -1.0],
            [0.0, 0.0, 1.0],
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [-1.0, 0.0, 0.0],
        ];

        for (face_points, expected) in cover_patch_face_point_indices()
            .into_iter()
            .zip(expected_normals)
        {
            let origin = points[face_points[0]];
            let first_edge = normalize_vector([
                points[face_points[1]][0] - origin[0],
                points[face_points[1]][1] - origin[1],
                points[face_points[1]][2] - origin[2],
            ])
            .unwrap();
            let second_edge = [
                points[face_points[2]][0] - points[face_points[1]][0],
                points[face_points[2]][1] - points[face_points[1]][1],
                points[face_points[2]][2] - points[face_points[1]][2],
            ];
            let normal = normalize_vector(cross_product(first_edge, second_edge)).unwrap();
            let dot = normal[0] * expected[0] + normal[1] * expected[1] + normal[2] * expected[2];

            assert!(dot > 0.99, "normal {normal:?} did not match {expected:?}");
        }
    }

    #[test]
    fn splits_mid_gray_clusters_before_area_filtering() {
        let replacement = FaceStyleInfo {
            styled_item_id: 1,
            style_id: 10,
            color: [0.25, 0.25, 0.25],
            bbox: BoundingBox {
                min_x: -5.0,
                max_x: 5.0,
                min_y: -5.0,
                max_y: 5.0,
                min_z: 1.0,
                max_z: 1.0,
            },
            face: PlanarFaceInfo {
                axis: 2,
                coordinate: 1.0,
            },
            outer_side: Some(OuterSide::Max),
        };
        let cluster_boxes = [
            (0.0, 0.0, 0.6, 0.6),
            (0.72, 0.0, 1.32, 0.6),
            (1.44, 0.0, 2.04, 0.6),
            (0.0, 0.72, 0.6, 1.32),
            (0.72, 0.72, 1.32, 1.32),
        ];
        let mut face_styles = vec![replacement];
        for (index, (min_x, min_y, max_x, max_y)) in cluster_boxes.into_iter().enumerate() {
            face_styles.push(FaceStyleInfo {
                styled_item_id: 100 + index,
                style_id: 20 + index,
                color: [0.6, 0.58, 0.57],
                bbox: BoundingBox {
                    min_x,
                    max_x,
                    min_y,
                    max_y,
                    min_z: 0.99,
                    max_z: 0.99,
                },
                face: PlanarFaceInfo {
                    axis: 2,
                    coordinate: 0.99,
                },
                outer_side: Some(OuterSide::Max),
            });
        }
        face_styles.push(FaceStyleInfo {
            styled_item_id: 200,
            style_id: 30,
            color: [0.6, 0.58, 0.57],
            bbox: BoundingBox {
                min_x: -4.5,
                max_x: -3.9,
                min_y: -4.5,
                max_y: -3.9,
                min_z: 0.99,
                max_z: 0.99,
            },
            face: PlanarFaceInfo {
                axis: 2,
                coordinate: 0.99,
            },
            outer_side: Some(OuterSide::Max),
        });

        let groups = build_outer_style_groups(&face_styles);
        let mut mid_gray_groups = groups
            .iter()
            .filter(|group| colors_match(group.color, [0.6, 0.58, 0.57]))
            .collect::<Vec<_>>();
        mid_gray_groups.sort_by_key(|group| std::cmp::Reverse(group.face_count));

        assert_eq!(mid_gray_groups.len(), 2);
        assert_eq!(mid_gray_groups[0].face_count, 5);
        assert_eq!(mid_gray_groups[1].face_count, 1);
        assert!(projected_area(mid_gray_groups[0].bbox, 2) < 3.0);
        assert!(
            choose_replacement_group(mid_gray_groups[0], &groups)
                .is_some_and(|group| group.representative_style_id == 10)
        );
    }

    #[test]
    fn keeps_large_outer_face_regions() {
        let step = b"\
#1 = CARTESIAN_POINT ( 'NONE',  ( 0.0, 0.0, 0.0 ) ) ;\n\
#2 = DIRECTION ( 'NONE',  ( 0.0, 0.0, 1.0 ) ) ;\n\
#3 = DIRECTION ( 'NONE',  ( 1.0, 0.0, 0.0 ) ) ;\n\
#4 = AXIS2_PLACEMENT_3D ( 'NONE', #1, #2, #3 ) ;\n\
#5 = PLANE ( 'NONE', #4 ) ;\n\
#6 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 1.0 ) ) ;\n\
#7 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 1.0 ) ) ;\n\
#8 = EDGE_LOOP ( 'NONE', ( #6, #7 ) ) ;\n\
#9 = FACE_OUTER_BOUND ( 'NONE', #8, .T. ) ;\n\
#10 = ADVANCED_FACE ( 'NONE', ( #9 ), #5, .T. ) ;\n\
#11 = CARTESIAN_POINT ( 'NONE',  ( -4.0, -4.0, 1.0 ) ) ;\n\
#12 = CARTESIAN_POINT ( 'NONE',  ( 4.0, 4.0, 1.0 ) ) ;\n\
#13 = EDGE_LOOP ( 'NONE', ( #11, #12 ) ) ;\n\
#14 = FACE_OUTER_BOUND ( 'NONE', #13, .T. ) ;\n\
#15 = ADVANCED_FACE ( 'NONE', ( #14 ), #5, .T. ) ;\n\
#16 = CARTESIAN_POINT ( 'NONE',  ( -5.0, -5.0, 0.0 ) ) ;\n\
#17 = CARTESIAN_POINT ( 'NONE',  ( 5.0, 5.0, 0.0 ) ) ;\n\
#18 = EDGE_LOOP ( 'NONE', ( #16, #17 ) ) ;\n\
#19 = FACE_OUTER_BOUND ( 'NONE', #18, .T. ) ;\n\
#20 = DIRECTION ( 'NONE',  ( 0.0, 0.0, -1.0 ) ) ;\n\
#21 = AXIS2_PLACEMENT_3D ( 'NONE', #16, #20, #3 ) ;\n\
#22 = PLANE ( 'NONE', #21 ) ;\n\
#23 = ADVANCED_FACE ( 'NONE', ( #19 ), #22, .T. ) ;\n\
#24 = CLOSED_SHELL ( 'NONE', ( #10, #15, #23 ) ) ;\n\
#25 = MANIFOLD_SOLID_BREP ( 'body', #24 ) ;\n\
#26 = SURFACE_STYLE_FILL_AREA ( #27 ) ;\n\
#27 = FILL_AREA_STYLE ( '', ( #28 ) ) ;\n\
#28 = FILL_AREA_STYLE_COLOUR ( '', #29 ) ;\n\
#29 = COLOUR_RGB ( '', 0.25, 0.25, 0.25 ) ;\n\
#30 = SURFACE_STYLE_USAGE ( .BOTH. , #31 ) ;\n\
#31 = SURFACE_SIDE_STYLE ( '', ( #26 ) ) ;\n\
#32 = PRESENTATION_STYLE_ASSIGNMENT (( #30 )) ;\n\
#33 = SURFACE_STYLE_FILL_AREA ( #34 ) ;\n\
#34 = FILL_AREA_STYLE ( '', ( #35 ) ) ;\n\
#35 = FILL_AREA_STYLE_COLOUR ( '', #36 ) ;\n\
#36 = COLOUR_RGB ( '', 1.0, 1.0, 1.0 ) ;\n\
#37 = SURFACE_STYLE_USAGE ( .BOTH. , #38 ) ;\n\
#38 = SURFACE_SIDE_STYLE ( '', ( #33 ) ) ;\n\
#39 = PRESENTATION_STYLE_ASSIGNMENT (( #37 )) ;\n\
#40 = STYLED_ITEM ( 'NONE', ( #32 ), #10 ) ;\n\
#41 = STYLED_ITEM ( 'NONE', ( #39 ), #15 ) ;\n\
#42 = STYLED_ITEM ( 'NONE', ( #32 ), #23 ) ;\n\
#43 = ADVANCED_BREP_SHAPE_REPRESENTATION ( 'Demo', ( #25 ), #4 ) ;\n";

        let (sanitized, stats) = sanitize_step_model(step).unwrap();

        assert_eq!(
            stats,
            StepSanitizeStats {
                total_solids: 1,
                removed_solids: 0,
                restyled_faces: 0,
                restyled_groups: 0,
            }
        );
        assert_eq!(sanitized, step);
    }
}
