use crate::common::{CommonState, ContextMenu};
use crate::game::{State, Transition, WizardState};
use crate::helpers::ID;
use crate::ui::UI;
use ezgui::{Choice, EventCtx, GfxCtx, Key, ModalMenu, Text, WarpingItemSlider};
use geom::Pt2D;
use map_model::{BusRoute, BusRouteID, BusStopID, Map};

pub struct BusRouteExplorer {
    slider: WarpingItemSlider<BusStopID>,
}

impl BusRouteExplorer {
    pub fn new(ctx: &mut EventCtx, ui: &UI, ctx_menu: &mut ContextMenu) -> Option<Box<dyn State>> {
        let map = &ui.primary.map;
        let routes = match ctx_menu.current_focus() {
            Some(ID::BusStop(bs)) => map.get_routes_serving_stop(bs),
            _ => {
                return None;
            }
        };
        if routes.is_empty() {
            return None;
        }
        if !ctx_menu.action(Key::E, "explore bus route", ctx) {
            return None;
        }
        if routes.len() == 1 {
            Some(Box::new(BusRouteExplorer::for_route(
                routes[0],
                &ui.primary.map,
                ctx,
            )))
        } else {
            Some(make_bus_route_picker(
                routes.into_iter().map(|r| r.id).collect(),
            ))
        }
    }

    fn for_route(route: &BusRoute, map: &Map, ctx: &mut EventCtx) -> BusRouteExplorer {
        let stops: Vec<(Pt2D, BusStopID, Text)> = route
            .stops
            .iter()
            .map(|bs| {
                let stop = map.get_bs(*bs);
                (stop.sidewalk_pos.pt(map), stop.id, Text::new())
            })
            .collect();
        BusRouteExplorer {
            slider: WarpingItemSlider::new(
                stops,
                &format!("Bus Route Explorer for {}", route.name),
                "stop",
                ctx,
            ),
        }
    }
}

impl State for BusRouteExplorer {
    fn event(&mut self, ctx: &mut EventCtx, ui: &mut UI) -> Transition {
        if ctx.redo_mouseover() {
            // TODO Or use what debug mode is showing?
            ui.recalculate_current_selection(ctx);
        }
        ctx.canvas.handle_event(ctx.input);

        if let Some((evmode, done_warping)) = self.slider.event(ctx) {
            if done_warping {
                ui.primary.current_selection = Some(ID::BusStop(*self.slider.get().1));
            }
            Transition::KeepWithMode(evmode)
        } else {
            Transition::Pop
        }
    }

    fn draw(&self, g: &mut GfxCtx, ui: &UI) {
        self.slider.draw(g);
        CommonState::draw_osd(g, ui, &ui.primary.current_selection);
    }
}

pub struct BusRoutePicker;
impl BusRoutePicker {
    pub fn new(ui: &UI, menu: &mut ModalMenu) -> Option<Box<dyn State>> {
        if !menu.action("explore a bus route") {
            return None;
        }
        Some(make_bus_route_picker(
            ui.primary
                .map
                .get_all_bus_routes()
                .iter()
                .map(|r| r.id)
                .collect(),
        ))
    }
}

fn make_bus_route_picker(choices: Vec<BusRouteID>) -> Box<dyn State> {
    WizardState::new(Box::new(move |wiz, ctx, ui| {
        let (_, id) = wiz.wrap(ctx).choose("Explore which bus route?", || {
            choices
                .iter()
                .map(|id| Choice::new(&ui.primary.map.get_br(*id).name, *id))
                .collect()
        })?;
        Some(Transition::Replace(Box::new(BusRouteExplorer::for_route(
            ui.primary.map.get_br(id),
            &ui.primary.map,
            ctx,
        ))))
    }))
}
