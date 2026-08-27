//! The Kernels workspace driven through its visible cards and canvas.

use cellarium::gui::canvas::kernel::{CellState, KernelEdit, KernelTool};
use cellarium::gui::{CellariumGui, Section, layout};
use cellarium::sim::experiment_model::{ExperimentSpec, KernelId};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

type Gui = Harness<'static, CellariumGui>;

fn one_kernel_gui() -> Gui {
    let spec = ExperimentSpec::single_channel_lenia(16, 16)
        .normalize_rules()
        .expect("the fixture normalizes");
    let mut app = CellariumGui::for_test(spec);
    app.navigation_mut().select(Section::Kernels);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 720.0))
        .build_ui_state(|ui, app: &mut CellariumGui| layout::draw(app, ui), app);
    harness.run();
    harness
}

fn click(gui: &mut Gui, label: &str) {
    gui.get_by_label(label).click();
    gui.run();
}

fn symbol_of(gui: &Gui, kernel: KernelId) -> String {
    gui.state()
        .kernel_cards()
        .into_iter()
        .find(|card| card.id == kernel)
        .map(|card| card.symbol)
        .unwrap_or_else(|| panic!("kernel {kernel:?} has no card"))
}

fn click_kernel_card(gui: &mut Gui, kernel: KernelId) {
    let symbol = symbol_of(gui, kernel);
    gui.get_by_label(symbol.as_str()).click();
    gui.run();
}

fn delete_kernel_card(gui: &mut Gui, kernel: KernelId) {
    let symbol = symbol_of(gui, kernel);
    gui.get_by_label(format!("Delete {symbol}").as_str())
        .click();
    gui.run();
}

/// Paint a value the way the canvas does, through the state the pointer drives.
fn paint_distinct_value(gui: &mut Gui, value: f32) {
    gui.state_mut().kernel_canvas_mut().tool = KernelTool::Weights;
    gui.state_mut().kernel_canvas_mut().paint_value = value;
    gui.state_mut()
        .apply_kernel_edit(KernelEdit::Weight { x: 0, y: 0, value });
    gui.run();
}

fn weight_at(gui: &Gui, x: usize, y: usize) -> f32 {
    gui.state().kernel_stencil().weight(x, y)
}

#[test]
fn four_kernels_can_be_added_switched_edited_and_deleted_by_mouse() {
    let mut gui = one_kernel_gui();
    for _ in 0..3 {
        click(&mut gui, "Add kernel");
    }
    assert_eq!(gui.state().kernel_cards().len(), 4);

    // Nonsequential order on purpose: an editor that only works when kernels
    // are visited in creation order is an editor with hidden state.
    let ids: Vec<KernelId> = gui
        .state()
        .kernel_cards()
        .into_iter()
        .map(|card| card.id)
        .collect();
    let order = [ids[3], ids[0], ids[2], ids[1]];
    for (index, id) in order.iter().enumerate() {
        click_kernel_card(&mut gui, *id);
        assert_eq!(gui.state().selected_kernel(), Some(*id));
        let value = (index as f32 + 1.0) / 10.0;
        paint_distinct_value(&mut gui, value);
        assert!(
            (weight_at(&gui, 0, 0) - value).abs() < 1e-6,
            "kernel {id:?} did not take the value painted into it"
        );
    }

    // Every kernel kept its own value; none of them share one grid.
    for (index, id) in order.iter().enumerate() {
        click_kernel_card(&mut gui, *id);
        let expected = (index as f32 + 1.0) / 10.0;
        assert!(
            (weight_at(&gui, 0, 0) - expected).abs() < 1e-6,
            "kernel {id:?} lost the value it was given"
        );
    }

    delete_kernel_card(&mut gui, ids[1]);
    assert_eq!(gui.state().kernel_cards().len(), 3);
    assert!(
        !gui.state()
            .kernel_cards()
            .iter()
            .any(|card| card.id == ids[1]),
        "the deleted kernel must be gone from the strip"
    );
}

#[test]
fn a_new_kernel_is_selected_immediately_and_carries_its_own_ordinal() {
    let mut gui = one_kernel_gui();
    click(&mut gui, "Add kernel");
    let cards = gui.state().kernel_cards();
    let last = cards.last().expect("a kernel was added");
    assert_eq!(gui.state().selected_kernel(), Some(last.id));
    for (index, card) in cards.iter().enumerate() {
        assert_eq!(card.ordinal, index + 1);
    }
}

#[test]
fn the_last_kernel_of_a_binding_cannot_be_deleted() {
    let gui = one_kernel_gui();
    let only = gui.state().kernel_cards()[0].symbol.clone();
    assert!(
        gui.get_by_label(format!("Delete {only}").as_str())
            .accesskit_node()
            .is_disabled(),
        "a binding must keep at least one kernel"
    );
}

#[test]
fn support_and_weights_are_separate_tools_with_distinct_results() {
    let mut gui = one_kernel_gui();
    click(&mut gui, "Add kernel");

    gui.state_mut().kernel_canvas_mut().tool = KernelTool::Weights;
    gui.state_mut().apply_kernel_edit(KernelEdit::Weight {
        x: 1,
        y: 1,
        value: 0.5,
    });
    gui.run();
    assert_eq!(
        gui.state().kernel_stencil().state(1, 1),
        CellState::Positive
    );

    // Switching a cell off leaves its weight in place but stops it counting.
    gui.state_mut().kernel_canvas_mut().tool = KernelTool::Support;
    gui.state_mut().apply_kernel_edit(KernelEdit::Active {
        x: 1,
        y: 1,
        active: false,
    });
    gui.run();
    assert_eq!(
        gui.state().kernel_stencil().state(1, 1),
        CellState::Inactive
    );
    assert!(
        (weight_at(&gui, 1, 1) - 0.5).abs() < 1e-6,
        "support must not discard the weight"
    );

    gui.state_mut().apply_kernel_edit(KernelEdit::Active {
        x: 1,
        y: 1,
        active: true,
    });
    gui.run();
    assert_eq!(
        gui.state().kernel_stencil().state(1, 1),
        CellState::Positive
    );
}

#[test]
fn deleting_a_referenced_kernel_asks_first_and_cancelling_changes_nothing() {
    let mut gui = one_kernel_gui();
    click(&mut gui, "Add kernel");
    let added = gui.state().selected_kernel().expect("the new kernel");
    let symbol = symbol_of(&gui, added);

    // Make the growth program depend on the new kernel.
    gui.state_mut().set_growth_source(format!("{symbol} * 2.0"));
    gui.run();
    let before = gui.state().document().audit_snapshot();

    delete_kernel_card(&mut gui, added);
    // Nothing has happened yet: the dialog is asking.
    assert!(gui.state().kernel_decision().is_some());
    assert_eq!(gui.state().document().audit_snapshot(), before);
    // The exact rewrite is on screen before the choice is made.
    gui.get_by_label(format!("before: {symbol} * 2.0").as_str());
    gui.get_by_label("after:  0.0 * 2.0");

    click(&mut gui, "Cancel");
    assert!(gui.state().kernel_decision().is_none());
    assert_eq!(
        gui.state().document().audit_snapshot(),
        before,
        "cancelling a deletion must leave the draft untouched"
    );
    assert!(
        gui.state()
            .kernel_cards()
            .iter()
            .any(|card| card.id == added)
    );
}

#[test]
fn confirming_a_referenced_deletion_removes_the_kernel_and_rewrites_the_source() {
    let mut gui = one_kernel_gui();
    click(&mut gui, "Add kernel");
    let added = gui.state().selected_kernel().expect("the new kernel");
    let symbol = symbol_of(&gui, added);
    gui.state_mut().set_growth_source(format!("{symbol} * 2.0"));
    gui.run();

    delete_kernel_card(&mut gui, added);
    click(&mut gui, "Replace references with 0 and remove");

    assert!(gui.state().kernel_decision().is_none());
    assert!(
        !gui.state()
            .kernel_cards()
            .iter()
            .any(|card| card.id == added),
        "the kernel must be gone"
    );
    assert_eq!(
        gui.state().growth_source(),
        "0.0 * 2.0",
        "the rewrite the dialog showed is the rewrite that happened"
    );
}

#[test]
fn the_wheel_reaches_an_exact_value_without_a_keyboard() {
    let mut gui = one_kernel_gui();
    gui.state_mut().kernel_canvas_mut().paint_value = 0.0;
    // Twenty coarse steps, then two fine ones back: 1.0 - 0.01.
    for _ in 0..20 {
        gui.state_mut()
            .kernel_canvas_mut()
            .adjust_paint_value(1.0, egui::Modifiers::NONE);
    }
    for _ in 0..2 {
        gui.state_mut()
            .kernel_canvas_mut()
            .adjust_paint_value(-1.0, egui::Modifiers::SHIFT);
    }
    let value = gui.state().kernel_canvas().paint_value;
    assert!((value - 0.99).abs() < 1e-5, "reached {value}");
}
