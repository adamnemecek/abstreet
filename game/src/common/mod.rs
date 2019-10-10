mod agent;
mod associated;
mod colors;
mod navigate;
mod route_explorer;
mod route_viewer;
mod shortcuts;
mod sidebar;
mod speed;
mod time;
mod trip_explorer;
mod turn_cycler;
mod warp;

pub use self::agent::AgentTools;
pub use self::colors::{
    ColorLegend, ObjectColorer, ObjectColorerBuilder, RoadColorer, RoadColorerBuilder,
};
pub use self::route_explorer::RouteExplorer;
pub use self::sidebar::ContextMenu;
pub use self::speed::SpeedControls;
pub use self::time::time_controls;
pub use self::trip_explorer::TripExplorer;
pub use self::warp::Warping;
use crate::game::Transition;
use crate::helpers::ID;
use crate::render::DrawOptions;
use crate::ui::UI;
use ezgui::{
    Color, EventCtx, EventLoopMode, GfxCtx, HorizontalAlignment, Line, ModalMenu, Text,
    VerticalAlignment,
};
use std::collections::BTreeSet;

pub struct CommonState {
    associated: associated::ShowAssociatedState,
    turn_cycler: turn_cycler::TurnCyclerState,
}

impl CommonState {
    pub fn new() -> CommonState {
        CommonState {
            associated: associated::ShowAssociatedState::Inactive,
            turn_cycler: turn_cycler::TurnCyclerState::Inactive,
        }
    }

    pub fn event(
        &mut self,
        ctx: &mut EventCtx,
        ui: &mut UI,
        menu: &mut ModalMenu,
    ) -> Option<Transition> {
        if menu.action("warp") {
            return Some(Transition::Push(warp::EnteringWarp::new()));
        }
        if menu.action("navigate") {
            return Some(Transition::Push(Box::new(navigate::Navigator::new(ui))));
        }
        if menu.action("shortcuts") {
            return Some(Transition::Push(shortcuts::ChoosingShortcut::new()));
        }

        self.associated.event(ui);
        if let Some(t) = self.turn_cycler.event(ctx, ui) {
            return Some(t);
        }
        if menu.action("take a screenshot") {
            return Some(Transition::KeepWithMode(
                EventLoopMode::ScreenCaptureCurrentShot,
            ));
        }

        None
    }

    pub fn draw(&self, g: &mut GfxCtx, ui: &UI) {
        self.turn_cycler.draw(g, ui);

        CommonState::draw_osd(g, ui, &ui.primary.current_selection);
    }

    pub fn default_osd(id: ID, ui: &UI) -> Text {
        let map = &ui.primary.map;
        let id_color = ui.cs.get_def("OSD ID color", Color::RED);
        let name_color = ui.cs.get_def("OSD name color", Color::CYAN);
        let mut osd = Text::new();
        match id {
            ID::Lane(l) => {
                osd.append_all(vec![
                    Line(l.to_string()).fg(id_color),
                    Line(" is "),
                    Line(map.get_parent(l).get_name()).fg(name_color),
                ]);
            }
            ID::Building(b) => {
                let bldg = map.get_b(b);
                osd.append_all(vec![
                    Line(b.to_string()).fg(id_color),
                    Line(" is "),
                    Line(bldg.get_name()).fg(name_color),
                ]);
                if let Some(ref p) = bldg.parking {
                    osd.append(Line(format!(
                        " ({} parking spots via {})",
                        p.num_stalls, p.name
                    )));
                }
            }
            ID::Turn(t) => {
                osd.append_all(vec![
                    Line(format!("TurnID({})", map.get_t(t).lookup_idx)).fg(id_color),
                    Line(" between "),
                    Line(map.get_parent(t.src).get_name()).fg(name_color),
                    Line(" and "),
                    Line(map.get_parent(t.dst).get_name()).fg(name_color),
                ]);
            }
            ID::Intersection(i) => {
                osd.append_all(vec![Line(i.to_string()).fg(id_color), Line(" of ")]);

                let mut road_names = BTreeSet::new();
                for r in &map.get_i(i).roads {
                    road_names.insert(map.get_r(*r).get_name());
                }
                let len = road_names.len();
                for (idx, n) in road_names.into_iter().enumerate() {
                    osd.append(Line(n).fg(name_color));
                    if idx != len - 1 {
                        osd.append(Line(", "));
                    }
                }
            }
            ID::Car(c) => {
                osd.append(Line(c.to_string()).fg(id_color));
                if let Some(r) = ui.primary.sim.bus_route_id(c) {
                    osd.append_all(vec![
                        Line(" serving "),
                        Line(&map.get_br(r).name).fg(name_color),
                    ]);
                }
            }
            ID::BusStop(bs) => {
                osd.append_all(vec![Line(bs.to_string()).fg(id_color), Line(" serving ")]);

                let routes = map.get_routes_serving_stop(bs);
                let len = routes.len();
                for (idx, n) in routes.into_iter().enumerate() {
                    osd.append(Line(&n.name).fg(name_color));
                    if idx != len - 1 {
                        osd.append(Line(", "));
                    }
                }
            }
            _ => {
                osd.append(Line(format!("{:?}", id)).fg(id_color));
            }
        }
        osd
    }

    pub fn draw_osd(g: &mut GfxCtx, ui: &UI, id: &Option<ID>) {
        let osd = if let Some(id) = id {
            CommonState::default_osd(id.clone(), ui)
        } else {
            Text::from(Line("..."))
        };
        CommonState::draw_custom_osd(g, osd);
    }

    pub fn draw_custom_osd(g: &mut GfxCtx, mut osd: Text) {
        let keys = g.get_active_context_menu_keys();
        if !keys.is_empty() {
            osd.append(Line("   Hotkeys: "));
            for (idx, key) in keys.into_iter().enumerate() {
                if idx != 0 {
                    osd.append(Line(", "));
                }
                osd.append(Line(key.describe()).fg(ezgui::HOTKEY_COLOR));
            }
        }

        g.draw_blocking_text(
            &osd,
            (HorizontalAlignment::FillScreen, VerticalAlignment::Bottom),
        );
    }

    pub fn draw_options(&self, ui: &UI) -> DrawOptions {
        let mut opts = DrawOptions::new();
        self.associated
            .override_colors(&mut opts.override_colors, ui);
        opts.suppress_traffic_signal_details = self
            .turn_cycler
            .suppress_traffic_signal_details(&ui.primary.map);
        opts
    }
}
