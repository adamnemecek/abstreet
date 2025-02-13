mod model;
mod upstream;
mod world;

use abstutil::{CmdArgs, Timer};
use ezgui::{
    hotkey, Canvas, Choice, Color, Drawable, EventCtx, EventLoopMode, GeomBatch, GfxCtx, Key, Line,
    ModalMenu, Text, TextureType, Wizard, GUI,
};
use geom::{Distance, Line, Polygon, Pt2D};
use map_model::raw::{OriginalBuilding, OriginalIntersection, OriginalRoad, RestrictionType};
use map_model::{osm, LANE_THICKNESS};
use model::{Model, ID};
use std::collections::HashSet;

struct UI {
    model: Model,
    state: State,
    menu: ModalMenu,
    sidebar: Text,

    last_id: Option<ID>,
}

enum State {
    Viewing { short_roads: HashSet<OriginalRoad> },
    MovingIntersection(OriginalIntersection),
    MovingBuilding(OriginalBuilding),
    MovingRoadPoint(OriginalRoad, usize),
    CreatingRoad(OriginalIntersection),
    EditingLanes(OriginalRoad, Wizard),
    EditingRoadAttribs(OriginalRoad, Wizard),
    SavingModel(Wizard),
    // bool is if key is down
    SelectingRectangle(Pt2D, Pt2D, bool),
    CreatingTurnRestrictionPt1(OriginalRoad),
    CreatingTurnRestrictionPt2(OriginalRoad, OriginalRoad, Wizard),
    // bool is show_tooltip
    PreviewIntersection(Drawable, Vec<(Text, Pt2D)>, bool),
    EnteringWarp(Wizard),
    StampingRoads(String, String, String, String),
}

impl State {
    fn viewing() -> State {
        State::Viewing {
            short_roads: HashSet::new(),
        }
    }
}

impl UI {
    fn new(ctx: &mut EventCtx) -> UI {
        let mut args = CmdArgs::new();
        let load = args.optional_free();
        let include_bldgs = args.enabled("--bldgs");
        let intersection_geom = args.enabled("--geom");
        let no_fixes = args.enabled("--nofixes");
        args.done();

        let model = if let Some(path) = load {
            Model::import(
                path,
                include_bldgs,
                intersection_geom,
                no_fixes,
                ctx.prerender,
            )
        } else {
            Model::blank()
        };
        ctx.set_textures(
            vec![
                ("assets/ui/hide.png", TextureType::Stretch),
                ("assets/ui/show.png", TextureType::Stretch),
            ],
            &mut Timer::throwaway(),
        );
        ctx.canvas.load_camera_state(&model.map.name);
        let mut ui = UI {
            model,
            state: State::viewing(),
            menu: ModalMenu::new(
                "Map Editor",
                vec![
                    (hotkey(Key::Escape), "quit"),
                    (None, "save raw map"),
                    (hotkey(Key::F), "save map fixes"),
                    (hotkey(Key::J), "warp to something"),
                    (None, "produce OSM parking+sidewalk diff"),
                    (hotkey(Key::G), "preview all intersections"),
                    (None, "find overlapping intersections"),
                    (hotkey(Key::Z), "find short roads"),
                ],
                ctx,
            ),
            sidebar: Text::new(),

            last_id: None,
        };
        ui.recount_parking_tags(ctx);
        ui
    }

    fn recount_parking_tags(&mut self, ctx: &EventCtx) {
        let mut ways_audited = HashSet::new();
        let mut ways_missing = HashSet::new();
        for r in self.model.map.roads.values() {
            if r.synthetic() {
                continue;
            }
            if r.osm_tags.contains_key(osm::INFERRED_PARKING) {
                ways_missing.insert(r.osm_tags[osm::OSM_WAY_ID].clone());
            } else {
                ways_audited.insert(r.osm_tags[osm::OSM_WAY_ID].clone());
            }
        }
        self.menu.set_info(
            ctx,
            Text::from(Line(format!(
                "Parking data audited: {} / {} ways",
                abstutil::prettyprint_usize(ways_audited.len()),
                abstutil::prettyprint_usize(ways_audited.len() + ways_missing.len())
            ))),
        );
    }
}

impl GUI for UI {
    fn event(&mut self, ctx: &mut EventCtx) -> EventLoopMode {
        ctx.canvas.handle_event(ctx.input);
        self.menu.event(ctx);
        if ctx.redo_mouseover() {
            self.model.world.handle_mouseover(ctx);
        }

        let mut cursor = ctx.canvas.get_cursor_in_map_space();
        // Negative coordinates break the quadtree in World, so try to prevent anything involving
        // them. Creating stuff near the boundary or moving things past it still crash, but this
        // and drawing the boundary kind of help.
        if let Some(pt) = cursor {
            if pt.x() < 0.0 || pt.y() < 0.0 {
                cursor = None;
            }
        }

        match self.state {
            State::Viewing {
                ref mut short_roads,
            } => {
                {
                    let before = match self.last_id {
                        Some(ID::Road(r)) | Some(ID::RoadPoint(r, _)) => Some(r),
                        _ => None,
                    };
                    let after = match self.model.world.get_selection() {
                        Some(ID::Road(r)) | Some(ID::RoadPoint(r, _)) => Some(r),
                        _ => None,
                    };
                    if before != after {
                        if let Some(id) = before {
                            self.model.stop_showing_pts(id);
                        }
                        if let Some(r) = after {
                            self.model.show_r_points(r, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        }
                    }
                }

                match self.model.world.get_selection() {
                    Some(ID::Intersection(i)) => {
                        if ctx.input.key_pressed(Key::LeftControl, "move intersection") {
                            self.state = State::MovingIntersection(i);
                        } else if ctx.input.key_pressed(Key::R, "create road") {
                            self.state = State::CreatingRoad(i);
                        } else if ctx.input.key_pressed(Key::Backspace, "delete building") {
                            self.model.delete_i(i);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::T, "toggle intersection type") {
                            self.model.toggle_i_type(i, ctx.prerender);
                        } else if !self.model.intersection_geom
                            && ctx
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
                        } else if ctx.input.key_pressed(Key::Backspace, "delete building") {
                            self.model.delete_b(b);
                            self.model.world.handle_mouseover(ctx);
                        }
                    }
                    Some(ID::Road(r)) => {
                        let could_swap = {
                            let lanes = self.model.map.roads[&r].get_spec();
                            lanes.fwd != lanes.back
                        };

                        if ctx.input.key_pressed(Key::Backspace, "delete road") {
                            self.model.delete_r(r);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::E, "edit lanes") {
                            self.state = State::EditingLanes(r, Wizard::new());
                        } else if ctx.input.key_pressed(Key::N, "edit name/speed") {
                            self.state = State::EditingRoadAttribs(r, Wizard::new());
                        } else if could_swap && ctx.input.key_pressed(Key::S, "swap lanes") {
                            self.model.swap_lanes(r, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::M, "merge road") {
                            self.model.merge_r(r, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                        } else if ctx.input.key_pressed(Key::T, "toggle parking") {
                            self.model.toggle_r_parking(r, ctx.prerender);
                            self.model.world.handle_mouseover(ctx);
                            self.recount_parking_tags(ctx);
                        } else if ctx.input.key_pressed(Key::F, "toggle sidewalks") {
                            self.model.toggle_r_sidewalks(r, ctx.prerender);
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
                                    .unwrap_or_else(|| "Unnamed street".to_string()),
                                road.osm_tags
                                    .get(osm::MAXSPEED)
                                    .cloned()
                                    .unwrap_or_else(|| "25 mph".to_string()),
                                road.osm_tags
                                    .get(osm::HIGHWAY)
                                    .cloned()
                                    .unwrap_or_else(|| "residential".to_string()),
                            );
                        } else if cursor.is_some()
                            && ctx.input.key_pressed(Key::P, "create new point")
                        {
                            if let Some(id) =
                                self.model.insert_r_pt(r, cursor.unwrap(), ctx.prerender)
                            {
                                self.model.world.force_set_selection(id);
                            }
                        } else if ctx.input.key_pressed(Key::X, "clear interior points") {
                            self.model.clear_r_pts(r, ctx.prerender);
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
                    Some(ID::TurnRestriction(tr)) => {
                        if ctx
                            .input
                            .key_pressed(Key::Backspace, "delete turn restriction")
                        {
                            self.model.delete_tr(tr);
                            self.model.world.handle_mouseover(ctx);
                        }
                    }
                    None => {
                        if self.menu.action("quit") {
                            self.before_quit(ctx.canvas);
                            std::process::exit(0);
                        } else if self.menu.action("save raw map") {
                            // TODO Only do this for synthetic maps
                            if self.model.map.name != "" {
                                self.model.export();
                            } else {
                                self.state = State::SavingModel(Wizard::new());
                            }
                        } else if self.menu.action("save map fixes") {
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
                                let id = self.model.create_b(pt, ctx.prerender);
                                self.model.world.force_set_selection(id);
                            }
                        } else if ctx.input.key_pressed(Key::LeftShift, "select area") {
                            if let Some(pt) = cursor {
                                self.state = State::SelectingRectangle(pt, pt, true);
                            }
                        } else if self.menu.action("warp to something") {
                            self.state = State::EnteringWarp(Wizard::new());
                        } else if self.menu.action("produce OSM parking+sidewalk diff") {
                            upstream::find_diffs(&self.model.map);
                        } else if !self.model.intersection_geom
                            && self.menu.action("preview all intersections")
                        {
                            let (draw, labels) = preview_all_intersections(&self.model, ctx);
                            self.state = State::PreviewIntersection(draw, labels, false);
                        } else if self.menu.action("find overlapping intersections") {
                            let (draw, labels) = find_overlapping_intersections(&self.model, ctx);
                            self.state = State::PreviewIntersection(draw, labels, false);
                        } else if short_roads.is_empty()
                            && self
                                .menu
                                .swap_action("find short roads", "clear short roads", ctx)
                        {
                            *short_roads = find_short_roads(&self.model);
                            if short_roads.is_empty() {
                                self.menu.change_action(
                                    "clear short roads",
                                    "find short roads",
                                    ctx,
                                );
                            }
                        } else if !short_roads.is_empty()
                            && self
                                .menu
                                .swap_action("clear short roads", "find short roads", ctx)
                        {
                            short_roads.clear();
                        }
                    }
                }
            }
            State::MovingIntersection(id) => {
                if let Some(pt) = cursor {
                    self.model.move_i(id, pt, ctx.prerender);
                    if ctx.input.key_released(Key::LeftControl) {
                        self.state = State::viewing();
                    }
                }
            }
            State::MovingBuilding(id) => {
                if let Some(pt) = cursor {
                    self.model.move_b(id, pt, ctx.prerender);
                    if ctx.input.key_released(Key::LeftControl) {
                        self.state = State::viewing();
                    }
                }
            }
            State::MovingRoadPoint(r, idx) => {
                if let Some(pt) = cursor {
                    self.model.move_r_pt(r, idx, pt, ctx.prerender);
                    if ctx.input.key_released(Key::LeftControl) {
                        self.state = State::viewing();
                    }
                }
            }
            State::CreatingRoad(i1) => {
                if ctx.input.key_pressed(Key::Escape, "stop defining road") {
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                } else if let Some(ID::Intersection(i2)) = self.model.world.get_selection() {
                    if i1 != i2 && ctx.input.key_pressed(Key::R, "finalize road") {
                        self.model.create_r(i1, i2, ctx.prerender);
                        self.state = State::viewing();
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
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                } else if wizard.aborted() {
                    self.state = State::viewing();
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
                        if let Some(h) = wiz
                            .choose_string("What highway type (for coloring)?", || {
                                vec!["motorway", "primary", "residential"]
                            })
                        {
                            self.model.set_r_name_and_speed(id, n, s, h, ctx.prerender);
                            done = true;
                        }
                    }
                }
                if done || wizard.aborted() {
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::SavingModel(ref mut wizard) => {
                if let Some(name) = wizard.wrap(ctx).input_string("Name the synthetic map") {
                    self.model.map.name = name;
                    self.model.export();
                    self.state = State::viewing();
                } else if wizard.aborted() {
                    self.state = State::viewing();
                }
            }
            State::SelectingRectangle(pt1, ref mut pt2, ref mut keydown) => {
                if ctx.input.key_pressed(Key::LeftShift, "select area") {
                    *keydown = true;
                } else if ctx.input.key_released(Key::LeftShift) {
                    *keydown = false;
                }

                if *keydown {
                    if let Some(pt) = cursor {
                        *pt2 = pt;
                    }
                }
                if ctx.input.key_pressed(Key::Escape, "stop selecting area") {
                    self.state = State::viewing();
                } else if ctx
                    .input
                    .key_pressed(Key::Backspace, "delete everything in area")
                {
                    if let Some(rect) = Polygon::rectangle_two_corners(pt1, *pt2) {
                        self.model.delete_everything_inside(rect);
                        self.model.world.handle_mouseover(ctx);
                    }
                    self.state = State::viewing();
                }
            }
            State::CreatingTurnRestrictionPt1(from) => {
                if ctx
                    .input
                    .key_pressed(Key::Escape, "stop defining turn restriction")
                {
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                } else if let Some(ID::Road(to)) = self.model.world.get_selection() {
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
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                } else if wizard.aborted() {
                    self.state = State::viewing();
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
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::EnteringWarp(ref mut wizard) => {
                if let Some(line) = wizard.wrap(ctx).input_string("Warp to what?") {
                    let mut ok = false;
                    if let Ok(num) = i64::from_str_radix(&line[1..line.len()], 10) {
                        if &line[0..=0] == "i" {
                            let id = OriginalIntersection { osm_node_id: num };
                            ctx.canvas
                                .center_on_map_pt(self.model.map.intersections[&id].point);
                            ok = true;
                        }
                    }
                    if !ok {
                        println!("Sorry, don't understand {}", line);
                    }
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                } else if wizard.aborted() {
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                }
            }
            State::StampingRoads(ref lanespec, ref name, ref speed, ref highway) => {
                if ctx
                    .input
                    .key_pressed(Key::Escape, "stop copying road metadata")
                {
                    self.state = State::viewing();
                    self.model.world.handle_mouseover(ctx);
                } else if let Some(ID::Road(id)) = self.model.world.get_selection() {
                    if ctx.input.key_pressed(
                        Key::C,
                        &format!(
                            "set name={}, speed={}, lanes={}, highway={}",
                            name, speed, lanespec, highway
                        ),
                    ) {
                        self.model.set_r_name_and_speed(
                            id,
                            name.to_string(),
                            speed.to_string(),
                            highway.to_string(),
                            ctx.prerender,
                        );
                        self.model
                            .edit_lanes(id, lanespec.to_string(), ctx.prerender);
                    }
                }
            }
        }

        self.sidebar = Text::new();
        self.sidebar.override_width = Some(0.3 * ctx.canvas.window_width);
        self.sidebar.override_height = Some(ctx.canvas.window_height);
        if let Some(id) = self.model.world.get_selection() {
            self.model.populate_obj_info(id, &mut self.sidebar);
        } else {
            self.sidebar.add_highlighted(Line("..."), Color::BLUE);
        }

        // I don't think a clickable menu of buttons makes sense here. These controls need to
        // operate on the thing where the mouse is currently. Sometimes that's not even an object
        // (like selecting an area or placing a new building).
        self.sidebar.add(Line(""));
        self.sidebar.add_highlighted(Line("Controls"), Color::BLUE);
        ctx.input.populate_osd(&mut self.sidebar);

        self.last_id = self.model.world.get_selection();

        EventLoopMode::InputOnly
    }

    fn draw(&self, g: &mut GfxCtx) {
        g.clear(Color::BLACK);

        // It's useful to see the origin.
        g.draw_polygon(
            Color::WHITE,
            &Polygon::rectangle_topleft(
                Pt2D::new(0.0, 0.0),
                Distance::meters(100.0),
                Distance::meters(10.0),
            ),
        );
        g.draw_polygon(
            Color::WHITE,
            &Polygon::rectangle_topleft(
                Pt2D::new(0.0, 0.0),
                Distance::meters(10.0),
                Distance::meters(100.0),
            ),
        );

        g.draw_polygon(Color::rgb(242, 239, 233), &self.model.map.boundary_polygon);
        match self.state {
            State::PreviewIntersection(_, _, _) => self.model.world.draw(g, |id| match id {
                ID::Intersection(_) => false,
                _ => true,
            }),
            _ => self.model.world.draw(g, |_| true),
        }

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
            State::EditingLanes(_, ref wizard)
            | State::EditingRoadAttribs(_, ref wizard)
            | State::SavingModel(ref wizard)
            | State::EnteringWarp(ref wizard) => {
                wizard.draw(g);
            }
            State::Viewing { ref short_roads } => {
                for r in short_roads {
                    if let Some(p) = self.model.world.get_unioned_polygon(ID::Road(*r)) {
                        g.draw_polygon(Color::CYAN, p);
                    }
                }
            }
            State::MovingIntersection(_)
            | State::MovingBuilding(_)
            | State::MovingRoadPoint(_, _)
            | State::StampingRoads(_, _, _, _) => {}
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

        self.menu.draw(g);
        g.draw_blocking_text(
            &self.sidebar,
            (
                ezgui::HorizontalAlignment::Left,
                ezgui::VerticalAlignment::Top,
            ),
        );
    }

    fn dump_before_abort(&self, canvas: &Canvas) {
        canvas.save_camera_state(&self.model.map.name);
    }

    fn before_quit(&self, canvas: &Canvas) {
        canvas.save_camera_state(&self.model.map.name);
    }
}

fn preview_intersection(
    i: OriginalIntersection,
    model: &Model,
    ctx: &EventCtx,
) -> (Drawable, Vec<(Text, Pt2D)>) {
    let (intersection, roads, debug) = model
        .map
        .preview_intersection(i, &mut Timer::new("calculate intersection_polygon"));
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
    (batch.upload(ctx), labels)
}

fn preview_all_intersections(model: &Model, ctx: &EventCtx) -> (Drawable, Vec<(Text, Pt2D)>) {
    let mut batch = GeomBatch::new();
    let mut timer = Timer::new("preview all intersections");
    timer.start_iter("preview", model.map.intersections.len());
    for i in model.map.intersections.keys() {
        timer.next();
        if model.map.roads_per_intersection(*i).is_empty() {
            continue;
        }
        let (intersection, _, _) = model.map.preview_intersection(*i, &mut timer);
        batch.push(Color::ORANGE.alpha(0.5), intersection);
    }
    (batch.upload(ctx), Vec::new())
}

fn find_overlapping_intersections(model: &Model, ctx: &EventCtx) -> (Drawable, Vec<(Text, Pt2D)>) {
    let mut timer = Timer::new("find overlapping intersections");
    let mut polygons = Vec::new();
    for i in model.map.intersections.keys() {
        if model.map.roads_per_intersection(*i).is_empty() {
            continue;
        }
        let (intersection, _, _) = model.map.preview_intersection(*i, &mut timer);
        polygons.push((*i, intersection));
    }

    let mut overlap = Vec::new();
    timer.start_iter(
        "terrible quadratic intersection check",
        polygons.len().pow(2),
    );
    for (i1, poly1) in &polygons {
        for (i2, poly2) in &polygons {
            timer.next();
            if i1 >= i2 {
                continue;
            }
            let hits = poly1.intersection(poly2);
            if !hits.is_empty() {
                overlap.extend(hits);
                timer.warn(format!("{} hits {}", i1, i2));
            }
        }
    }

    let mut batch = GeomBatch::new();
    batch.extend(Color::RED.alpha(0.5), overlap);
    (batch.upload(ctx), Vec::new())
}

// TODO OriginalRoad is dangerous, as this map changes. :\
fn find_short_roads(model: &Model) -> HashSet<OriginalRoad> {
    // Assume the full map has been built. We really care about short lanes there.
    let map: map_model::Map =
        abstutil::read_binary(abstutil::path_map(&model.map.name), &mut Timer::throwaway());
    // Buses are 12.5
    let threshold = Distance::meters(13.0);
    let mut roads: HashSet<OriginalRoad> = HashSet::new();
    for l in map.all_lanes() {
        if l.length() < threshold {
            roads.insert(map.get_r(l.parent).orig_id);
        }
    }
    println!("{} short roads", roads.len());
    for r in &roads {
        println!("- {}", r);
    }
    roads
}

fn main() {
    ezgui::run(
        ezgui::Settings::new("Synthetic map editor", (1800.0, 800.0)),
        |ctx| UI::new(ctx),
    );
}
