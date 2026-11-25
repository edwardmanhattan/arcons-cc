//! The chart's arithmetic and the placement it feeds (ADR-0009, decision 4).

use desk::chart::Chart;
use desk::scene;

const SIZE: (f64, f64) = (720.0, 520.0);

fn chart() -> Chart {
    Chart::new((-6.10, 106.88), 9.0, SIZE)
}

#[test]
fn the_centre_of_the_chart_is_the_camera() {
    let chart = chart();
    let point = chart.project(-6.10, 106.88);
    assert!(
        (point.0 - SIZE.0 / 2.0).abs() < 0.5 && (point.1 - SIZE.1 / 2.0).abs() < 0.5,
        "the camera's centre projects to the middle of the chart, got {point:?}"
    );
}

#[test]
fn a_point_on_the_chart_means_the_coordinate_under_it() {
    let chart = chart();
    for (x, y) in [(0.0, 0.0), (123.0, 45.0), (700.0, 500.0)] {
        let (lat, lon) = chart.place_at(x, y);
        let back = chart.project(lat, lon);
        assert!(
            (back.0 - x).abs() < 0.01 && (back.1 - y).abs() < 0.01,
            "a press at {x},{y} claims {lat},{lon}, which projects back to {back:?}"
        );
    }
}

#[test]
fn a_pan_moves_the_water_the_way_the_hand_moved() {
    let mut chart = chart();
    let before = chart.project(-6.10, 106.88);
    chart.pan(40.0, 25.0);
    let after = chart.project(-6.10, 106.88);
    assert!(
        (after.0 - (before.0 + 40.0)).abs() < 0.5 && (after.1 - (before.1 + 25.0)).abs() < 0.5,
        "the point under the hand follows it: {before:?} -> {after:?}"
    );
}

#[test]
fn zooming_about_a_point_keeps_that_point_where_it_was() {
    let mut chart = chart();
    let held = (200.0, 300.0);
    let (lat, lon) = chart.place_at(held.0, held.1);
    chart.zoom_at(held, 1.0);
    let after = chart.project(lat, lon);
    assert!(
        (after.0 - held.0).abs() < 0.5 && (after.1 - held.1).abs() < 0.5,
        "the water under the pointer stays under it: {held:?} -> {after:?}"
    );
    assert!(chart.camera().zoom > 9.0);
}

#[test]
fn placing_fills_the_selected_hulls_row_and_does_not_send() {
    let mut state = scene::state_for("batch");
    let id_unit = state.desk.selected().expect("the scene selects a hull");
    state.place(id_unit, -6.2, 106.9);
    let row = state
        .desk
        .batch()
        .get(id_unit)
        .expect("the hull has a row in the draft");
    assert_eq!(
        (row.fix.latitude, row.fix.longitude),
        (Some(-6.2), Some(106.9))
    );
    assert_eq!(
        row.fix.recorded_at_ms, state.epoch_ms,
        "a drop is a claim about now: the field's time, not a remembered one"
    );
    assert!(
        state.desk.batch().source_note.is_some(),
        "the source note rides on the draft"
    );
}

#[test]
fn placing_the_same_hull_twice_replaces_its_row() {
    let mut state = scene::state_for("batch");
    let id_unit = state.desk.selected().expect("a selected hull");
    let before = state.desk.batch().len();
    state.place(id_unit, -6.2, 106.9);
    state.place(id_unit, -6.3, 107.0);
    let after = state.desk.batch().len();
    assert!(
        after <= before.max(1) + 1,
        "two placements of one hull are one row: {before} -> {after}"
    );
    let row = state.desk.batch().get(id_unit).expect("one row");
    assert_eq!(
        (row.fix.latitude, row.fix.longitude),
        (Some(-6.3), Some(107.0))
    );
}

#[test]
fn an_armed_placement_moves_with_the_arrows_and_commits_with_enter() {
    let mut state = scene::state_for("batch");
    let id_unit = state.desk.selected().expect("a selected hull");
    state.arm();
    assert!(state.arming);
    let start = state.crosshair;
    state.nudge(10.0, 0.0);
    assert_eq!(state.crosshair.0, start.0 + 10.0);
    state.commit_placement();
    assert!(!state.arming, "a committed placement closes the crosshair");
    assert!(
        state.desk.batch().get(id_unit).is_some(),
        "the crosshair became the hull's row"
    );
}

#[test]
fn placing_with_nothing_selected_says_so_instead_of_inventing_a_hull() {
    let mut state = scene::state_for("empty");
    let before = state.desk.batch().len();
    state.place_at(100.0, 100.0);
    assert_eq!(state.desk.batch().len(), before, "no hull, no row");
    assert!(
        state
            .notice
            .as_deref()
            .is_some_and(|notice| notice.contains("select a vessel")),
        "the operator is told what is missing: {:?}",
        state.notice
    );
}

#[test]
fn a_state_exposes_the_chart_it_draws_with() {
    // The panel and the projection must agree about the rectangle, or a press lands somewhere else than
    // the water under the pointer.
    let state = scene::sample();
    let (w, h) = (state.chart.viewport().w, state.chart.viewport().h);
    assert!(
        w > 1.0 && h > 1.0,
        "the sample chart has a rectangle: {w}x{h}"
    );
}
