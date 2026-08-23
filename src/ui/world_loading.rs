use std::f64::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use egui::{
    pos2, vec2, Align, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
    UiBuilder,
};

use crate::view::{
    SphericalProjection, SphericalProjectionKind, WorldLoadingPalette, WORLD_LOADING_PALETTE,
};

const WORLD_LOADING_PIECE_COUNT: usize = 20;
const WORLD_LOADING_OUTLINE_LATITUDE_STEPS: usize = 24;
const WORLD_LOADING_TRANSITION_SECONDS: f64 = 0.3;
const WORLD_LOADING_STAGGER_WINDOW_SECONDS: f64 = 0.6;
const WORLD_LOADING_HOLD_SECONDS: f64 = 0.3;
const WORLD_LOADING_ASSEMBLY_END_SECONDS: f64 =
    WORLD_LOADING_STAGGER_WINDOW_SECONDS + WORLD_LOADING_TRANSITION_SECONDS;
const WORLD_LOADING_EXIT_START_SECONDS: f64 =
    WORLD_LOADING_ASSEMBLY_END_SECONDS + WORLD_LOADING_HOLD_SECONDS;
const WORLD_LOADING_CYCLE_SECONDS: f64 = WORLD_LOADING_EXIT_START_SECONDS
    + WORLD_LOADING_STAGGER_WINDOW_SECONDS
    + WORLD_LOADING_TRANSITION_SECONDS;
const WORLD_LOADING_TRAVEL_VIEWBOX_UNITS: f32 = 18.0;
const WORLD_LOADING_PROJECTION_HEIGHT_FRACTION: f64 = 0.8;
const WORLD_LOADING_VIEWBOX_WIDTH: f32 = 1_000.0;
const WORLD_LOADING_VIEWBOX_HEIGHT: f32 = 500.0;
const CLIP_CROSS_EPSILON: f32 =
    f32::EPSILON * WORLD_LOADING_VIEWBOX_WIDTH * WORLD_LOADING_VIEWBOX_HEIGHT;
const MAP_WIDTH_FRACTION: f32 = 0.78;
const MAP_MAX_WIDTH_POINTS: f32 = 900.0;
const STAGE_PADDING_HEIGHT_FRACTION: f32 = 0.04;
const STAGE_PADDING_MIN_POINTS: f32 = 26.0;
const STAGE_PADDING_MAX_POINTS: f32 = 54.0;
const COMPACT_VIEWPORT_HEIGHT_POINTS: f32 = 680.0;
const COMPACT_STAGE_PADDING_POINTS: f32 = 12.0;
const COPY_OVERLAP_WIDTH_FRACTION: f32 = -0.016;
const COPY_OVERLAP_MIN_POINTS: f32 = -16.0;
const COPY_OVERLAP_MAX_POINTS: f32 = -7.0;
const EYEBROW_FONT_SIZE: f32 = 9.0;
const EYEBROW_BOTTOM_GAP_POINTS: f32 = 11.0;
const TITLE_VIEWPORT_WIDTH_FRACTION: f32 = 0.032;
const TITLE_MIN_FONT_SIZE: f32 = 28.0;
const TITLE_MAX_FONT_SIZE: f32 = 44.0;
const CLOCK_TOP_GAP_POINTS: f32 = 13.0;
const CLOCK_FONT_SIZE: f32 = 15.0;
const PROCESS_TOP_GAP_POINTS: f32 = 15.0;
const PROCESS_FONT_SIZE: f32 = 11.0;
const MAP_SURFACE_ALPHA: f32 = 0.72;
const MAP_OUTLINE_ALPHA: f32 = 0.20;
const MAP_OUTLINE_WIDTH_POINTS: f32 = 1.25;
const MAP_PIECE_SEAM_ALPHA: f32 = 0.50;
const MAP_PIECE_SEAM_WIDTH_POINTS: f32 = 2.0;
const REPAINT_INTERVAL: Duration = Duration::from_millis(16);
const AMBIENT_HORIZONTAL_INSET_FRACTION: f32 = 0.12;
const AMBIENT_VERTICAL_INSET_FRACTION: f32 = 0.18;
const AMBIENT_BLUR_POINTS: f32 = 56.0;
const AMBIENT_ALPHA: f32 = 0.17;
const AMBIENT_GLOW_LAYER_COUNT: usize = 4;

const EYEBROW: &str = "WORLD FORMATION ENGINE";
const ACTIVE_TITLE: &str = "世界正在成形";
const CANCELLING_TITLE: &str = "正在取消构建";
const PROCESS_COPY: &str = "构筑地表 · 求解海陆 · 发布新世界";

#[derive(Debug, Clone, Copy)]
struct LoadingPiece {
    vertices: &'static [usize],
    tone: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LoadingPieceFrame {
    opacity: f32,
    travel: f32,
}

const WORLD_LOADING_VERTICES: [[f32; 2]; 42] = [
    [0.0, 0.0],
    [429.46, 308.59],
    [413.82, 267.92],
    [459.79, 161.84],
    [555.1, 153.17],
    [610.62, 253.11],
    [569.52, 330.47],
    [400.31, 500.0],
    [396.28, 500.0],
    [386.07, 395.36],
    [590.74, 382.82],
    [405.76, 88.91],
    [277.88, 224.23],
    [269.99, 203.36],
    [299.18, 135.25],
    [710.72, 131.29],
    [739.09, 190.39],
    [714.38, 236.89],
    [595.81, 85.32],
    [412.97, 0.0],
    [591.32, 0.0],
    [764.66, 327.39],
    [760.59, 351.17],
    [610.8, 403.6],
    [226.8, 342.27],
    [224.66, 330.67],
    [632.22, 500.0],
    [216.53, 0.0],
    [772.5, 0.0],
    [835.0, 500.0],
    [155.56, 500.0],
    [47.3, 0.0],
    [168.4, 168.2],
    [897.75, 276.69],
    [821.12, 170.59],
    [930.3, 0.0],
    [113.61, 277.79],
    [1_000.0, 306.76],
    [1_000.0, 500.0],
    [0.0, 500.0],
    [0.0, 301.29],
    [1_000.0, 0.0],
];

// Shared indices keep every tessellation seam as one geometric fact. Array order
// is the approved center-out animation order.
const WORLD_LOADING_PIECES: [LoadingPiece; WORLD_LOADING_PIECE_COUNT] = [
    LoadingPiece {
        vertices: &[1, 2, 3, 4, 5, 6],
        tone: 0,
    },
    LoadingPiece {
        vertices: &[7, 8, 9, 1, 6, 10],
        tone: 1,
    },
    LoadingPiece {
        vertices: &[11, 3, 2, 12, 13, 14],
        tone: 2,
    },
    LoadingPiece {
        vertices: &[15, 16, 17, 5, 4, 18],
        tone: 3,
    },
    LoadingPiece {
        vertices: &[19, 20, 18, 4, 3, 11],
        tone: 4,
    },
    LoadingPiece {
        vertices: &[10, 6, 5, 17, 21, 22, 23],
        tone: 5,
    },
    LoadingPiece {
        vertices: &[2, 1, 9, 24, 25, 12],
        tone: 6,
    },
    LoadingPiece {
        vertices: &[26, 7, 10, 23],
        tone: 0,
    },
    LoadingPiece {
        vertices: &[27, 19, 11, 14],
        tone: 1,
    },
    LoadingPiece {
        vertices: &[20, 28, 15, 18],
        tone: 2,
    },
    LoadingPiece {
        vertices: &[29, 26, 23, 22],
        tone: 3,
    },
    LoadingPiece {
        vertices: &[8, 30, 24, 9],
        tone: 4,
    },
    LoadingPiece {
        vertices: &[31, 27, 14, 13, 32],
        tone: 5,
    },
    LoadingPiece {
        vertices: &[33, 21, 17, 16, 34],
        tone: 6,
    },
    LoadingPiece {
        vertices: &[28, 35, 34, 16, 15],
        tone: 0,
    },
    LoadingPiece {
        vertices: &[13, 12, 25, 36, 32],
        tone: 1,
    },
    LoadingPiece {
        vertices: &[37, 38, 29, 22, 21, 33],
        tone: 2,
    },
    LoadingPiece {
        vertices: &[25, 24, 30, 39, 40, 36],
        tone: 3,
    },
    LoadingPiece {
        vertices: &[0, 31, 32, 36, 40],
        tone: 4,
    },
    LoadingPiece {
        vertices: &[35, 41, 37, 33, 34],
        tone: 5,
    },
];

pub(crate) fn show_world_loading(
    ui: &mut Ui,
    elapsed: Duration,
    reduce_motion: bool,
    cancelling: bool,
) {
    ui.ctx().request_repaint_after(REPAINT_INTERVAL);

    let available = ui.available_size();
    let (viewport, _) = ui.allocate_exact_size(available, Sense::hover());
    let painter = ui.painter_at(viewport);
    painter.rect_filled(viewport, 0.0, opaque(WORLD_LOADING_PALETTE.background));

    let title_size = (viewport.width() * TITLE_VIEWPORT_WIDTH_FRACTION)
        .clamp(TITLE_MIN_FONT_SIZE, TITLE_MAX_FONT_SIZE);
    let status_height = ui.fonts(|fonts| {
        fonts.row_height(&FontId::proportional(EYEBROW_FONT_SIZE))
            + EYEBROW_BOTTOM_GAP_POINTS
            + fonts.row_height(&FontId::proportional(title_size))
            + CLOCK_TOP_GAP_POINTS
            + fonts.row_height(&FontId::monospace(CLOCK_FONT_SIZE))
            + PROCESS_TOP_GAP_POINTS
            + fonts.row_height(&FontId::proportional(PROCESS_FONT_SIZE))
    });
    let stage_padding = if viewport.height() <= COMPACT_VIEWPORT_HEIGHT_POINTS {
        COMPACT_STAGE_PADDING_POINTS
    } else {
        (viewport.height() * STAGE_PADDING_HEIGHT_FRACTION)
            .clamp(STAGE_PADDING_MIN_POINTS, STAGE_PADDING_MAX_POINTS)
    };
    let copy_overlap = (viewport.width() * COPY_OVERLAP_WIDTH_FRACTION)
        .clamp(COPY_OVERLAP_MIN_POINTS, COPY_OVERLAP_MAX_POINTS);
    let map_height_limit =
        (viewport.height() - 2.0 * stage_padding - status_height - copy_overlap).max(1.0);
    let map_width = (viewport.width() * MAP_WIDTH_FRACTION)
        .min(MAP_MAX_WIDTH_POINTS)
        .min(map_height_limit * (WORLD_LOADING_VIEWBOX_WIDTH / WORLD_LOADING_VIEWBOX_HEIGHT))
        .max(1.0);
    let map_size = vec2(
        map_width,
        map_width * WORLD_LOADING_VIEWBOX_HEIGHT / WORLD_LOADING_VIEWBOX_WIDTH,
    );
    let group_height = map_size.y + copy_overlap + status_height;
    let group_top = viewport.center().y - group_height * 0.5;
    let map_rect = Rect::from_min_size(
        pos2(viewport.center().x - map_size.x * 0.5, group_top),
        map_size,
    );

    paint_loading_map(
        &painter,
        map_rect,
        elapsed.as_secs_f64(),
        reduce_motion,
        &WORLD_LOADING_PALETTE,
    );

    let status_rect = Rect::from_min_max(
        pos2(viewport.left(), map_rect.bottom() + copy_overlap),
        pos2(viewport.right(), viewport.bottom()),
    );
    let mut status_ui = ui.new_child(
        UiBuilder::new()
            .id_salt("world_build_loading_status")
            .max_rect(status_rect)
            .layout(Layout::top_down(Align::Center)),
    );
    status_ui.spacing_mut().item_spacing.y = 0.0;
    status_ui.label(
        RichText::new(EYEBROW)
            .size(EYEBROW_FONT_SIZE)
            .strong()
            .color(opaque(WORLD_LOADING_PALETTE.accent)),
    );
    status_ui.add_space(EYEBROW_BOTTOM_GAP_POINTS);
    status_ui.label(
        RichText::new(if cancelling {
            CANCELLING_TITLE
        } else {
            ACTIVE_TITLE
        })
        .size(title_size)
        .color(opaque(WORLD_LOADING_PALETTE.ink)),
    );
    status_ui.add_space(CLOCK_TOP_GAP_POINTS);
    status_ui.label(
        RichText::new(format!("{} · ELAPSED", format_elapsed(elapsed)))
            .size(CLOCK_FONT_SIZE)
            .monospace()
            .color(opaque(WORLD_LOADING_PALETTE.outline)),
    );
    status_ui.add_space(PROCESS_TOP_GAP_POINTS);
    status_ui.label(
        RichText::new(PROCESS_COPY)
            .size(PROCESS_FONT_SIZE)
            .color(opaque(WORLD_LOADING_PALETTE.muted)),
    );
}

fn paint_loading_map(
    painter: &egui::Painter,
    map_rect: Rect,
    elapsed_seconds: f64,
    reduce_motion: bool,
    palette: &WorldLoadingPalette,
) {
    let core_radius = vec2(
        map_rect.width() * (0.5 - AMBIENT_HORIZONTAL_INSET_FRACTION),
        map_rect.height() * (0.5 - AMBIENT_VERTICAL_INSET_FRACTION),
    );
    for layer in 0..AMBIENT_GLOW_LAYER_COUNT {
        let blur_fraction =
            (AMBIENT_GLOW_LAYER_COUNT - layer) as f32 / AMBIENT_GLOW_LAYER_COUNT as f32;
        painter.add(Shape::ellipse_filled(
            map_rect.center(),
            core_radius + vec2(AMBIENT_BLUR_POINTS, AMBIENT_BLUR_POINTS) * blur_fraction,
            translucent(
                palette.accent,
                AMBIENT_ALPHA / AMBIENT_GLOW_LAYER_COUNT as f32,
            ),
        ));
    }

    let outline = equal_earth_outline();
    let screen_outline = to_screen_points(&outline, map_rect);
    painter.add(Shape::convex_polygon(
        screen_outline.clone(),
        translucent(palette.surface, MAP_SURFACE_ALPHA),
        Stroke::NONE,
    ));

    let frame = loading_frame(elapsed_seconds, reduce_motion);
    for (index, piece_frame) in frame.into_iter().enumerate() {
        if piece_frame.opacity <= f32::EPSILON {
            continue;
        }
        let points = clipped_piece_to_outline(index, piece_frame.travel, &outline);
        if points.len() < 3 {
            continue;
        }
        let piece = WORLD_LOADING_PIECES[index];
        painter.add(Shape::convex_polygon(
            to_screen_points(&points, map_rect),
            translucent(palette.tones[piece.tone], piece_frame.opacity),
            Stroke::new(
                MAP_PIECE_SEAM_WIDTH_POINTS,
                translucent(
                    palette.background,
                    MAP_PIECE_SEAM_ALPHA * piece_frame.opacity,
                ),
            ),
        ));
    }

    painter.add(Shape::closed_line(
        screen_outline,
        Stroke::new(
            MAP_OUTLINE_WIDTH_POINTS,
            translucent(palette.outline, MAP_OUTLINE_ALPHA),
        ),
    ));
}

fn loading_frame(
    elapsed_seconds: f64,
    reduce_motion: bool,
) -> [LoadingPieceFrame; WORLD_LOADING_PIECE_COUNT] {
    let loop_seconds = elapsed_seconds.rem_euclid(WORLD_LOADING_CYCLE_SECONDS);
    let stagger_seconds =
        WORLD_LOADING_STAGGER_WINDOW_SECONDS / (WORLD_LOADING_PIECE_COUNT - 1) as f64;
    std::array::from_fn(|index| {
        let delay = index as f64 * stagger_seconds;
        let enter = smoothstep((loop_seconds - delay) / WORLD_LOADING_TRANSITION_SECONDS);
        let exit = smoothstep(
            (loop_seconds - WORLD_LOADING_EXIT_START_SECONDS - delay)
                / WORLD_LOADING_TRANSITION_SECONDS,
        );
        LoadingPieceFrame {
            opacity: enter * (1.0 - exit),
            travel: if reduce_motion {
                0.0
            } else {
                1.0 - enter + exit
            },
        }
    })
}

fn smoothstep(value: f64) -> f32 {
    let t = value.clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

fn equal_earth_outline() -> Vec<[f32; 2]> {
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0)
        .expect("fixed Equal Earth projection must be valid");
    let bounds = projection.bounds();
    let projection_width = bounds.max_x() - bounds.min_x();
    let projection_height = bounds.max_y() - bounds.min_y();
    let projection_aspect = projection_width / projection_height;
    let viewbox_aspect = f64::from(WORLD_LOADING_VIEWBOX_WIDTH / WORLD_LOADING_VIEWBOX_HEIGHT);
    let projection_width_fraction =
        WORLD_LOADING_PROJECTION_HEIGHT_FRACTION * projection_aspect / viewbox_aspect;
    let center_x = (bounds.min_x() + bounds.max_x()) * 0.5;
    let center_y = (bounds.min_y() + bounds.max_y()) * 0.5;

    let mut outline = Vec::with_capacity(2 * (WORLD_LOADING_OUTLINE_LATITUDE_STEPS + 1));
    for index in 0..=WORLD_LOADING_OUTLINE_LATITUDE_STEPS {
        let latitude = FRAC_PI_2 - PI * index as f64 / WORLD_LOADING_OUTLINE_LATITUDE_STEPS as f64;
        outline.push(project_outline_point(
            projection,
            latitude,
            PI,
            center_x,
            center_y,
            projection_width,
            projection_height,
            projection_width_fraction,
        ));
    }
    for index in 0..=WORLD_LOADING_OUTLINE_LATITUDE_STEPS {
        let latitude = -FRAC_PI_2 + PI * index as f64 / WORLD_LOADING_OUTLINE_LATITUDE_STEPS as f64;
        outline.push(project_outline_point(
            projection,
            latitude,
            -PI,
            center_x,
            center_y,
            projection_width,
            projection_height,
            projection_width_fraction,
        ));
    }
    outline
}

#[allow(clippy::too_many_arguments)]
fn project_outline_point(
    projection: SphericalProjection,
    latitude: f64,
    longitude: f64,
    center_x: f64,
    center_y: f64,
    projection_width: f64,
    projection_height: f64,
    projection_width_fraction: f64,
) -> [f32; 2] {
    let point = projection
        .forward_latitude_relative_longitude(latitude, longitude)
        .expect("fixed outline samples must lie inside Equal Earth");
    [
        ((0.5 + (point.x() - center_x) / projection_width * projection_width_fraction)
            * f64::from(WORLD_LOADING_VIEWBOX_WIDTH)) as f32,
        ((0.5
            - (point.y() - center_y) / projection_height
                * WORLD_LOADING_PROJECTION_HEIGHT_FRACTION)
            * f64::from(WORLD_LOADING_VIEWBOX_HEIGHT)) as f32,
    ]
}

fn clipped_piece_to_outline(
    piece_index: usize,
    travel: f32,
    outline: &[[f32; 2]],
) -> Vec<[f32; 2]> {
    let piece = WORLD_LOADING_PIECES[piece_index];
    let (centroid_x, centroid_y) = piece.vertices.iter().fold((0.0, 0.0), |sum, &index| {
        let point = WORLD_LOADING_VERTICES[index];
        (sum.0 + point[0], sum.1 + point[1])
    });
    let vertex_count = piece.vertices.len() as f32;
    let direction_x = centroid_x / vertex_count - WORLD_LOADING_VIEWBOX_WIDTH * 0.5;
    let direction_y = centroid_y / vertex_count - WORLD_LOADING_VIEWBOX_HEIGHT * 0.5;
    let direction_length = direction_x.hypot(direction_y);
    let offset = WORLD_LOADING_TRAVEL_VIEWBOX_UNITS * travel;
    let translation = [
        direction_x / direction_length * offset,
        direction_y / direction_length * offset,
    ];
    let translated: Vec<_> = piece
        .vertices
        .iter()
        .map(|&index| {
            let point = WORLD_LOADING_VERTICES[index];
            [point[0] + translation[0], point[1] + translation[1]]
        })
        .collect();
    let clipped = clip_convex_polygon(&translated, outline);
    debug_assert!(clipped
        .iter()
        .all(|&point| point_is_inside_convex_polygon(point, outline)));
    clipped
}

fn clip_convex_polygon(subject: &[[f32; 2]], clip_window: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let orientation = polygon_area(clip_window).signum();
    let mut output = subject.to_vec();
    for edge_index in 0..clip_window.len() {
        if output.is_empty() {
            break;
        }
        let clip_start = clip_window[edge_index];
        let clip_end = clip_window[(edge_index + 1) % clip_window.len()];
        let input = std::mem::take(&mut output);
        let mut previous = *input.last().expect("non-empty polygon");
        let mut previous_inside = point_is_inside_edge(previous, clip_start, clip_end, orientation);
        for current in input {
            let current_inside = point_is_inside_edge(current, clip_start, clip_end, orientation);
            if current_inside {
                if !previous_inside {
                    output.push(segment_edge_intersection(
                        previous, current, clip_start, clip_end,
                    ));
                }
                output.push(current);
            } else if previous_inside {
                output.push(segment_edge_intersection(
                    previous, current, clip_start, clip_end,
                ));
            }
            previous = current;
            previous_inside = current_inside;
        }
    }
    output
}

fn point_is_inside_convex_polygon(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let orientation = polygon_area(polygon).signum();
    (0..polygon.len()).all(|index| {
        point_is_inside_edge(
            point,
            polygon[index],
            polygon[(index + 1) % polygon.len()],
            orientation,
        )
    })
}

fn point_is_inside_edge(
    point: [f32; 2],
    edge_start: [f32; 2],
    edge_end: [f32; 2],
    orientation: f32,
) -> bool {
    cross(
        [edge_end[0] - edge_start[0], edge_end[1] - edge_start[1]],
        [point[0] - edge_start[0], point[1] - edge_start[1]],
    ) * orientation
        >= -CLIP_CROSS_EPSILON
}

fn segment_edge_intersection(
    segment_start: [f32; 2],
    segment_end: [f32; 2],
    edge_start: [f32; 2],
    edge_end: [f32; 2],
) -> [f32; 2] {
    let edge = [edge_end[0] - edge_start[0], edge_end[1] - edge_start[1]];
    let start_distance = cross(
        edge,
        [
            segment_start[0] - edge_start[0],
            segment_start[1] - edge_start[1],
        ],
    );
    let end_distance = cross(
        edge,
        [
            segment_end[0] - edge_start[0],
            segment_end[1] - edge_start[1],
        ],
    );
    let amount = start_distance / (start_distance - end_distance);
    [
        segment_start[0] + (segment_end[0] - segment_start[0]) * amount,
        segment_start[1] + (segment_end[1] - segment_start[1]) * amount,
    ]
}

fn cross(left: [f32; 2], right: [f32; 2]) -> f32 {
    left[0] * right[1] - left[1] * right[0]
}

fn polygon_area(points: &[[f32; 2]]) -> f32 {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let next = points[(index + 1) % points.len()];
            point[0] * next[1] - next[0] * point[1]
        })
        .sum::<f32>()
        * 0.5
}

fn to_screen_points(points: &[[f32; 2]], map_rect: Rect) -> Vec<Pos2> {
    points
        .iter()
        .map(|point| {
            pos2(
                map_rect.left() + point[0] / WORLD_LOADING_VIEWBOX_WIDTH * map_rect.width(),
                map_rect.top() + point[1] / WORLD_LOADING_VIEWBOX_HEIGHT * map_rect.height(),
            )
        })
        .collect()
}

fn format_elapsed(elapsed: Duration) -> String {
    let whole_seconds = elapsed.as_secs();
    format!("{:02}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

fn opaque(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn translucent(rgb: [u8; 3], alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        rgb[0],
        rgb[1],
        rgb[2],
        (alpha.clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Duration;

    use super::*;

    const EPSILON: f32 = 1.0e-4;

    #[test]
    fn puzzle_is_twenty_convex_shared_edge_pieces_with_safe_tones() {
        assert_eq!(WORLD_LOADING_PIECES.len(), 20);

        let mut used_tones = BTreeSet::new();
        for piece in WORLD_LOADING_PIECES {
            assert!(piece.vertices.len() >= 3);
            assert!(piece
                .vertices
                .iter()
                .all(|&index| index < WORLD_LOADING_VERTICES.len()));
            assert!(is_convex(piece.vertices));
            used_tones.insert(piece.tone);
        }
        assert_eq!(used_tones.len(), WORLD_LOADING_PALETTE.tones.len());

        for (left, left_piece) in WORLD_LOADING_PIECES.iter().enumerate() {
            for (right, right_piece) in WORLD_LOADING_PIECES.iter().enumerate().skip(left + 1) {
                if pieces_share_edge(left_piece.vertices, right_piece.vertices) {
                    assert_ne!(
                        left_piece.tone, right_piece.tone,
                        "adjacent pieces {left} and {right} share a tone"
                    );
                }
            }
        }
    }

    #[test]
    fn canonical_equal_earth_outline_has_expected_projection_proportions() {
        let outline = equal_earth_outline();
        let min_x = outline
            .iter()
            .map(|point| point[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = outline
            .iter()
            .map(|point| point[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = outline
            .iter()
            .map(|point| point[1])
            .fold(f32::INFINITY, f32::min);
        let max_y = outline
            .iter()
            .map(|point| point[1])
            .fold(f32::NEG_INFINITY, f32::max);

        assert!((min_y - 0.1 * WORLD_LOADING_VIEWBOX_HEIGHT).abs() < EPSILON);
        assert!((max_y - 0.9 * WORLD_LOADING_VIEWBOX_HEIGHT).abs() < EPSILON);
        let aspect = (max_x - min_x) / (max_y - min_y);
        assert!((2.04..2.07).contains(&aspect));

        let polar_width = outline[outline.len() - 1][0] - outline[0][0];
        let equatorial_width = max_x - min_x;
        assert!((0.59..0.595).contains(&(polar_width.abs() / equatorial_width)));
    }

    #[test]
    fn resting_pieces_clip_to_exactly_one_projection_surface() {
        let outline = equal_earth_outline();
        let pieces_area: f32 = (0..WORLD_LOADING_PIECES.len())
            .map(|index| polygon_area(&clipped_piece_to_outline(index, 0.0, &outline)).abs())
            .sum();
        let outline_area = polygon_area(&outline).abs();
        let accumulated_area_tolerance =
            outline_area * f32::EPSILON * WORLD_LOADING_PIECE_COUNT as f32;

        assert!(
            (pieces_area - outline_area).abs() <= accumulated_area_tolerance,
            "pieces {pieces_area}, outline {outline_area}, delta {}",
            pieces_area - outline_area
        );
        for index in 0..WORLD_LOADING_PIECES.len() {
            let clipped = clipped_piece_to_outline(index, 1.0, &outline);
            assert!(!clipped.is_empty());
            assert!(clipped
                .iter()
                .all(|point| point_is_inside_convex_polygon(*point, &outline)));
        }
    }

    #[test]
    fn pieces_enter_and_exit_in_order_with_outward_motion() {
        let entering = loading_frame(0.25, false);
        assert!(entering[0].opacity > entering[1].opacity);
        assert!(entering[1].opacity > entering[2].opacity);
        assert!(entering[0].travel < entering[1].travel);
        assert!(entering[1].travel < entering[2].travel);

        let exiting = loading_frame(1.35, false);
        assert!((exiting[0].opacity - 0.5).abs() < EPSILON);
        assert!(exiting[1].opacity > exiting[0].opacity);
        assert!(exiting[2].opacity > exiting[1].opacity);
        assert!(exiting[2].opacity < 1.0);
        assert_eq!(exiting[5].opacity, 1.0);
        assert!(exiting[0].travel > exiting[1].travel);
        assert!(exiting[1].travel > exiting[2].travel);
        assert_eq!(exiting[5].travel, 0.0);
    }

    #[test]
    fn choreography_loops_without_drift_and_reduced_motion_only_removes_travel() {
        let first = loading_frame(0.25, false);
        assert_eq!(first, loading_frame(2.35, false));

        let reduced = loading_frame(0.25, true);
        for (full, reduced) in first.into_iter().zip(reduced) {
            assert_eq!(full.opacity, reduced.opacity);
            assert_eq!(reduced.travel, 0.0);
        }
    }

    #[test]
    fn elapsed_clock_is_stable_beyond_one_hour() {
        assert_eq!(format_elapsed(Duration::ZERO), "00:00");
        assert_eq!(format_elapsed(Duration::from_millis(65_900)), "01:05");
        assert_eq!(format_elapsed(Duration::from_secs(3_600)), "60:00");
    }

    #[test]
    fn loading_view_draws_all_pieces_and_semantic_status_text() {
        let context = egui::Context::default();
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    show_world_loading(ui, Duration::from_millis(900), false, false);
                });
            },
        );

        let mut texts = Vec::new();
        let mut filled_paths = 0;
        for shape in &output.shapes {
            collect_rendered_content(&shape.shape, &mut texts, &mut filled_paths);
        }
        assert!(texts.iter().any(|text| text == ACTIVE_TITLE));
        assert!(texts.iter().any(|text| text == EYEBROW));
        assert!(texts.iter().any(|text| text.contains("00:00")));
        assert!(texts.iter().any(|text| text == PROCESS_COPY));
        assert!(filled_paths > WORLD_LOADING_PIECE_COUNT);
    }

    fn is_convex(indices: &[usize]) -> bool {
        let mut direction = 0.0_f32;
        for index in 0..indices.len() {
            let a = WORLD_LOADING_VERTICES[indices[index]];
            let b = WORLD_LOADING_VERTICES[indices[(index + 1) % indices.len()]];
            let c = WORLD_LOADING_VERTICES[indices[(index + 2) % indices.len()]];
            let turn = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            if turn.abs() <= f32::EPSILON {
                continue;
            }
            if direction != 0.0 && turn.signum() != direction {
                return false;
            }
            direction = turn.signum();
        }
        direction != 0.0
    }

    fn pieces_share_edge(left: &[usize], right: &[usize]) -> bool {
        (0..left.len()).any(|left_index| {
            let edge = [left[left_index], left[(left_index + 1) % left.len()]];
            (0..right.len()).any(|right_index| {
                let candidate = [right[right_index], right[(right_index + 1) % right.len()]];
                edge == [candidate[1], candidate[0]]
            })
        })
    }

    fn collect_rendered_content(
        shape: &egui::epaint::Shape,
        texts: &mut Vec<String>,
        filled_paths: &mut usize,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            egui::epaint::Shape::Path(path) if path.fill != Color32::TRANSPARENT => {
                *filled_paths += 1;
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_rendered_content(shape, texts, filled_paths);
                }
            }
            _ => {}
        }
    }
}
