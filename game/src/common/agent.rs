use crate::common::route_explorer::RouteExplorer;
use crate::common::route_viewer::RouteViewer;
use crate::common::trip_explorer::TripExplorer;
use crate::common::ColorLegend;
use crate::game::{Transition, WizardState};
use crate::helpers::ID;
use crate::render::{AgentColorScheme, MIN_ZOOM_FOR_DETAIL};
use crate::ui::UI;
use ezgui::{hotkey, Choice, ContextMenu, EventCtx, GfxCtx, Key, ModalMenu};
use geom::{Duration, Pt2D};
use sim::{TripID, TripResult};
use std::cell::RefCell;

pub struct AgentTools {
    following: Option<(TripID, Option<Pt2D>, Duration)>,
    route_viewer: RouteViewer,
    // Weird to stash this here and lazily sync it, but...
    agent_cs_legend: RefCell<Option<(AgentColorScheme, ColorLegend)>>,
}

impl AgentTools {
    pub fn new() -> AgentTools {
        AgentTools {
            following: None,
            route_viewer: RouteViewer::Inactive,
            agent_cs_legend: RefCell::new(None),
        }
    }

    pub fn event(
        &mut self,
        ctx: &mut EventCtx,
        ui: &UI,
        parent_menu: &mut ModalMenu,
        ctx_menu: &mut ContextMenu<ID>,
    ) -> Option<Transition> {
        if self.following.is_none() {
            if let Some(agent) = ctx_menu.current_focus().and_then(|id| id.agent_id()) {
                if let Some(trip) = ui.primary.sim.agent_to_trip(agent) {
                    if ctx_menu.action(Key::F, format!("follow {}", agent), ctx) {
                        self.following = Some((
                            trip,
                            ui.primary
                                .sim
                                .get_canonical_pt_per_trip(trip, &ui.primary.map)
                                .ok(),
                            ui.primary.sim.time(),
                        ));
                        parent_menu.add_action(hotkey(Key::F), "stop following agent", ctx);
                    }
                }
            }
        }
        if let Some((trip, _, time)) = self.following {
            if ui.primary.sim.time() != time {
                match ui
                    .primary
                    .sim
                    .get_canonical_pt_per_trip(trip, &ui.primary.map)
                {
                    TripResult::Ok(pt) => {
                        ctx.canvas.center_on_map_pt(pt);
                        self.following = Some((trip, Some(pt), ui.primary.sim.time()));
                    }
                    TripResult::ModeChange => {
                        self.following = Some((trip, None, ui.primary.sim.time()));
                    }
                    TripResult::TripDone => {
                        println!("{} is done or aborted, so no more following", trip);
                        self.following = None;
                        parent_menu.remove_action("stop following agent", ctx);
                    }
                    TripResult::TripDoesntExist => {
                        println!("{} doesn't exist yet, so not following", trip);
                        self.following = None;
                        parent_menu.remove_action("stop following agent", ctx);
                    }
                }
            }
            if self.following.is_some() && parent_menu.consume_action("stop following agent", ctx) {
                self.following = None;
            }
        }
        self.route_viewer.event(ctx, ui, parent_menu, ctx_menu);

        if let Some(explorer) = RouteExplorer::new(ctx, ui, ctx_menu) {
            return Some(Transition::Push(Box::new(explorer)));
        }
        if let Some(explorer) = TripExplorer::new(ctx, ui, ctx_menu) {
            return Some(Transition::Push(Box::new(explorer)));
        }

        if parent_menu.action("change agent colorscheme") {
            return Some(Transition::Push(WizardState::new(Box::new(
                |wiz, ctx, ui| {
                    let (_, acs) = wiz.wrap(ctx).choose("Which colorscheme for agents?", || {
                        let mut choices = Vec::new();
                        for (acs, name) in AgentColorScheme::all() {
                            if ui.agent_cs != acs {
                                choices.push(Choice::new(name, acs));
                            }
                        }
                        choices
                    })?;
                    ui.agent_cs = acs;
                    ui.primary.draw_map.agents.borrow_mut().invalidate_cache();
                    if let Some(ref mut s) = ui.secondary {
                        s.draw_map.agents.borrow_mut().invalidate_cache();
                    }
                    Some(Transition::Pop)
                },
            ))));
        }

        None
    }

    pub fn draw(&self, g: &mut GfxCtx, ui: &UI) {
        self.route_viewer.draw(g, ui);

        if g.canvas.cam_zoom < MIN_ZOOM_FOR_DETAIL {
            let mut maybe_legend = self.agent_cs_legend.borrow_mut();
            if maybe_legend
                .as_ref()
                .map(|(acs, _)| *acs != ui.agent_cs)
                .unwrap_or(true)
            {
                *maybe_legend = Some((ui.agent_cs, ui.agent_cs.make_color_legend(&ui.cs)));
            }
            maybe_legend.as_ref().unwrap().1.draw(g);
        }
    }
}
