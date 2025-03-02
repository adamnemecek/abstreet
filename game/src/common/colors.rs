use crate::helpers::ID;
use crate::render::{DrawOptions, MIN_ZOOM_FOR_DETAIL};
use crate::ui::{ShowEverything, UI};
use ezgui::{Color, Drawable, EventCtx, GeomBatch, GfxCtx, Line, ScreenPt, Text};
use geom::{Distance, Polygon, Pt2D};
use map_model::{LaneID, Map, RoadID};
use sim::DontDrawAgents;
use std::collections::HashMap;

pub struct RoadColorerBuilder {
    prioritized_colors: Vec<Color>,
    zoomed_override_colors: HashMap<ID, Color>,
    roads: HashMap<RoadID, Color>,
    legend: ColorLegend,
}

pub struct RoadColorer {
    zoomed_override_colors: HashMap<ID, Color>,
    unzoomed: Drawable,
    legend: ColorLegend,
}

impl RoadColorer {
    pub fn draw(&self, g: &mut GfxCtx, ui: &UI) {
        let mut opts = DrawOptions::new();
        if g.canvas.cam_zoom < MIN_ZOOM_FOR_DETAIL {
            ui.draw(g, opts, &DontDrawAgents {}, &ShowEverything::new());
            g.redraw(&self.unzoomed);
        } else {
            opts.override_colors = self.zoomed_override_colors.clone();
            ui.draw(g, opts, &ui.primary.sim, &ShowEverything::new());
        }

        self.legend.draw(g);
    }
}

impl RoadColorerBuilder {
    // Colors listed earlier override those listed later. This is used in unzoomed mode, when one
    // road has lanes of different colors.
    pub fn new(header: Text, prioritized_colors: Vec<(&str, Color)>) -> RoadColorerBuilder {
        RoadColorerBuilder {
            prioritized_colors: prioritized_colors.iter().map(|(_, c)| *c).collect(),
            zoomed_override_colors: HashMap::new(),
            roads: HashMap::new(),
            legend: ColorLegend::new(header, prioritized_colors),
        }
    }

    pub fn add(&mut self, l: LaneID, color: Color, map: &Map) {
        self.zoomed_override_colors.insert(ID::Lane(l), color);
        let r = map.get_parent(l).id;
        if let Some(existing) = self.roads.get(&r) {
            if self.prioritized_colors.iter().position(|c| *c == color)
                < self.prioritized_colors.iter().position(|c| c == existing)
            {
                self.roads.insert(r, color);
            }
        } else {
            self.roads.insert(r, color);
        }
    }

    pub fn build(self, ctx: &EventCtx, map: &Map) -> RoadColorer {
        let mut batch = GeomBatch::new();
        for (r, color) in self.roads {
            batch.push(color, map.get_r(r).get_thick_polygon().unwrap());
        }
        RoadColorer {
            zoomed_override_colors: self.zoomed_override_colors,
            unzoomed: batch.upload(ctx),
            legend: self.legend,
        }
    }
}

pub struct ObjectColorerBuilder {
    zoomed_override_colors: HashMap<ID, Color>,
    legend: ColorLegend,
    roads: Vec<(RoadID, Color)>,
}

pub struct ObjectColorer {
    zoomed_override_colors: HashMap<ID, Color>,
    unzoomed: Drawable,
    legend: ColorLegend,
}

impl ObjectColorer {
    pub fn draw(&self, g: &mut GfxCtx, ui: &UI) {
        let mut opts = DrawOptions::new();
        if g.canvas.cam_zoom < MIN_ZOOM_FOR_DETAIL {
            ui.draw(g, opts, &DontDrawAgents {}, &ShowEverything::new());
            g.redraw(&self.unzoomed);
        } else {
            opts.override_colors = self.zoomed_override_colors.clone();
            ui.draw(g, opts, &ui.primary.sim, &ShowEverything::new());
        }

        self.legend.draw(g);
    }
}

impl ObjectColorerBuilder {
    pub fn new(header: Text, rows: Vec<(&str, Color)>) -> ObjectColorerBuilder {
        ObjectColorerBuilder {
            zoomed_override_colors: HashMap::new(),
            legend: ColorLegend::new(header, rows),
            roads: Vec::new(),
        }
    }

    pub fn add(&mut self, id: ID, color: Color) {
        if let ID::Road(r) = id {
            self.roads.push((r, color));
        } else {
            self.zoomed_override_colors.insert(id, color);
        }
    }

    pub fn build(mut self, ctx: &EventCtx, map: &Map) -> ObjectColorer {
        let mut batch = GeomBatch::new();
        for (id, color) in &self.zoomed_override_colors {
            let poly = match id {
                ID::Building(b) => map.get_b(*b).polygon.clone(),
                ID::Intersection(i) => map.get_i(*i).polygon.clone(),
                _ => unreachable!(),
            };
            batch.push(*color, poly);
        }
        for (r, color) in self.roads {
            batch.push(color, map.get_r(r).get_thick_polygon().unwrap());
            for l in map.get_r(r).all_lanes() {
                self.zoomed_override_colors.insert(ID::Lane(l), color);
            }
        }
        ObjectColorer {
            zoomed_override_colors: self.zoomed_override_colors,
            unzoomed: batch.upload(ctx),
            legend: self.legend,
        }
    }
}

pub struct ColorLegend {
    header: Text,
    rows: Vec<(String, Color)>,
}

impl ColorLegend {
    pub fn new(header: Text, rows: Vec<(&str, Color)>) -> ColorLegend {
        ColorLegend {
            header,
            rows: rows
                .into_iter()
                .map(|(label, c)| (label.to_string(), c.alpha(1.0)))
                .collect(),
        }
    }

    pub fn draw(&self, g: &mut GfxCtx) {
        // TODO Want to draw a little rectangular box on each row, but how do we know positioning?
        // - v1: manually figure it out here with line height, padding, etc
        // - v2: be able to say something like "row: rectangle with width=30, height=80% of row.
        // then 10px spacing. then this text"
        // TODO Need to recalculate all this if the panel moves
        let mut txt = self.header.clone();
        for (label, _) in &self.rows {
            txt.add(Line(label));
        }
        g.draw_text_at_screenspace_topleft(
            &txt,
            ScreenPt::new(
                50.0,
                g.canvas.window_height
                    - (g.default_line_height()
                        * ((self.rows.len() + self.header.num_lines() + 1) as f64)),
            ),
        );

        let mut batch = GeomBatch::new();
        // Hacky way to extend the text box's background a little...
        batch.push(
            Color::grey(0.2),
            Polygon::rectangle_topleft(
                Pt2D::new(
                    0.0,
                    g.canvas.window_height
                        - (g.default_line_height()
                            * ((self.rows.len() + self.header.num_lines() + 1) as f64)),
                ),
                Distance::meters(50.0),
                Distance::meters(
                    g.default_line_height()
                        * ((self.rows.len() + self.header.num_lines() + 1) as f64),
                ),
            ),
        );
        let square_dims = 0.8 * g.default_line_height();
        for (idx, (_, c)) in self.rows.iter().enumerate() {
            let offset_from_bottom = 1 + self.rows.len() - idx;
            batch.push(
                *c,
                Polygon::rectangle_topleft(
                    Pt2D::new(
                        20.0,
                        g.canvas.window_height
                            - g.default_line_height() * (offset_from_bottom as f64)
                            + (g.default_line_height() - square_dims) / 2.0,
                    ),
                    Distance::meters(square_dims),
                    Distance::meters(square_dims),
                ),
            );
        }
        g.fork_screenspace();
        batch.draw(g);
        g.unfork();
    }
}
