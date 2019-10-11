use crate::game::{State, Transition};
use crate::helpers::ID;
use crate::render::{DrawCtx, DrawOptions, DrawTurn, TrafficSignalDiagram};
use crate::ui::{ShowEverything, UI};
use ezgui::{hotkey, Color, ContextMenu, EventCtx, GeomBatch, GfxCtx, Key, ModalMenu};
use map_model::{IntersectionID, LaneID, Map, TurnType};

pub enum TurnCyclerState {
    Inactive,
    ShowLane(LaneID),
    CycleTurns(LaneID, usize),
}

impl TurnCyclerState {
    pub fn event(
        &mut self,
        ctx: &mut EventCtx,
        ui: &mut UI,
        ctx_menu: &mut ContextMenu<ID>,
    ) -> Option<Transition> {
        match ctx_menu.current_focus() {
            Some(ID::Lane(id)) if !ui.primary.map.get_turns_from_lane(id).is_empty() => {
                if let TurnCyclerState::CycleTurns(current, idx) = self {
                    if *current != id {
                        *self = TurnCyclerState::ShowLane(id);
                    } else if ctx_menu.action(Key::Z, "cycle through this lane's turns", ctx) {
                        *self = TurnCyclerState::CycleTurns(id, *idx + 1);
                    }
                } else {
                    *self = TurnCyclerState::ShowLane(id);
                    if ctx_menu.action(Key::Z, "cycle through this lane's turns", ctx) {
                        *self = TurnCyclerState::CycleTurns(id, 0);
                    }
                }
            }
            Some(ID::Intersection(i)) => {
                if let Some(ref signal) = ui.primary.map.maybe_get_traffic_signal(i) {
                    if ctx_menu.action(Key::F, "show full traffic signal diagram", ctx) {
                        ui.primary.current_selection = None;
                        // TODO Also reset focus?
                        let (idx, _, _) =
                            signal.current_phase_and_remaining_time(ui.primary.sim.time());
                        return Some(Transition::Push(Box::new(ShowTrafficSignal {
                            menu: ModalMenu::new(
                                "Traffic Signal Diagram",
                                vec![
                                    vec![
                                        (hotkey(Key::UpArrow), "select previous cycle"),
                                        (hotkey(Key::DownArrow), "select next cycle"),
                                    ],
                                    vec![(hotkey(Key::Escape), "quit")],
                                ],
                                ctx,
                            ),
                            diagram: TrafficSignalDiagram::new(i, idx, &ui.primary.map, ctx),
                        })));
                    }
                }
                *self = TurnCyclerState::Inactive;
            }
            _ => {
                *self = TurnCyclerState::Inactive;
            }
        }

        None
    }

    pub fn draw(&self, g: &mut GfxCtx, ui: &UI) {
        match self {
            TurnCyclerState::Inactive => {}
            TurnCyclerState::ShowLane(l) => {
                for turn in &ui.primary.map.get_turns_from_lane(*l) {
                    DrawTurn::draw_full(turn, g, color_turn_type(turn.turn_type, ui).alpha(0.5));
                }
            }
            TurnCyclerState::CycleTurns(l, idx) => {
                let turns = ui.primary.map.get_turns_from_lane(*l);
                let current = turns[*idx % turns.len()];
                DrawTurn::draw_full(current, g, color_turn_type(current.turn_type, ui));

                let mut batch = GeomBatch::new();
                for t in ui.primary.map.get_turns_in_intersection(current.id.parent) {
                    if current.conflicts_with(t) {
                        DrawTurn::draw_dashed(
                            t,
                            &mut batch,
                            ui.cs.get_def("conflicting turn", Color::RED.alpha(0.8)),
                        );
                    }
                }
                batch.draw(g);
            }
        }
    }

    pub fn suppress_traffic_signal_details(&self, map: &Map) -> Option<IntersectionID> {
        match self {
            TurnCyclerState::ShowLane(l) | TurnCyclerState::CycleTurns(l, _) => {
                Some(map.get_l(*l).dst_i)
            }
            TurnCyclerState::Inactive => None,
        }
    }
}

fn color_turn_type(t: TurnType, ui: &UI) -> Color {
    match t {
        TurnType::SharedSidewalkCorner => {
            ui.cs.get_def("shared sidewalk corner turn", Color::BLACK)
        }
        TurnType::Crosswalk => ui.cs.get_def("crosswalk turn", Color::WHITE),
        TurnType::Straight => ui.cs.get_def("straight turn", Color::BLUE),
        TurnType::LaneChangeLeft => ui.cs.get_def("change lanes left turn", Color::CYAN),
        TurnType::LaneChangeRight => ui.cs.get_def("change lanes right turn", Color::PURPLE),
        TurnType::Right => ui.cs.get_def("right turn", Color::GREEN),
        TurnType::Left => ui.cs.get_def("left turn", Color::RED),
    }
}

struct ShowTrafficSignal {
    menu: ModalMenu,
    diagram: TrafficSignalDiagram,
}

impl State for ShowTrafficSignal {
    fn event(&mut self, ctx: &mut EventCtx, _: &mut UI) -> Transition {
        self.menu.handle_event(ctx, None);
        ctx.canvas.handle_event(ctx.input);
        if self.menu.action("quit") {
            return Transition::Pop;
        }
        self.diagram.event(ctx, &mut self.menu);
        Transition::Keep
    }

    fn draw(&self, g: &mut GfxCtx, ui: &UI) {
        ui.draw(
            g,
            DrawOptions::new(),
            &ui.primary.sim,
            &ShowEverything::new(),
        );
        let ctx = DrawCtx {
            cs: &ui.cs,
            map: &ui.primary.map,
            draw_map: &ui.primary.draw_map,
            sim: &ui.primary.sim,
        };
        self.diagram.draw(g, &ctx);

        self.menu.draw(g);
    }
}
