mod model;

use abstutil::CmdArgs;
use ezgui::{
    Choice, Color, ContextMenu, Drawable, EventCtx, EventLoopMode, GeomBatch, GfxCtx, Key, Line,
    SidebarPos, Text, Wizard, GUI,
};
use geom::{Distance, Line, Polygon, Pt2D};
use map_model::raw::{RestrictionType, StableBuildingID, StableIntersectionID, StableRoadID};
use map_model::{osm, LANE_THICKNESS};
use model::{Direction, Model, ID};
use std::process;

struct UI {
    model: Model,
    state: State,
    osd: Text,
    ctx_menu: ContextMenu<ID>,
}

enum State {
    Viewing,
    MovingIntersection(StableIntersectionID),
    MovingBuilding(StableBuildingID),
    MovingRoadPoint(StableRoadID, usize),
    LabelingBuilding(StableBuildingID, Wizard),
    LabelingRoad((StableRoadID, Direction), Wizard),
    LabelingIntersection(StableIntersectionID, Wizard),
    CreatingRoad(StableIntersectionID),
    EditingLanes(StableRoadID, Wizard),
    EditingRoadAttribs(StableRoadID, Wizard),
    SavingModel(Wizard),
    // bool is if key is down
    SelectingRectangle(Pt2D, Pt2D, bool),
    CreatingTurnRestrictionPt1(StableRoadID),
    CreatingTurnRestrictionPt2(StableRoadID, StableRoadID, Wizard),
    PreviewIntersection(Drawable, Vec<(Text, Pt2D)>, bool),
    EnteringWarp(Wizard),
    StampingRoads(String, String, String),
}

impl UI {
    fn new(ctx: &EventCtx) -> UI {
        let mut args = CmdArgs::new();
        let load = args.optional_free();
        let include_bldgs = args.enabled("--bldgs");
        let edit_fixes = args.optional("--fixes");
        args.done();

        let model = if let Some(path) = load {
            Model::import(&path, include_bldgs, edit_fixes, ctx.prerender)
        } else {
            Model::blank()
        };
        UI {
            model,
            state: State::Viewing,
            osd: Text::new(),
            ctx_menu: ContextMenu::new("Objects", ctx, SidebarPos::Left),
        }
    }
}

impl GUI for UI {
    fn event(&mut self, ctx: &mut EventCtx) -> EventLoopMode {
        ctx.canvas.handle_event(ctx.input);
        if ctx.redo_mouseover() {
            self.model.world.handle_mouseover(ctx);
        }
        if self.ctx_menu.event(ctx, self.model.world.get_selection()) {
            if let Some(ID::Lane(id, _, _)) = self.model.world.get_selection() {
                let mut txt = Text::new();
                for (k, v) in &self.model.map.roads[&id].osm_tags {
                    txt.add_appended(vec![
                        Line(k).fg(Color::RED),
                        Line(" = "),
                        Line(v).fg(Color::CYAN),
                    ]);
                }
                for (restriction, dst) in &self.model.map.roads[&id].turn_restrictions {
                    txt.add_appended(vec![
                        Line("Restriction: "),
                        Line(format!("{:?}", restriction)).fg(Color::RED),
                        Line(" to "),
                        Line(format!("way {}", dst)).fg(Color::CYAN),
                    ]);
                }
                self.ctx_menu.set_obj_info(txt);
            } else if let Some(ID::Intersection(i)) = self.model.world.get_selection() {
                let mut txt = Text::new();
                txt.add(Line(format!(
                    "{} is {:?}",
                    i, self.model.map.intersections[&i].orig_id
                )));
                for r in self.model.map.roads_per_intersection(i) {
                    txt.add(Line(format!("- {}", r)));
                }
                self.ctx_menu.set_obj_info(txt);
            }
        }

        match self.state {
            State::Viewing => {
                let cursor = ctx.canvas.get_cursor_in_map_space();
                match self.model.world.get_selection() {
                    Some(ID::Intersection(i)) => {
                        if ctx.input.key_pressed(Key::LeftControl, "move intersection") {
                            self.state = State::MovingIntersection(i);
                        } else if ctx.input.key_pressed(Key::R, "create road") {
                            self.state = State::CreatingRoad(i);
                        } else if ctx
                            .input
                            .key_pressed(Key::Backspace, &format!("delete {}", i))
                        {
                            self.model.delete_i(i);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::T, "toggle intersection type") {
                            self.model.toggle_i_type(i, ctx.prerender);
                        } else if ctx.input.key_pressed(Key::L, "label intersection") {
                            self.state = State::LabelingIntersection(i, Wizard::new());
                        } else if ctx
                            .input
                            .key_pressed(Key::P, "preview intersection geometry")
                        {
                            let (draw, labels) = preview_intersection(i, &self.model, ctx);
                            self.state = State::PreviewIntersection(draw, labels, false);
                        }
                    }
                    Some(ID::Building(b)) => {
                        if ctx.input.key_pressed(Key::LeftControl, "move building") {
                            self.state = State::MovingBuilding(b);
                        } else if ctx
                            .input
                            .key_pressed(Key::Backspace, &format!("delete {}", b))
                        {
                            self.model.delete_b(b);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::L, "label building") {
                            self.state = State::LabelingBuilding(b, Wizard::new());
                        }
                    }
                    Some(ID::Lane(r, dir, _)) => {
                        if ctx
                            .input
                            .key_pressed(Key::Backspace, &format!("delete {}", r))
                        {
                            self.model.delete_r(r);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::E, "edit lanes") {
                            self.state = State::EditingLanes(r, Wizard::new());
                        } else if ctx.input.key_pressed(Key::N, "edit name/speed") {
                            self.state = State::EditingRoadAttribs(r, Wizard::new());
                        } else if ctx.input.key_pressed(Key::S, "swap lanes") {
                            self.model.swap_lanes(r, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::L, "label side of the road") {
                            self.state = State::LabelingRoad((r, dir), Wizard::new());
                        } else if ctx.input.key_pressed(Key::P, "move road points") {
                            if self.model.showing_pts.is_some() {
                                self.model.stop_showing_pts();
                            }
                            self.model.show_r_points(r, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::M, "merge road") {
                            self.model.merge_r(r, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx
                            .input
                            .key_pressed(Key::R, "create turn restriction from here")
                        {
                            self.state = State::CreatingTurnRestrictionPt1(r);
                        } else if ctx
                            .input
                            .key_pressed(Key::C, "copy road name and speed to other roads")
                        {
                            let road = &self.model.map.roads[&r];
                            self.state = State::StampingRoads(
                                road.get_spec().to_string(),
                                road.osm_tags
                                    .get(osm::NAME)
                                    .cloned()
                                    .unwrap_or_else(String::new),
                                road.osm_tags
                                    .get(osm::MAXSPEED)
                                    .cloned()
                                    .unwrap_or_else(String::new),
                            );
                        }
                    }
                    Some(ID::RoadPoint(r, idx)) => {
                        if ctx.input.key_pressed(Key::LeftControl, "move point") {
                            self.state = State::MovingRoadPoint(r, idx);
                        } else if ctx.input.key_pressed(Key::Backspace, "delete point") {
                            self.model.delete_r_pt(r, idx, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        }
                    }
                    Some(ID::TurnRestriction(from, to, idx)) => {
                        if ctx
                            .input
                            .key_pressed(Key::Backspace, "delete turn restriction")
                        {
                            self.model.delete_tr(from, to, idx, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        }
                    }
                    None => {
                        if ctx.input.unimportant_key_pressed(Key::Escape, "quit") {
                            process::exit(0);
                        } else if ctx.input.key_pressed(Key::S, "save") {
                            if self.model.map.name != "" {
                                self.model.export();
                            } else {
                                self.state = State::SavingModel(Wizard::new());
                            }
                        } else if ctx.input.key_pressed(Key::F, "save map fixes") {
                            self.model.save_fixes();
                        } else if ctx.input.key_pressed(Key::I, "create intersection") {
                            if let Some(pt) = cursor {
                                self.model.create_i(pt, ctx.prerender);
                                self.model.world.handle_mouseover(ctx);
                            }
                        // TODO Silly bug: Mouseover doesn't actually work! I think the cursor being
                        // dead-center messes up the precomputed triangles.
                        } else if ctx.input.key_pressed(Key::B, "create building") {
                            if let Some(pt) = cursor {
                                self.model.create_b(pt, ctx.prerender);
                                self.model.world.handle_mouseover(ctx);
                            }
                        } else if ctx.input.key_pressed(Key::LeftShift, "select area") {
                            if let Some(pt) = cursor {
                                self.state = State::SelectingRectangle(pt, pt, true);
                            }
                        } else if self.model.showing_pts.is_some()
                            && ctx.input.key_pressed(Key::P, "stop moving road points")
                        {
                            self.model.stop_showing_pts();
                        } else if ctx.input.key_pressed(Key::J, "warp to something") {
                            self.state = State::EnteringWarp(Wizard::new());
                        }
                    }
                }
            }
            State::MovingIntersection(id) => {
                if let Some(cursor) = ctx.canvas.get_cursor_in_map_space() {
                    self.model.move_i(id, cursor, ctx.prerender);
                    if ctx.input.key_released(Key::LeftControl) {
                        self.state = State::Viewing;
                    }
                }
            }
            State::MovingBuilding(id) => {
                if let Some(cursor) = ctx.canvas.get_cursor_in_map_space() {
                    self.model.move_b(id, cursor, ctx.prerender);
                    if ctx.input.key_released(Key::LeftControl) {
                        self.state = State::Viewing;
                    }
                }
            }
            State::MovingRoadPoint(r, idx) => {
                if let Some(cursor) = ctx.canvas.get_cursor_in_map_space() {
                    self.model.move_r_pt(r, idx, cursor, ctx.prerender);
                    if ctx.input.key_released(Key::LeftControl) {
                        self.state = State::Viewing;
                    }
                }
            }
            State::LabelingBuilding(id, ref mut wizard) => {
                if let Some(label) = wizard.wrap(ctx).input_string_prefilled(
                    "Label the building",
                    self.model.map.buildings[&id]
                        .osm_tags
                        .get(osm::LABEL)
                        .cloned()
                        .unwrap_or_else(String::new),
                ) {
                    self.model.set_b_label(id, label, ctx.prerender);
                    self.state = State::Viewing;
                } else if wizard.aborted() {
                    self.state = State::Viewing;
                }
            }
            State::LabelingRoad(pair, ref mut wizard) => {
                if let Some(label) = wizard.wrap(ctx).input_string_prefilled(
                    "Label this side of the road",
                    self.model.get_r_label(pair).unwrap_or_else(String::new),
                ) {
                    self.model.set_r_label(pair, label, ctx.prerender);
                    self.state = State::Viewing;
                } else if wizard.aborted() {
                    self.state = State::Viewing;
                }
            }
            State::LabelingIntersection(id, ref mut wizard) => {
                if let Some(label) = wizard.wrap(ctx).input_string_prefilled(
                    "Label the intersection",
                    self.model.map.intersections[&id]
                        .label
                        .clone()
                        .unwrap_or_else(String::new),
                ) {
                    self.model.set_i_label(id, label, ctx.prerender);
                    self.state = State::Viewing;
                } else if wizard.aborted() {
                    self.state = State::Viewing;
                }
            }
            State::CreatingRoad(i1) => {
                if ctx.input.key_pressed(Key::Escape, "stop defining road") {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                } else if let Some(ID::Intersection(i2)) = self.model.world.get_selection() {
                    if i1 != i2 && ctx.input.key_pressed(Key::R, "finalize road") {
                        self.model.create_r(i1, i2, ctx.prerender);
                        self.state = State::Viewing;
                        self.model.world.handle_mouseover(ctx);
                    }
                }
            }
            State::EditingLanes(id, ref mut wizard) => {
                if let Some(s) = wizard.wrap(ctx).input_string_prefilled(
                    "Specify the lanes",
                    self.model.map.roads[&id].get_spec().to_string(),
                ) {
                    self.model.edit_lanes(id, s, ctx.prerender);
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                } else if wizard.aborted() {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::EditingRoadAttribs(id, ref mut wizard) => {
                let (orig_name, orig_speed) = {
                    let r = &self.model.map.roads[&id];
                    (
                        r.osm_tags
                            .get(osm::NAME)
                            .cloned()
                            .unwrap_or_else(String::new),
                        r.osm_tags
                            .get(osm::MAXSPEED)
                            .cloned()
                            .unwrap_or_else(String::new),
                    )
                };

                let mut wiz = wizard.wrap(ctx);
                let mut done = false;
                if let Some(n) = wiz.input_string_prefilled("Name the road", orig_name) {
                    if let Some(s) = wiz.input_string_prefilled("What speed limit?", orig_speed) {
                        self.model.set_r_name_and_speed(id, n, s, ctx.prerender);
                        done = true;
                    }
                }
                if done || wizard.aborted() {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::SavingModel(ref mut wizard) => {
                if let Some(name) = wizard.wrap(ctx).input_string("Name the synthetic map") {
                    self.model.map.name = name;
                    self.model.export();
                    self.state = State::Viewing;
                } else if wizard.aborted() {
                    self.state = State::Viewing;
                }
            }
            State::SelectingRectangle(pt1, ref mut pt2, ref mut keydown) => {
                if ctx.input.key_pressed(Key::LeftShift, "select area") {
                    *keydown = true;
                } else if ctx.input.key_released(Key::LeftShift) {
                    *keydown = false;
                }

                if *keydown {
                    if let Some(cursor) = ctx.canvas.get_cursor_in_map_space() {
                        *pt2 = cursor;
                    }
                }
                if ctx.input.key_pressed(Key::Escape, "stop selecting area") {
                    self.state = State::Viewing;
                } else if ctx
                    .input
                    .key_pressed(Key::Backspace, "delete everything area")
                {
                    if let Some(rect) = Polygon::rectangle_two_corners(pt1, *pt2) {
                        self.model.delete_everything_inside(rect, ctx.prerender);
                        self.model.world.handle_mouseover(ctx);
                    }
                    self.state = State::Viewing;
                }
            }
            State::CreatingTurnRestrictionPt1(from) => {
                if ctx
                    .input
                    .key_pressed(Key::Escape, "stop defining turn restriction")
                {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                } else if let Some(ID::Lane(to, _, _)) = self.model.world.get_selection() {
                    if ctx
                        .input
                        .key_pressed(Key::R, "create turn restriction to here")
                    {
                        if self.model.map.can_add_turn_restriction(from, to) {
                            self.state = State::CreatingTurnRestrictionPt2(from, to, Wizard::new());
                        } else {
                            println!("These roads aren't connected");
                        }
                    }
                }
            }
            State::CreatingTurnRestrictionPt2(from, to, ref mut wizard) => {
                if let Some((_, restriction)) =
                    wizard.wrap(ctx).choose("What turn restriction?", || {
                        vec![
                            Choice::new("ban turns between", RestrictionType::BanTurns),
                            Choice::new(
                                "only allow turns between",
                                RestrictionType::OnlyAllowTurns,
                            ),
                        ]
                    })
                {
                    self.model.add_tr(from, restriction, to, ctx.prerender);
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                } else if wizard.aborted() {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::PreviewIntersection(_, _, ref mut show_tooltip) => {
                if *show_tooltip && ctx.input.key_released(Key::RightAlt) {
                    *show_tooltip = false;
                } else if !*show_tooltip && ctx.input.key_pressed(Key::RightAlt, "show map pt") {
                    *show_tooltip = true;
                }

                if ctx
                    .input
                    .key_pressed(Key::P, "stop previewing intersection")
                {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::EnteringWarp(ref mut wizard) => {
                if let Some(line) = wizard.wrap(ctx).input_string("Warp to what?") {
                    let mut ok = false;
                    if let Ok(num) = usize::from_str_radix(&line[1..line.len()], 10) {
                        if &line[0..=0] == "i" {
                            let id = StableIntersectionID(num);
                            ctx.canvas
                                .center_on_map_pt(self.model.map.intersections[&id].point);
                            ok = true;
                        } else if &line[0..=0] == "r" {
                            let id = StableRoadID(num);
                            ctx.canvas.center_on_map_pt(self.model.get_r_center(id));
                            ok = true;
                        }
                    }
                    if !ok {
                        println!("Sorry, don't understand {}", line);
                    }
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                } else if wizard.aborted() {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::StampingRoads(ref lanespec, ref name, ref speed) => {
                if ctx
                    .input
                    .key_pressed(Key::Escape, "stop copying road metadata")
                {
                    self.state = State::Viewing;
                    self.model.world.handle_mouseover(ctx);
                } else if let Some(ID::Lane(id, _, _)) = self.model.world.get_selection() {
                    if ctx.input.key_pressed(
                        Key::C,
                        &format!("set name={}, speed={}, lanes={}", name, speed, lanespec),
                    ) {
                        self.model.set_r_name_and_speed(
                            id,
                            name.to_string(),
                            speed.to_string(),
                            ctx.prerender,
                        );
                        self.model
                            .edit_lanes(id, lanespec.to_string(), ctx.prerender);
                    }
                }
            }
        }

        self.osd = Text::new();
        ctx.input.populate_osd(&mut self.osd);
        EventLoopMode::InputOnly
    }

    fn draw(&self, g: &mut GfxCtx) {
        g.clear(Color::BLACK);
        g.draw_polygon(Color::rgb(242, 239, 233), &self.model.map.boundary_polygon);
        self.model.world.draw(g);
        self.ctx_menu.draw(g);

        match self.state {
            State::CreatingRoad(i1) => {
                if let Some(cursor) = g.get_cursor_in_map_space() {
                    if let Some(l) =
                        Line::maybe_new(self.model.map.intersections[&i1].point, cursor)
                    {
                        g.draw_line(Color::GREEN, Distance::meters(5.0), &l);
                    }
                }
            }
            State::LabelingBuilding(_, ref wizard)
            | State::LabelingRoad(_, ref wizard)
            | State::LabelingIntersection(_, ref wizard)
            | State::EditingLanes(_, ref wizard)
            | State::EditingRoadAttribs(_, ref wizard)
            | State::SavingModel(ref wizard)
            | State::EnteringWarp(ref wizard) => {
                wizard.draw(g);
            }
            State::Viewing
            | State::MovingIntersection(_)
            | State::MovingBuilding(_)
            | State::MovingRoadPoint(_, _)
            | State::StampingRoads(_, _, _) => {}
            State::SelectingRectangle(pt1, pt2, _) => {
                if let Some(rect) = Polygon::rectangle_two_corners(pt1, pt2) {
                    g.draw_polygon(Color::BLUE.alpha(0.5), &rect);
                }
            }
            State::CreatingTurnRestrictionPt1(from) => {
                if let Some(cursor) = g.get_cursor_in_map_space() {
                    if let Some(l) = Line::maybe_new(self.model.get_r_center(from), cursor) {
                        g.draw_arrow(Color::PURPLE, LANE_THICKNESS, &l);
                    }
                }
            }
            State::CreatingTurnRestrictionPt2(from, to, ref wizard) => {
                if let Some(l) =
                    Line::maybe_new(self.model.get_r_center(from), self.model.get_r_center(to))
                {
                    g.draw_arrow(Color::PURPLE, LANE_THICKNESS, &l);
                }
                wizard.draw(g);
            }
            State::PreviewIntersection(ref draw, ref labels, show_tooltip) => {
                g.redraw(draw);
                for (txt, pt) in labels {
                    g.draw_text_at_mapspace(txt, *pt);
                }

                if show_tooltip {
                    // TODO Argh, covers up mouseover tooltip.
                    if let Some(cursor) = g.canvas.get_cursor_in_map_space() {
                        g.draw_mouse_tooltip(&Text::from(Line(cursor.to_string())));
                    }
                }
            }
        };

        g.draw_blocking_text(&self.osd, ezgui::BOTTOM_RIGHT);
    }
}

fn preview_intersection(
    i: StableIntersectionID,
    model: &Model,
    ctx: &EventCtx,
) -> (Drawable, Vec<(Text, Pt2D)>) {
    let (intersection, roads, debug) = model.map.preview_intersection(i);
    let mut batch = GeomBatch::new();
    let mut labels = Vec::new();
    batch.push(Color::ORANGE.alpha(0.5), intersection);
    for r in roads {
        batch.push(Color::GREEN.alpha(0.5), r);
    }
    for (label, poly) in debug {
        labels.push((Text::from(Line(label)), poly.center()));
        batch.push(Color::RED.alpha(0.5), poly);
    }
    (ctx.prerender.upload(batch), labels)
}

fn main() {
    ezgui::run(
        ezgui::Settings::new("Synthetic map editor", (1800.0, 800.0)),
        |ctx| UI::new(ctx),
    );
}
