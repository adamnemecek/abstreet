use crate::common::TripExplorer;
use crate::game::{msg, State, Transition, WizardState};
use crate::sandbox::gameplay::{cmp_count_fewer, cmp_count_more, cmp_duration_shorter};
use crate::ui::UI;
use abstutil::prettyprint_usize;
use ezgui::{
    hotkey, Choice, Color, EventCtx, GfxCtx, HorizontalAlignment, Key, Line, ModalMenu, Text,
    VerticalAlignment, Wizard,
};
use geom::{Duration, Statistic, Time};
use sim::{Analytics, TripID, TripMode};
use std::collections::BTreeSet;

pub struct Scoreboard {
    menu: ModalMenu,
    summary: Text,
}

impl Scoreboard {
    pub fn new(ctx: &mut EventCtx, ui: &UI, prebaked: &Analytics) -> Scoreboard {
        let menu = ModalMenu::new(
            "Finished trips summary",
            vec![
                (hotkey(Key::Escape), "quit"),
                (hotkey(Key::B), "browse trips"),
                (hotkey(Key::P), "examine parking overhead"),
            ],
            ctx,
        );

        let (now_all, now_aborted, now_per_mode) = ui
            .primary
            .sim
            .get_analytics()
            .all_finished_trips(ui.primary.sim.time());
        let (baseline_all, baseline_aborted, baseline_per_mode) =
            prebaked.all_finished_trips(ui.primary.sim.time());

        // TODO Include unfinished count
        let mut txt = Text::new();
        txt.add_appended(vec![
            Line("Finished trips as of "),
            Line(ui.primary.sim.time().ampm_tostring()).fg(Color::CYAN),
        ]);
        txt.add_appended(vec![
            Line(format!(
                "  {} aborted trips (",
                prettyprint_usize(now_aborted)
            )),
            cmp_count_fewer(now_aborted, baseline_aborted),
            Line(")"),
        ]);
        // TODO Refactor
        txt.add_appended(vec![
            Line(format!(
                "{} total finished trips (",
                prettyprint_usize(now_all.count())
            )),
            cmp_count_more(now_all.count(), baseline_all.count()),
            Line(")"),
        ]);
        if now_all.count() > 0 && baseline_all.count() > 0 {
            for stat in Statistic::all() {
                txt.add(Line(format!("  {}: {} ", stat, now_all.select(stat))));
                txt.append_all(cmp_duration_shorter(
                    now_all.select(stat),
                    baseline_all.select(stat),
                ));
            }
        }

        for mode in TripMode::all() {
            let a = &now_per_mode[&mode];
            let b = &baseline_per_mode[&mode];
            txt.add_appended(vec![
                Line(format!("{} {} trips (", prettyprint_usize(a.count()), mode)),
                cmp_count_more(a.count(), b.count()),
                Line(")"),
            ]);
            if a.count() > 0 && b.count() > 0 {
                for stat in Statistic::all() {
                    txt.add(Line(format!("  {}: {} ", stat, a.select(stat))));
                    txt.append_all(cmp_duration_shorter(a.select(stat), b.select(stat)));
                }
            }
        }

        Scoreboard { menu, summary: txt }
    }
}

impl State for Scoreboard {
    fn event(&mut self, ctx: &mut EventCtx, ui: &mut UI) -> Transition {
        self.menu.event(ctx);
        if self.menu.action("quit") {
            return Transition::Pop;
        }
        if self.menu.action("browse trips") {
            return Transition::Push(WizardState::new(Box::new(browse_trips)));
        }
        if self.menu.action("examine parking overhead") {
            // TODO Need word wrapping here
            return Transition::Push(msg(
                "Parking overhead",
                ui.primary.sim.get_analytics().analyze_parking_phases(),
            ));
        }
        Transition::Keep
    }

    fn draw(&self, g: &mut GfxCtx, _: &UI) {
        g.draw_blocking_text(
            &self.summary,
            (HorizontalAlignment::Center, VerticalAlignment::Center),
        );
        self.menu.draw(g);
    }
}

fn browse_trips(wiz: &mut Wizard, ctx: &mut EventCtx, ui: &mut UI) -> Option<Transition> {
    let mut wizard = wiz.wrap(ctx);
    let (_, mode) = wizard.choose("Browse which trips?", || {
        let modes = ui
            .primary
            .sim
            .get_analytics()
            .finished_trips
            .iter()
            .filter_map(|(_, _, m, _)| *m)
            .collect::<BTreeSet<TripMode>>();
        TripMode::all()
            .into_iter()
            .map(|m| Choice::new(m.to_string(), m).active(modes.contains(&m)))
            .collect()
    })?;
    let (_, trip) = wizard.choose("Examine which trip?", || {
        let mut filtered: Vec<&(Time, TripID, Option<TripMode>, Duration)> = ui
            .primary
            .sim
            .get_analytics()
            .finished_trips
            .iter()
            .filter(|(_, _, m, _)| *m == Some(mode))
            .collect();
        filtered.sort_by_key(|(_, _, _, dt)| *dt);
        filtered.reverse();
        filtered
            .into_iter()
            // TODO Show percentile for time
            .map(|(_, id, _, dt)| Choice::new(format!("{} taking {}", id, dt), *id))
            .collect()
    })?;

    wizard.reset();
    Some(Transition::Push(Box::new(TripExplorer::new(trip, ctx, ui))))
}
