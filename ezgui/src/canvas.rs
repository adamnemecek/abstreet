use crate::{Color, ScreenPt, ScreenRectangle, Text, UserInput};
use abstutil::Timer;
use geom::{Bounds, Distance, Polygon, Pt2D};
use glium::texture::Texture2dArray;
use serde_derive::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;

pub struct Canvas {
    // All of these f64's are in screen-space, so do NOT use Pt2D.
    // Public for saving/loading... should probably do better
    pub cam_x: f64,
    pub cam_y: f64,
    pub cam_zoom: f64,

    // TODO We probably shouldn't even track screen-space cursor when we don't have the cursor.
    pub(crate) cursor_x: f64,
    pub(crate) cursor_y: f64,
    pub(crate) window_has_cursor: bool,

    // Only for drags starting on the map. Only used to pan the map.
    pub(crate) drag_canvas_from: Option<ScreenPt>,
    pub(crate) actually_dragging: bool,
    pub(crate) drag_just_ended: bool,

    pub window_width: f64,
    pub window_height: f64,
    pub(crate) hidpi_factor: f64,

    // TODO Bit weird and hacky to mutate inside of draw() calls.
    pub(crate) covered_areas: RefCell<Vec<ScreenRectangle>>,
    pub(crate) covered_polygons: RefCell<Vec<Polygon>>,

    // Kind of just ezgui state awkwardly stuck here...
    pub(crate) lctrl_held: bool,
    // This should be mutually exclusive among all buttons and things in map-space.
    pub(crate) button_tooltip: Option<Text>,

    // TODO Definitely a weird place to stash this!
    pub(crate) texture_arrays: Vec<Texture2dArray>,
    pub(crate) texture_lookups: HashMap<String, Color>,
}

impl Canvas {
    pub(crate) fn new(initial_width: f64, initial_height: f64, hidpi_factor: f64) -> Canvas {
        Canvas {
            cam_x: 0.0,
            cam_y: 0.0,
            cam_zoom: 1.0,

            cursor_x: 0.0,
            cursor_y: 0.0,
            window_has_cursor: true,

            drag_canvas_from: None,
            actually_dragging: false,
            drag_just_ended: false,

            window_width: initial_width,
            window_height: initial_height,
            hidpi_factor,

            covered_areas: RefCell::new(Vec::new()),
            covered_polygons: RefCell::new(Vec::new()),

            lctrl_held: false,
            button_tooltip: None,

            texture_arrays: Vec::new(),
            texture_lookups: HashMap::new(),
        }
    }

    pub fn handle_event(&mut self, input: &mut UserInput) {
        // Can't start dragging or zooming on top of covered area
        if self.get_cursor_in_map_space().is_some() {
            if input.left_mouse_button_pressed() {
                self.drag_canvas_from = Some(self.get_cursor_in_screen_space());
            }

            if let Some(scroll) = input.get_mouse_scroll() {
                let old_zoom = self.cam_zoom;
                self.cam_zoom = 1.1_f64.powf(old_zoom.log(1.1) + scroll);

                // Make screen_to_map of cursor_{x,y} still point to the same thing after zooming.
                self.cam_x =
                    ((self.cam_zoom / old_zoom) * (self.cursor_x + self.cam_x)) - self.cursor_x;
                self.cam_y =
                    ((self.cam_zoom / old_zoom) * (self.cursor_y + self.cam_y)) - self.cursor_y;
            }
        }

        // If we start the drag on the map and move the mouse off the map, keep dragging.
        if let Some(click) = self.drag_canvas_from {
            let pt = self.get_cursor_in_screen_space();
            self.cam_x += click.x - pt.x;
            self.cam_y += click.y - pt.y;
            self.drag_canvas_from = Some(pt);
            if !self.actually_dragging && click != pt {
                self.actually_dragging = true;
            }

            if input.left_mouse_button_released() {
                self.drag_canvas_from = None;
                if self.actually_dragging {
                    self.drag_just_ended = true;
                } else {
                }
                self.actually_dragging = false;
            }
        } else if self.drag_just_ended {
            self.drag_just_ended = false;
        }
    }

    pub(crate) fn start_drawing(&self) {
        self.covered_areas.borrow_mut().clear();
        self.covered_polygons.borrow_mut().clear();
    }

    pub fn mark_covered_area(&self, rect: ScreenRectangle) {
        self.covered_areas.borrow_mut().push(rect);
    }

    pub fn get_cursor_in_screen_space(&self) -> ScreenPt {
        ScreenPt::new(self.cursor_x, self.cursor_y)
    }

    pub fn get_cursor_in_map_space(&self) -> Option<Pt2D> {
        if self.window_has_cursor {
            let pt = self.get_cursor_in_screen_space();

            for rect in self.covered_areas.borrow().iter() {
                if rect.contains(pt) {
                    return None;
                }
            }
            for c in self.covered_polygons.borrow().iter() {
                if c.contains_pt(pt.to_pt()) {
                    return None;
                }
            }

            Some(self.screen_to_map(pt))
        } else {
            None
        }
    }

    pub fn screen_to_map(&self, pt: ScreenPt) -> Pt2D {
        Pt2D::new(
            (pt.x + self.cam_x) / self.cam_zoom,
            (pt.y + self.cam_y) / self.cam_zoom,
        )
    }

    pub fn center_to_screen_pt(&self) -> ScreenPt {
        ScreenPt::new(self.window_width / 2.0, self.window_height / 2.0)
    }

    pub fn center_to_map_pt(&self) -> Pt2D {
        self.screen_to_map(self.center_to_screen_pt())
    }

    pub fn center_on_map_pt(&mut self, pt: Pt2D) {
        self.cam_x = (pt.x() * self.cam_zoom) - (self.window_width / 2.0);
        self.cam_y = (pt.y() * self.cam_zoom) - (self.window_height / 2.0);
    }

    pub(crate) fn map_to_screen(&self, pt: Pt2D) -> ScreenPt {
        ScreenPt::new(
            (pt.x() * self.cam_zoom) - self.cam_x,
            (pt.y() * self.cam_zoom) - self.cam_y,
        )
    }

    pub fn get_screen_bounds(&self) -> Bounds {
        let mut b = Bounds::new();
        b.update(self.screen_to_map(ScreenPt::new(0.0, 0.0)));
        b.update(self.screen_to_map(ScreenPt::new(self.window_width, self.window_height)));
        b
    }

    pub fn texture(&self, filename: &str) -> Color {
        if let Some(c) = self.texture_lookups.get(filename) {
            return *c;
        }
        panic!("Don't know texture {}", filename);
    }

    pub fn texture_rect(&self, filename: &str) -> (Color, Polygon) {
        let color = self.texture(filename);
        let dims = color.texture_dims();
        (
            color,
            Polygon::rectangle_topleft(
                Pt2D::new(0.0, 0.0),
                Distance::meters(dims.width),
                Distance::meters(dims.height),
            ),
        )
    }

    pub fn save_camera_state(&self, map_name: &str) {
        let state = CameraState {
            cam_x: self.cam_x,
            cam_y: self.cam_y,
            cam_zoom: self.cam_zoom,
        };
        abstutil::write_json(abstutil::path_camera_state(map_name), &state);
    }

    // True if this succeeds
    pub fn load_camera_state(&mut self, map_name: &str) -> bool {
        match abstutil::maybe_read_json::<CameraState>(
            abstutil::path_camera_state(map_name),
            &mut Timer::throwaway(),
        ) {
            Ok(ref loaded) => {
                self.cam_x = loaded.cam_x;
                self.cam_y = loaded.cam_y;
                self.cam_zoom = loaded.cam_zoom;
                true
            }
            _ => false,
        }
    }
}

pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
    FillScreen,
}

pub enum VerticalAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CameraState {
    cam_x: f64,
    cam_y: f64,
    cam_zoom: f64,
}
