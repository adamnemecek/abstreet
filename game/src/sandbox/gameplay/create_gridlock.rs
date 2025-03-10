use crate::game::{msg, Transition};
use crate::render::AgentColorScheme;
use crate::sandbox::gameplay::{cmp_count_fewer, manage_acs, GameplayState};
use crate::sandbox::overlays::Overlays;
use crate::ui::UI;
use abstutil::prettyprint_usize;
use ezgui::{hotkey, EventCtx, Key, Line, ModalMenu, Text};
use geom::Time;
use sim::{Analytics, TripMode};

pub struct CreateGridlock {
    time: Time,
}

impl CreateGridlock {
    pub fn new(ctx: &EventCtx) -> (ModalMenu, Box<dyn GameplayState>) {
        (
            ModalMenu::new(
                "Cause gridlock",
                vec![
                    (hotkey(Key::E), "show agent delay"),
                    (hotkey(Key::H), "help"),
                ],
                ctx,
            ),
            Box::new(CreateGridlock {
                time: Time::START_OF_DAY,
            }),
        )
    }
}

impl GameplayState for CreateGridlock {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        ui: &mut UI,
        _: &mut Overlays,
        prebaked: &Analytics,
        menu: &mut ModalMenu,
    ) -> Option<Transition> {
        menu.event(ctx);
        manage_acs(
            menu,
            ctx,
            ui,
            "show agent delay",
            "hide agent delay",
            AgentColorScheme::Delay,
        );

        if self.time != ui.primary.sim.time() {
            self.time = ui.primary.sim.time();
            menu.set_info(ctx, gridlock_panel(ui, prebaked));
        }

        if menu.action("help") {
            return Some(Transition::Push(msg("Help", vec![
                        "You might notice a few places in the map where gridlock forms already.",
                        "You can make things worse!",
                        "How few lanes can you close for construction before everything grinds to a halt?",
                    ])));
        }
        None
    }
}

fn gridlock_panel(ui: &UI, prebaked: &Analytics) -> Text {
    let (now_all, _, now_per_mode) = ui
        .primary
        .sim
        .get_analytics()
        .all_finished_trips(ui.primary.sim.time());
    let (baseline_all, _, baseline_per_mode) = prebaked.all_finished_trips(ui.primary.sim.time());

    let mut txt = Text::new();
    txt.add_appended(vec![
        Line(format!(
            "{} total finished trips (",
            prettyprint_usize(now_all.count())
        )),
        cmp_count_fewer(now_all.count(), baseline_all.count()),
        Line(")"),
    ]);

    for mode in TripMode::all() {
        let a = now_per_mode[&mode].count();
        let b = baseline_per_mode[&mode].count();
        txt.add_appended(vec![
            Line(format!("  {}: {} (", mode, prettyprint_usize(a))),
            cmp_count_fewer(a, b),
            Line(")"),
        ]);
    }

    txt
}
