//! The periodic tiling canvas.
//!
//! The unit cell is drawn strongly and its true periodic neighbours are drawn
//! translucent behind it, so the user is looking at the tiling rather than at
//! one polygon and a promise. Every polygon on screen comes from the same
//! [`CanvasTransform`] the pointer is hit-tested against.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui};

use crate::document::tiling::ConstructionTarget;
use crate::gui::canvas::CanvasTransform;
use crate::gui::theme;
use crate::sim::tiling::{
    BasisId, EdgeRef, PeriodicTilingDraft, PrototypeId, PrototypeShape, SeamConstraint, Vec2,
    neighbor_offsets, polygon,
    solver::{DragTarget, solve_edge_constraints},
};

/// Screen radius the vertex handles are drawn at.
const HANDLE_RADIUS: f32 = 5.0;
/// Screen radius within which a press grabs a handle. Deliberately larger than
/// the drawn dot: aiming at a five-pixel target is not a skill the editor
/// should demand.
const GRAB_RADIUS: f32 = 9.0;
/// Scale used until a tiling exists to fit.
const BLANK_SCALE: f64 = 220.0;

/// What the pointer does on the canvas.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TilingTool {
    #[default]
    Select,
    Draw,
}

/// Transient canvas state the GUI owns between frames. The committed tiling
/// lives in the document; only work in progress lives here.
#[derive(Default)]
pub struct TilingCanvasState {
    pub transform: Option<CanvasTransform>,
    pub tool: TilingTool,
    pub target: ConstructionTarget,
    pub selected_prototype: Option<PrototypeId>,
    pub selected_basis: Option<BasisId>,
    pub selected_vertex: Option<usize>,
    /// Seam constraints the user has accepted; drags respect them.
    pub seams: Vec<SeamConstraint>,
    /// Why the last vertex was refused, shown next to the pointer.
    pub rejection: Option<String>,
    /// Held seams the last drag could not keep closed. A drag is never blocked
    /// on their account; it leaves them here to be seen and repaired.
    pub broken: Vec<SeamConstraint>,
    construction: Vec<Vec2>,
    /// Points removed by "Undo point", newest last.
    redo: Vec<Vec2>,
    hovered: Option<Vec2>,
    dragging: Option<(PrototypeId, usize)>,
    neighbor_copies: usize,
}

impl TilingCanvasState {
    /// Start drawing a polygon that will become a new basis.
    pub fn begin_new_basis(&mut self) {
        self.tool = TilingTool::Draw;
        self.target = ConstructionTarget::NewBasis;
        self.construction.clear();
        self.redo.clear();
        self.rejection = None;
    }

    /// Start redrawing an existing polygon in place.
    pub fn begin_reshape(&mut self, prototype: PrototypeId) {
        self.tool = TilingTool::Draw;
        self.target = ConstructionTarget::ReplacePrototype(prototype);
        self.construction.clear();
        self.redo.clear();
        self.rejection = None;
    }

    pub fn cancel(&mut self) {
        self.tool = TilingTool::Select;
        self.construction.clear();
        self.redo.clear();
        self.rejection = None;
    }

    pub fn construction(&self) -> &[Vec2] {
        &self.construction
    }

    pub fn drawing(&self) -> bool {
        self.tool == TilingTool::Draw
    }

    /// Copies of the unit cell drawn around the centre one in the last frame.
    pub fn neighbor_copies(&self) -> usize {
        self.neighbor_copies
    }

    pub fn hovered(&self) -> Option<Vec2> {
        self.hovered
    }

    /// Append a vertex, or record why it was refused. An invalid point never
    /// reaches the construction path.
    pub fn push_vertex(&mut self, point: Vec2) -> bool {
        match polygon::validate_open_path_append(&self.construction, point) {
            Ok(()) => {
                self.construction.push(point);
                self.redo.clear();
                self.rejection = None;
                true
            }
            Err(reason) => {
                self.rejection = Some(reason);
                false
            }
        }
    }

    pub fn undo_point(&mut self) -> bool {
        match self.construction.pop() {
            Some(point) => {
                self.redo.push(point);
                self.rejection = None;
                true
            }
            None => false,
        }
    }

    pub fn redo_point(&mut self) -> bool {
        match self.redo.pop() {
            Some(point) => {
                self.construction.push(point);
                self.rejection = None;
                true
            }
            None => false,
        }
    }

    pub fn can_undo_point(&self) -> bool {
        !self.construction.is_empty()
    }

    pub fn can_redo_point(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Drop the fitted transform so the next frame refits the tiling.
    pub fn request_fit(&mut self) {
        self.transform = None;
    }
}

/// What the pointer did to the tiling this frame.
#[derive(Clone, Debug, Default)]
pub struct TilingCanvasResponse {
    /// A tiling the pointer changed, for the document to store.
    pub commit: Option<PeriodicTilingDraft>,
    /// A basis the pointer selected, including through a periodic copy.
    pub selected: Option<BasisId>,
    /// World point under the pointer, for the readout.
    pub hovered: Option<Vec2>,
}

pub fn render_tiling_canvas(
    ui: &mut Ui,
    size: egui::Vec2,
    draft: Option<&PeriodicTilingDraft>,
    state: &mut TilingCanvasState,
) -> TilingCanvasResponse {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::DOMAIN_EXTERIOR);

    // One frame's transform serves both drawing and hit testing.
    let transform = match &mut state.transform {
        Some(transform) => {
            transform.viewport = rect;
            *transform
        }
        None => {
            let fitted = fit_transform(rect, draft);
            state.transform = Some(fitted);
            fitted
        }
    };

    let mut result = TilingCanvasResponse::default();

    if let Some(draft) = draft {
        state.neighbor_copies = draw_neighbors(&painter, &transform, draft);
        draw_unit_cell(&painter, &transform, draft, state);
        draw_lattice_vectors(&painter, &transform, draft);
        draw_seam_hints(&painter, &transform, draft);
    } else {
        state.neighbor_copies = 0;
        painter.text(
            rect.center_top() + egui::vec2(0.0, 12.0),
            egui::Align2::CENTER_TOP,
            "no tiling yet — pick a preset or draw one",
            egui::FontId::proportional(14.0),
            theme::state_color(theme::State::Draft),
        );
    }

    // Wheel zooms about the pointer and middle drag pans, exactly as on the
    // simulation canvas.
    if let Some(pointer) = response.hover_pos() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0
            && let Some(transform) = &mut state.transform
        {
            transform.zoom_at(pointer, (scroll as f64 / 120.0).exp2());
        }
        let world = transform.screen_to_world(pointer);
        state.hovered = Some(Vec2::new(world[0], world[1]));
        result.hovered = state.hovered;
    } else {
        state.hovered = None;
    }
    if response.dragged_by(egui::PointerButton::Middle)
        && let Some(transform) = &mut state.transform
    {
        transform.pan_screen(response.drag_delta());
    }

    match state.tool {
        TilingTool::Draw => {
            draw_construction(&painter, &transform, state);
            if response.clicked_by(egui::PointerButton::Primary)
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let world = transform.screen_to_world(pointer);
                state.push_vertex(Vec2::new(world[0], world[1]));
            }
        }
        TilingTool::Select => {
            if let Some(draft) = draft {
                // Grabbing is judged from where the button went down, not from
                // where the pointer is by the time egui reports a drag: a quick
                // drag has already left the handle by then.
                let press_origin = ui.input(|input| input.pointer.press_origin());
                select_and_drag(
                    &response,
                    &transform,
                    draft,
                    press_origin,
                    state,
                    &mut result,
                );
            }
        }
    }

    result
}

/// Centre the view on the unit cell, or on the origin when there is nothing to
/// fit yet.
fn fit_transform(rect: Rect, draft: Option<&PeriodicTilingDraft>) -> CanvasTransform {
    let Some(draft) = draft else {
        return CanvasTransform::new(rect, [0.0, 0.0], BLANK_SCALE);
    };
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for offset in [[0, 0]].into_iter().chain(visible_offsets(draft)) {
        let translation = lattice_translation(draft, offset);
        for vertices in cell_polygons(draft) {
            for vertex in vertices.1 {
                let point = vertex + translation;
                min[0] = min[0].min(point.x);
                min[1] = min[1].min(point.y);
                max[0] = max[0].max(point.x);
                max[1] = max[1].max(point.y);
            }
        }
    }
    if !min[0].is_finite() || !max[0].is_finite() || max[0] <= min[0] || max[1] <= min[1] {
        return CanvasTransform::new(rect, [0.0, 0.0], BLANK_SCALE);
    }
    let span = [max[0] - min[0], max[1] - min[1]];
    let scale = (((rect.width() as f64) - 32.0).max(1.0) / span[0])
        .min(((rect.height() as f64) - 32.0).max(1.0) / span[1])
        .max(1e-6);
    CanvasTransform::new(
        rect,
        [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0],
        scale,
    )
}

/// Lattice offsets the canvas draws around the unit cell.
///
/// This is a question about visibility, not adjacency. A square's proven
/// adjacency ring holds only its four edge neighbours, and drawing just those
/// leaves the diagonals blank so the pattern reads as a plus sign rather than a
/// tiling. The canvas therefore draws the eight surrounding cells, plus any
/// proven neighbour outside that block: an oblique lattice can be adjacent to a
/// copy two steps away. Every offset drawn is a true lattice translate.
fn visible_offsets(draft: &PeriodicTilingDraft) -> Vec<[i32; 2]> {
    if draft.translation_a.cross(draft.translation_b).abs() <= f64::MIN_POSITIVE {
        return Vec::new();
    }
    let mut offsets: Vec<[i32; 2]> = (-1..=1)
        .flat_map(|a| (-1..=1).map(move |b| [a, b]))
        .filter(|offset| *offset != [0, 0])
        .collect();
    for proven in neighbor_offsets(draft) {
        if !offsets.contains(&proven) {
            offsets.push(proven);
        }
    }
    offsets
}

fn lattice_translation(draft: &PeriodicTilingDraft, offset: [i32; 2]) -> Vec2 {
    draft.translation_a * f64::from(offset[0]) + draft.translation_b * f64::from(offset[1])
}

/// Placed polygons of the unit cell, paired with the basis they belong to.
fn cell_polygons(draft: &PeriodicTilingDraft) -> Vec<(BasisId, Vec<Vec2>)> {
    draft
        .instances
        .iter()
        .filter_map(|instance| {
            let prototype = draft
                .prototypes
                .iter()
                .find(|entry| entry.id == instance.prototype)?;
            let base = polygon::prototype_vertices(&prototype.shape).ok()?;
            Some((
                instance.id,
                polygon::transform_vertices(&base, instance.transform),
            ))
        })
        .collect()
}

/// Fill and outline a polygon that may be concave.
///
/// egui fills a path as a fan from its first point, which is only correct for a
/// convex shape. A user-drawn basis is often concave, so the interior is
/// triangulated here. The triangles go into one mesh rather than one shape
/// each: separate shapes are each anti-aliased against the background, which
/// leaves visible hairlines along every interior edge of the triangulation.
fn fill_polygon(painter: &egui::Painter, points: &[Pos2], fill: Color32, stroke: Stroke) {
    if points.len() < 3 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for point in points {
        mesh.colored_vertex(*point, fill);
    }
    for [a, b, c] in ear_clip(points) {
        mesh.add_triangle(a, b, c);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::closed_line(points.to_vec(), stroke));
}

/// Ear clipping, returning `points.len() - 2` index triples for any simple
/// polygon. A polygon that turns out not to be simple falls back to a fan, so a
/// degenerate draft still draws something instead of vanishing.
fn ear_clip(points: &[Pos2]) -> Vec<[u32; 3]> {
    let fan = || {
        (1..points.len() - 1)
            .map(|index| [0, index as u32, index as u32 + 1])
            .collect::<Vec<_>>()
    };
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    if signed_area_screen(points) < 0.0 {
        remaining.reverse();
    }
    let mut triangles = Vec::with_capacity(points.len().saturating_sub(2));
    let mut guard = 0;
    while remaining.len() > 3 {
        guard += 1;
        if guard > points.len() * points.len() {
            return fan();
        }
        let count = remaining.len();
        let mut clipped = false;
        for position in 0..count {
            let previous = remaining[(position + count - 1) % count];
            let current = remaining[position];
            let next = remaining[(position + 1) % count];
            if cross(points[previous], points[current], points[next]) <= 0.0 {
                continue;
            }
            let contains_other = remaining
                .iter()
                .filter(|index| **index != previous && **index != current && **index != next)
                .any(|index| {
                    inside_triangle(
                        points[*index],
                        points[previous],
                        points[current],
                        points[next],
                    )
                });
            if contains_other {
                continue;
            }
            triangles.push([previous as u32, current as u32, next as u32]);
            remaining.remove(position);
            clipped = true;
            break;
        }
        if !clipped {
            return fan();
        }
    }
    triangles.push([
        remaining[0] as u32,
        remaining[1] as u32,
        remaining[2] as u32,
    ]);
    triangles
}

fn cross(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn inside_triangle(point: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let first = cross(a, b, point);
    let second = cross(b, c, point);
    let third = cross(c, a, point);
    (first >= 0.0 && second >= 0.0 && third >= 0.0)
        || (first <= 0.0 && second <= 0.0 && third <= 0.0)
}

fn signed_area_screen(points: &[Pos2]) -> f32 {
    points
        .iter()
        .enumerate()
        .map(|(index, a)| {
            let b = points[(index + 1) % points.len()];
            a.x * b.y - b.x * a.y
        })
        .sum::<f32>()
        / 2.0
}

fn screen_points(transform: &CanvasTransform, vertices: &[Vec2]) -> Vec<Pos2> {
    vertices
        .iter()
        .map(|vertex| transform.world_to_screen([vertex.x, vertex.y]))
        .collect()
}

/// Draw the periodic copies. They are translucent and never interactive except
/// through selection, which maps a copy back to its semantic basis.
fn draw_neighbors(
    painter: &egui::Painter,
    transform: &CanvasTransform,
    draft: &PeriodicTilingDraft,
) -> usize {
    let polygons = cell_polygons(draft);
    let mut copies = 0;
    for offset in visible_offsets(draft) {
        let translation = lattice_translation(draft, offset);
        for (_, vertices) in &polygons {
            let shifted: Vec<Vec2> = vertices
                .iter()
                .map(|vertex| *vertex + translation)
                .collect();
            let points = screen_points(transform, &shifted);
            fill_polygon(
                painter,
                &points,
                theme::NEIGHBOR_FILL,
                Stroke::new(1.0, theme::NEIGHBOR_STROKE),
            );
        }
        copies += 1;
    }
    copies
}

fn draw_unit_cell(
    painter: &egui::Painter,
    transform: &CanvasTransform,
    draft: &PeriodicTilingDraft,
    state: &TilingCanvasState,
) {
    for (basis, vertices) in cell_polygons(draft) {
        let points = screen_points(transform, &vertices);
        if points.len() < 3 {
            continue;
        }
        let selected = state.selected_basis == Some(basis);
        // Selection changes the outline only. Restyling the fill as well made
        // the selected polygon read as a different kind of tile.
        let stroke = if selected {
            Stroke::new(2.5, theme::state_color(theme::State::Live))
        } else {
            Stroke::new(1.5, theme::CELL_STROKE)
        };
        fill_polygon(painter, &points, theme::CELL_FILL, stroke);
        if selected {
            for (index, point) in points.iter().enumerate() {
                let held = state.selected_vertex == Some(index);
                painter.circle_filled(
                    *point,
                    if held {
                        HANDLE_RADIUS
                    } else {
                        HANDLE_RADIUS - 2.0
                    },
                    if held {
                        theme::state_color(theme::State::Live)
                    } else {
                        theme::CELL_STROKE
                    },
                );
            }
        }
    }
}

/// Show the two translation vectors, since they are what makes the pattern
/// periodic and they are otherwise invisible.
fn draw_lattice_vectors(
    painter: &egui::Painter,
    transform: &CanvasTransform,
    draft: &PeriodicTilingDraft,
) {
    let origin = transform.world_to_screen([0.0, 0.0]);
    for vector in [draft.translation_a, draft.translation_b] {
        let tip = transform.world_to_screen([vector.x, vector.y]);
        painter.line_segment([origin, tip], Stroke::new(1.5, theme::LATTICE_VECTOR));
    }
}

fn draw_construction(
    painter: &egui::Painter,
    transform: &CanvasTransform,
    state: &TilingCanvasState,
) {
    let points = screen_points(transform, &state.construction);
    for pair in points.windows(2) {
        painter.line_segment(
            [pair[0], pair[1]],
            Stroke::new(2.0, theme::state_color(theme::State::Draft)),
        );
    }
    for point in &points {
        painter.circle_filled(*point, 4.0, theme::state_color(theme::State::Draft));
    }
    // A preview edge from the last placed vertex to the pointer, so the user
    // can see the edge before committing to it.
    if let (Some(last), Some(hovered)) = (points.last(), state.hovered) {
        let live = transform.world_to_screen([hovered.x, hovered.y]);
        painter.line_segment(
            [*last, live],
            Stroke::new(1.0, theme::state_color(theme::State::Draft)),
        );
    }
    if let (Some(reason), Some(hovered)) = (&state.rejection, state.hovered) {
        let anchor = transform.world_to_screen([hovered.x, hovered.y]);
        let font = egui::FontId::proportional(13.0);
        let color = theme::state_color(theme::State::Invalid);
        // Near the right edge the message would run off the clipped canvas and
        // lose its ending, so it flips to the other side of the pointer.
        let width = painter
            .layout_no_wrap(reason.clone(), font.clone(), color)
            .rect
            .width();
        let (offset, align) = if anchor.x + 10.0 + width > transform.viewport.right() {
            (egui::vec2(-10.0, -10.0), egui::Align2::RIGHT_BOTTOM)
        } else {
            (egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM)
        };
        painter.text(anchor + offset, align, reason, font, color);
    }
}

/// Selection and constrained vertex dragging.
fn select_and_drag(
    response: &egui::Response,
    transform: &CanvasTransform,
    draft: &PeriodicTilingDraft,
    press_origin: Option<Pos2>,
    state: &mut TilingCanvasState,
    result: &mut TilingCanvasResponse,
) {
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(pointer) = press_origin.or(response.interact_pointer_pos())
        && let Some(handle) = hit_vertex(transform, draft, state, pointer)
    {
        state.dragging = Some(handle);
        state.selected_vertex = Some(handle.1);
        state.selected_prototype = Some(handle.0);
    }

    if let Some((prototype, vertex)) = state.dragging {
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let world = transform.screen_to_world(pointer);
            // A refused drag says why. Silently leaving the vertex where it was
            // is indistinguishable from a canvas that has stopped responding.
            // Held seams no longer refuse anything; what they cannot follow
            // they report, and the readout picks it up from `broken`.
            match move_vertex(draft, &state.seams, prototype, vertex, world) {
                Ok(moved) => {
                    state.rejection = None;
                    state.broken = moved.broken;
                    result.commit = Some(moved.draft);
                }
                Err(reason) => state.rejection = Some(reason),
            }
        }
        if response.drag_stopped() {
            state.dragging = None;
        }
        return;
    }

    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let world = transform.screen_to_world(pointer);
        if let Some(handle) = hit_vertex(transform, draft, state, pointer) {
            state.selected_basis = draft
                .instances
                .iter()
                .find(|instance| instance.prototype == handle.0)
                .map(|instance| instance.id);
            state.selected_prototype = Some(handle.0);
            state.selected_vertex = Some(handle.1);
        } else if let Some(basis) = hit_basis(draft, Vec2::new(world[0], world[1])) {
            state.selected_basis = Some(basis);
            state.selected_vertex = None;
            state.selected_prototype = draft
                .instances
                .iter()
                .find(|instance| instance.id == basis)
                .map(|instance| instance.prototype);
            result.selected = Some(basis);
        }
    }
}

/// Find a vertex handle under the pointer, preferring the selected polygon.
fn hit_vertex(
    transform: &CanvasTransform,
    draft: &PeriodicTilingDraft,
    state: &TilingCanvasState,
    pointer: Pos2,
) -> Option<(PrototypeId, usize)> {
    let order = state
        .selected_prototype
        .into_iter()
        .chain(draft.prototypes.iter().map(|prototype| prototype.id));
    for prototype_id in order {
        let Some(vertices) = placed_vertices(draft, prototype_id) else {
            continue;
        };
        for (index, vertex) in vertices.iter().enumerate() {
            let screen = transform.world_to_screen([vertex.x, vertex.y]);
            if screen.distance(pointer) <= GRAB_RADIUS {
                return Some((prototype_id, index));
            }
        }
    }
    None
}

fn placed_vertices(draft: &PeriodicTilingDraft, prototype: PrototypeId) -> Option<Vec<Vec2>> {
    let instance = draft
        .instances
        .iter()
        .find(|instance| instance.prototype == prototype)?;
    let shape = &draft
        .prototypes
        .iter()
        .find(|entry| entry.id == prototype)?
        .shape;
    let base = polygon::prototype_vertices(shape).ok()?;
    Some(polygon::transform_vertices(&base, instance.transform))
}

/// Which basis a world point falls in, including through a periodic copy: a
/// click on a translucent neighbour selects the basis it is a copy of.
pub fn hit_basis(draft: &PeriodicTilingDraft, point: Vec2) -> Option<BasisId> {
    let polygons = cell_polygons(draft);
    for offset in [[0, 0]].into_iter().chain(visible_offsets(draft)) {
        let translation = lattice_translation(draft, offset);
        for (basis, vertices) in polygons.iter().rev() {
            let shifted: Vec<Vec2> = vertices
                .iter()
                .map(|vertex| *vertex + translation)
                .collect();
            if polygon_contains(point, &shifted) {
                return Some(*basis);
            }
        }
    }
    None
}

fn polygon_contains(point: Vec2, vertices: &[Vec2]) -> bool {
    let mut inside = false;
    for index in 0..vertices.len() {
        let a = vertices[index];
        let b = vertices[(index + 1) % vertices.len()];
        if (a.y > point.y) != (b.y > point.y) {
            let t = (point.y - a.y) / (b.y - a.y);
            if point.x < a.x + t * (b.x - a.x) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Draw what the seam assistant has to say, on the drawing itself.
///
/// An arrow runs from each edge that does not yet meet its partner towards
/// where it has to go, and an edge with no partner at all is outlined in the
/// invalid colour. Saying "0 pairs proposed" in a toolbar told the user
/// nothing about *which* edges or *which way*; this is the same information
/// placed where the problem is.
fn draw_seam_hints(
    painter: &egui::Painter,
    transform: &CanvasTransform,
    draft: &PeriodicTilingDraft,
) {
    let Ok(assessment) = crate::sim::tiling::assess_seams(draft) else {
        return;
    };
    for candidate in &assessment.candidates {
        if candidate.bucket == crate::sim::tiling::SeamBucket::Held {
            continue;
        }
        let Some((start, end)) = edge_endpoints(draft, candidate.constraint.lhs) else {
            continue;
        };
        let colour = match candidate.bucket {
            crate::sim::tiling::SeamBucket::Ready => theme::state_color(theme::State::Draft),
            _ => theme::SEAM_DISTANT,
        };
        // The edge itself is marked as well as the direction it must go. A
        // gap of a hundredth of a unit draws an arrow a couple of pixels long,
        // and a hint too small to see is not a hint; the stroke says *which*
        // edge even when the arrow can only hint at how far.
        painter.line_segment(
            [world_point(transform, start), world_point(transform, end)],
            egui::Stroke::new(2.0, colour),
        );
        let midpoint = (start + end) * 0.5;
        arrow(
            painter,
            transform,
            midpoint,
            midpoint + candidate.hint(),
            colour,
        );
    }
    for orphan in &assessment.orphans {
        let Some((start, end)) = edge_endpoints(draft, orphan.edge) else {
            continue;
        };
        painter.line_segment(
            [world_point(transform, start), world_point(transform, end)],
            egui::Stroke::new(3.0, theme::state_color(theme::State::Invalid)),
        );
    }
}

fn world_point(transform: &CanvasTransform, point: Vec2) -> egui::Pos2 {
    transform.world_to_screen([point.x, point.y])
}

fn edge_endpoints(draft: &PeriodicTilingDraft, edge: EdgeRef) -> Option<(Vec2, Vec2)> {
    let instance = draft
        .instances
        .iter()
        .find(|instance| instance.id == edge.tile)?;
    crate::sim::tiling::snap::world_edge(draft, instance, usize::from(edge.edge))
}

/// A line with a head, so the hint reads as a direction rather than a smear.
fn arrow(
    painter: &egui::Painter,
    transform: &CanvasTransform,
    from: Vec2,
    to: Vec2,
    colour: egui::Color32,
) {
    let tail = world_point(transform, from);
    let head = world_point(transform, to);
    let along = head - tail;
    let length = along.length();
    // Below a few pixels an arrowhead is a blob on top of its own tail, which
    // reads as a defect rather than as a hint.
    if !length.is_finite() || length < 4.0 {
        return;
    }
    let stroke = egui::Stroke::new(2.0, colour);
    painter.line_segment([tail, head], stroke);
    let unit = along / length;
    let side = egui::vec2(-unit.y, unit.x);
    let size = (length * 0.28).clamp(4.0, 11.0);
    painter.line_segment([head, head - unit * size + side * size * 0.5], stroke);
    painter.line_segment([head, head - unit * size - side * size * 0.5], stroke);
}

/// The outcome of dragging one vertex.
#[derive(Clone, Debug, PartialEq)]
pub struct VertexMove {
    pub draft: PeriodicTilingDraft,
    /// Held seams this move pulled apart. Empty when the solver kept up.
    pub broken: Vec<SeamConstraint>,
}

/// Move one vertex, honouring accepted seam constraints.
///
/// With seams accepted the solver moves the whole equivalence class, so an edge
/// that was glued stays glued. Without them the vertex moves alone, and a move
/// that would break the polygon is refused rather than stored.
pub fn move_vertex(
    draft: &PeriodicTilingDraft,
    seams: &[SeamConstraint],
    prototype: PrototypeId,
    vertex: usize,
    to: [f64; 2],
) -> Result<VertexMove, String> {
    let target = Vec2::new(to[0], to[1]);
    if !target.x.is_finite() || !target.y.is_finite() {
        return Err("vertex coordinates must be finite".into());
    }

    // With seams held the solver moves the whole equivalence class, so an edge
    // that was glued stays glued. This is the good case and it is tried first.
    if !seams.is_empty()
        && let Ok(solved) = solve_edge_constraints(
            draft,
            seams,
            Some(DragTarget {
                prototype,
                vertex,
                to: target,
            }),
        )
        && solved.draft != *draft
    {
        return Ok(VertexMove {
            draft: solved.draft,
            broken: Vec::new(),
        });
    }

    // And when it cannot keep up, the drag still happens. Refusing it — which
    // is what this did, under "try a smaller move or cancel the seams" — makes
    // the held seams a cage: the user cannot reach the shape they are aiming
    // for except by throwing away every constraint first. The seams that came
    // apart are reported instead, and can be closed again or released.
    let free = free_move(draft, prototype, vertex, target)?;
    let broken = seams
        .iter()
        .copied()
        .filter(|seam| !crate::sim::tiling::assist::constraint_closes(&free, *seam))
        .collect();
    Ok(VertexMove {
        draft: free,
        broken,
    })
}

/// Move the one vertex and nothing else, refusing only what stops being a
/// polygon. This is a geometry rule rather than a seam rule: a self-crossing
/// outline is not a shape the rest of the program can represent.
fn free_move(
    draft: &PeriodicTilingDraft,
    prototype: PrototypeId,
    vertex: usize,
    target: Vec2,
) -> Result<PeriodicTilingDraft, String> {
    let mut next = draft.clone();
    let entry = next
        .prototypes
        .iter_mut()
        .find(|entry| entry.id == prototype)
        .ok_or("the dragged polygon is missing")?;
    let PrototypeShape::SimplePolygon { vertices } = &mut entry.shape else {
        // A regular polygon has no free vertices; reshaping one means redrawing
        // it, which the Draw tool already covers.
        return Err("a regular polygon has no free vertices; redraw it instead".into());
    };
    *vertices
        .get_mut(vertex)
        .ok_or("the dragged vertex is missing")? = target;
    if let Some(issue) = polygon::validate_polygon(vertices).first() {
        return Err(issue.message.clone());
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{TilingPreset, build_preset};
    use eframe::egui::pos2;

    fn triangle() -> Vec<Vec2> {
        vec![
            Vec2::new(-0.5, -0.4),
            Vec2::new(0.5, -0.4),
            Vec2::new(0.0, 0.5),
        ]
    }

    #[test]
    fn undo_and_redo_walk_the_construction_path_both_ways() {
        let mut state = TilingCanvasState::default();
        state.begin_new_basis();
        for point in triangle() {
            assert!(state.push_vertex(point));
        }
        assert_eq!(state.construction().len(), 3);
        assert!(state.undo_point());
        assert_eq!(state.construction().len(), 2);
        assert!(state.redo_point());
        assert_eq!(state.construction(), triangle().as_slice());
        assert!(!state.redo_point(), "nothing left to redo");
    }

    #[test]
    fn a_new_point_discards_the_redo_trail() {
        let mut state = TilingCanvasState::default();
        state.begin_new_basis();
        state.push_vertex(Vec2::new(0.0, 0.0));
        state.push_vertex(Vec2::new(1.0, 0.0));
        state.undo_point();
        assert!(state.can_redo_point());
        state.push_vertex(Vec2::new(0.0, 1.0));
        assert!(
            !state.can_redo_point(),
            "a fresh point makes the old branch unreachable"
        );
    }

    #[test]
    fn an_invalid_vertex_is_refused_with_a_reason_and_changes_nothing() {
        let mut state = TilingCanvasState::default();
        state.begin_new_basis();
        state.push_vertex(Vec2::new(0.0, 0.0));
        state.push_vertex(Vec2::new(1.0, 0.0));

        for bad in [
            Vec2::new(1.0, 0.0),           // duplicate
            Vec2::new(f64::NAN, 0.0),      // non-finite
            Vec2::new(f64::INFINITY, 1.0), // non-finite
        ] {
            let before = state.construction().to_vec();
            assert!(!state.push_vertex(bad), "{bad:?} must be refused");
            assert!(state.rejection.is_some(), "the reason must be shown");
            assert_eq!(state.construction(), before.as_slice());
        }
    }

    #[test]
    fn a_self_crossing_edge_is_refused() {
        let mut state = TilingCanvasState::default();
        state.begin_new_basis();
        for point in [
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(1.0, -1.0),
        ] {
            state.push_vertex(point);
        }
        assert_eq!(
            state.construction().len(),
            3,
            "the crossing edge must not be appended"
        );
        assert!(state.rejection.is_some());
    }

    #[test]
    fn a_polygon_may_not_grow_past_the_vertex_budget() {
        let mut state = TilingCanvasState::default();
        state.begin_new_basis();
        // A convex fan never self-intersects, so only the budget can stop it.
        for index in 0..70 {
            let angle = f64::from(index) * std::f64::consts::TAU / 70.0;
            state.push_vertex(Vec2::new(angle.cos(), angle.sin()));
        }
        assert_eq!(state.construction().len(), 64);
        assert!(
            state
                .rejection
                .as_deref()
                .is_some_and(|reason| reason.contains("at most")),
            "{:?}",
            state.rejection
        );
    }

    #[test]
    fn a_concave_polygon_triangulates_without_leaving_the_outline() {
        // An arrowhead: vertex 3 points back inside, so a fan from vertex 0
        // would paint outside the shape.
        let arrow = [
            pos2(0.0, 0.0),
            pos2(4.0, 0.0),
            pos2(4.0, 4.0),
            pos2(2.0, 1.0),
            pos2(0.0, 4.0),
        ];
        let triangles = ear_clip(&arrow);
        assert_eq!(triangles.len(), arrow.len() - 2);

        // The triangulated area must equal the polygon's own area. A fan over a
        // concave shape overshoots it.
        let total: f32 = triangles
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (arrow[*a as usize], arrow[*b as usize], arrow[*c as usize]);
                cross(a, b, c).abs() / 2.0
            })
            .sum();
        assert!(
            (total - signed_area_screen(&arrow).abs()).abs() < 1e-3,
            "triangulated {total}, polygon {}",
            signed_area_screen(&arrow).abs()
        );
    }

    #[test]
    fn a_degenerate_outline_still_produces_a_full_triangulation() {
        // Every vertex on one line: no ear exists, so the fallback must still
        // return a complete fan rather than an empty fill.
        let line = [
            pos2(0.0, 0.0),
            pos2(1.0, 0.0),
            pos2(2.0, 0.0),
            pos2(3.0, 0.0),
        ];
        assert_eq!(ear_clip(&line).len(), line.len() - 2);
    }

    #[test]
    fn every_preset_shows_a_full_ring_of_periodic_copies() {
        for preset in TilingPreset::ALL {
            let draft = build_preset(preset, 1.0);
            let offsets = visible_offsets(&draft);
            assert!(
                offsets.len() >= 8,
                "{preset:?} must show the cells around it, found {}",
                offsets.len()
            );
            assert!(
                !offsets.contains(&[0, 0]),
                "{preset:?} centre is not one of its own neighbours"
            );
            // Proven adjacency is a subset of what is drawn, never the reverse.
            for proven in neighbor_offsets(&draft) {
                assert!(offsets.contains(&proven), "{preset:?} hid a real neighbour");
            }
        }
    }

    /// Held seams used to veto a drag they could not follow, which left the
    /// user unable to reach a shape without discarding every constraint first.
    /// The drag now happens and the seams that came apart are named.
    #[test]
    fn a_drag_the_held_seams_cannot_follow_still_moves_the_vertex() {
        let draft = build_preset(TilingPreset::Square, 1.0);
        let prototype = draft.prototypes[0].id;
        let seams = crate::sim::tiling::propose_full_edge_seams(&draft, 1e-6)
            .unwrap()
            .into_iter()
            .map(|proposal| proposal.constraint)
            .collect::<Vec<_>>();
        assert!(!seams.is_empty(), "the square must have seams to hold");

        // A large, deliberately awkward move: far enough that the constrained
        // solve cannot place every linked vertex consistently.
        let moved = move_vertex(&draft, &seams, prototype, 2, [3.7, 2.9])
            .expect("a held seam must not veto a drag");
        let PrototypeShape::SimplePolygon { vertices } = &moved.draft.prototypes[0].shape else {
            panic!("the preset square is a simple polygon");
        };
        assert_ne!(
            vertices[2],
            Vec2::new(1.0, 1.0),
            "the vertex has to have actually moved"
        );

        // Whatever it could not keep, it has to name.
        for seam in &moved.broken {
            assert!(
                !crate::sim::tiling::assist::constraint_closes(&moved.draft, *seam),
                "a seam reported as broken must really be open"
            );
        }
        for seam in &seams {
            if !moved.broken.contains(seam) {
                assert!(
                    crate::sim::tiling::assist::constraint_closes(&moved.draft, *seam),
                    "a seam not reported as broken must really still close"
                );
            }
        }
    }

    /// Geometry rules still bite. A fold is not a shape, held seams or not.
    #[test]
    fn a_move_that_folds_the_polygon_is_still_refused() {
        let draft = build_preset(TilingPreset::Square, 1.0);
        let prototype = draft.prototypes[0].id;
        let reason = move_vertex(&draft, &[], prototype, 2, [-1.0, -1.0])
            .expect_err("a fold must still be refused");
        assert!(!reason.is_empty());
    }

    #[test]
    fn a_degenerate_lattice_draws_no_copies_rather_than_infinite_overlap() {
        let mut draft = build_preset(TilingPreset::Square, 1.0);
        draft.translation_b = draft.translation_a;
        assert!(visible_offsets(&draft).is_empty());
    }

    #[test]
    fn a_click_on_a_periodic_copy_selects_the_basis_it_copies() {
        let draft = build_preset(TilingPreset::Square, 1.0);
        // One period to the right of the centre cell.
        let inside_copy = Vec2::new(1.5, 0.5);
        assert_eq!(hit_basis(&draft, inside_copy), Some(BasisId(0)));
        assert_eq!(hit_basis(&draft, Vec2::new(0.5, 0.5)), Some(BasisId(0)));
    }

    #[test]
    fn dragging_a_vertex_moves_it_and_refuses_a_move_that_breaks_the_polygon() {
        let draft = build_preset(TilingPreset::Square, 1.0);
        let prototype = draft.prototypes[0].id;
        let moved = move_vertex(&draft, &[], prototype, 2, [1.4, 1.2]).expect("a valid move");
        let PrototypeShape::SimplePolygon { vertices } = &moved.draft.prototypes[0].shape else {
            panic!("the preset square is a simple polygon");
        };
        assert_eq!(vertices[2], Vec2::new(1.4, 1.2));

        // Dragging one corner across the opposite edge folds the square. Every
        // refusal carries a reason, because the canvas shows it to the user.
        for bad in [[-1.0, -1.0], [f64::NAN, 0.0]] {
            let reason = move_vertex(&draft, &[], prototype, 2, bad)
                .expect_err("a drag that breaks the polygon must be refused");
            assert!(!reason.is_empty(), "a refusal must say why");
        }
    }

    #[test]
    fn fitting_a_blank_canvas_still_produces_a_usable_transform() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(640.0, 480.0));
        let transform = fit_transform(rect, None);
        assert!(transform.pixels_per_world > 0.0);
        // The origin sits at the centre, so a world click near it lands on screen.
        let center = transform.world_to_screen([0.0, 0.0]);
        assert!(rect.contains(center));
    }

    #[test]
    fn fitting_a_tiling_keeps_the_whole_neighbourhood_on_screen() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(640.0, 480.0));
        let draft = build_preset(TilingPreset::RegularHexagon, 1.0);
        let transform = fit_transform(rect, Some(&draft));
        for offset in neighbor_offsets(&draft) {
            let translation = lattice_translation(&draft, offset);
            for (_, vertices) in cell_polygons(&draft) {
                for vertex in vertices {
                    let point = vertex + translation;
                    let screen = transform.world_to_screen([point.x, point.y]);
                    assert!(
                        rect.expand(1.0).contains(screen),
                        "{point:?} fell outside the viewport"
                    );
                }
            }
        }
    }
}
