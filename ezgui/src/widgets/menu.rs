use crate::{
    hotkey, lctrl, text, Canvas, Color, Event, GeomBatch, GfxCtx, InputResult, Key, Line, MultiKey,
    ScreenDims, ScreenPt, ScreenRectangle, Text,
};
use geom::{Circle, Distance, Polygon, Pt2D};
use std::collections::HashSet;

const ICON_BACKGROUND: Color = Color::grey(0.5);
const ICON_BACKGROUND_SELECTED: Color = Color::YELLOW;
const ICON_SYMBOL: Color = Color::grey(0.8);
const ICON_SYMBOL_SELECTED: Color = Color::grey(0.2);

// Stores some associated data with each choice
pub struct Menu<T: Clone> {
    pub prompt: Text,
    choices: Vec<Choice<T>>,
    current_idx: Option<usize>,
    mouse_in_bounds: bool,
    keys_enabled: bool,
    hideable: bool,
    hidden: bool,
    pos: Position,
    top_left: ScreenPt,
    total_width: f64,
    // dy1 values of the separator half-rows
    separators: Vec<f64>,
    icon_selected: bool,
}

// TODO Maybe reuse the public Choice from wizard
struct Choice<T: Clone> {
    hotkey: Option<MultiKey>,
    label: String,
    active: bool,
    data: T,
    // How far is the top of this row below the prompt's bottom?
    dy1: f64,
}

#[derive(Clone)]
pub enum Position {
    ScreenCenter,
    TopLeftAt(ScreenPt),
    SomeCornerAt(ScreenPt),
    TopRightOfScreen,
}

impl<T: Clone> Menu<T> {
    pub fn new(
        mut prompt: Text,
        raw_choice_groups: Vec<Vec<(Option<MultiKey>, String, T)>>,
        keys_enabled: bool,
        hideable: bool,
        pos: Position,
        canvas: &Canvas,
    ) -> Menu<T> {
        let mut used_keys = HashSet::new();
        let mut used_labels = HashSet::new();
        let mut choices = Vec::new();
        let mut txt = prompt.clone();
        let prompt_lines = prompt.num_lines();
        let mut separator_offset = 0.0;
        let mut separators = Vec::new();
        for group in raw_choice_groups {
            for (maybe_key, label, data) in group {
                if let Some(key) = maybe_key {
                    if used_keys.contains(&key) {
                        panic!("Menu for {:?} uses {} twice", prompt, key.describe());
                    }
                    used_keys.insert(key);
                }

                if used_labels.contains(&label) {
                    panic!("Menu for {:?} has two entries for {}", prompt, label);
                }
                used_labels.insert(label.clone());

                let dy1 = ((txt.num_lines() - prompt_lines) as f64) * canvas.line_height
                    + separator_offset;
                if let Some(key) = maybe_key {
                    txt.add(Line(format!("{} - {}", key.describe(), label)));
                } else {
                    txt.add(Line(&label));
                }

                choices.push(Choice {
                    hotkey: maybe_key,
                    label,
                    active: true,
                    data,
                    dy1,
                });
            }
            separator_offset += canvas.line_height / 2.0;
            if let Some(ref c) = choices.last() {
                separators.push(c.dy1 + canvas.line_height);
            }
        }
        // The last one would be at the very bottom of the menu
        separators.pop();

        if choices.is_empty() {
            //panic!("Can't create a menu without choices for {:?}", prompt);
        }

        let (total_width, total_height) = canvas.text_dims(&txt);
        let top_left = pos.get_top_left(canvas, ScreenDims::new(total_width, total_height));
        prompt.override_width = Some(total_width);

        Menu {
            prompt,
            choices,
            current_idx: if keys_enabled { Some(0) } else { None },
            keys_enabled,
            // TODO Bit of a hack, but eh.
            mouse_in_bounds: !keys_enabled,
            pos,
            hideable,
            hidden: false,
            top_left,
            total_width,
            separators,
            icon_selected: false,
        }
    }

    pub fn event(&mut self, ev: Event, canvas: &mut Canvas) -> InputResult<T> {
        if self.hideable {
            if let Event::MouseMovedTo(pt) = ev {
                if !canvas.is_dragging() {
                    self.icon_selected = self
                        .get_expand_icon(canvas)
                        .contains_pt(Pt2D::new(pt.x, pt.y));
                }
            }

            if (ev == Event::KeyPress(Key::Tab))
                || (ev == Event::LeftMouseButtonDown && self.icon_selected)
            {
                if self.hidden {
                    self.hidden = false;
                } else {
                    self.hidden = true;
                    self.current_idx = None;
                }
                canvas.hide_modal_menus = self.hidden;
                self.recalculate_geom(canvas);
                return InputResult::StillActive;
            }
        }

        if !self.hidden {
            // Handle the mouse
            if ev == Event::LeftMouseButtonDown {
                if let Some(i) = self.current_idx {
                    let choice = &self.choices[i];
                    if choice.active && self.mouse_in_bounds {
                        return InputResult::Done(choice.label.clone(), choice.data.clone());
                    } else {
                        return InputResult::StillActive;
                    }
                } else {
                    return InputResult::Canceled;
                }
            } else if ev == Event::RightMouseButtonDown {
                return InputResult::Canceled;
            } else if let Event::MouseMovedTo(pt) = ev {
                if !canvas.is_dragging() {
                    for (idx, choice) in self.choices.iter().enumerate() {
                        let y1 = self.top_left.y
                            + choice.dy1
                            + (self.prompt.num_lines() as f64) * canvas.line_height;

                        if choice.active
                            && (ScreenRectangle {
                                x1: self.top_left.x,
                                y1,
                                x2: self.top_left.x + self.total_width,
                                y2: y1 + canvas.line_height,
                            }
                            .contains(pt))
                        {
                            self.current_idx = Some(idx);
                            self.mouse_in_bounds = true;
                            return InputResult::StillActive;
                        }
                    }
                    self.mouse_in_bounds = false;
                    if !self.keys_enabled {
                        self.current_idx = None;
                    }
                    return InputResult::StillActive;
                }
            }

            // Handle keys
            if self.keys_enabled {
                let idx = self.current_idx.unwrap();
                if ev == Event::KeyPress(Key::Enter) {
                    let choice = &self.choices[idx];
                    if choice.active {
                        return InputResult::Done(choice.label.clone(), choice.data.clone());
                    } else {
                        return InputResult::StillActive;
                    }
                } else if ev == Event::KeyPress(Key::UpArrow) {
                    if idx > 0 {
                        self.current_idx = Some(idx - 1);
                    }
                } else if ev == Event::KeyPress(Key::DownArrow) {
                    if idx < self.choices.len() - 1 {
                        self.current_idx = Some(idx + 1);
                    }
                }
            }
        }

        if let Event::KeyPress(key) = ev {
            let pressed = if canvas.lctrl_held {
                lctrl(key)
            } else {
                hotkey(key)
            };
            for choice in &self.choices {
                if choice.active && pressed == choice.hotkey {
                    return InputResult::Done(choice.label.clone(), choice.data.clone());
                }
            }
        }

        // This is always an option, but do this last, in case Escape is a hotkey of a menu choice.
        if ev == Event::KeyPress(Key::Escape) {
            return InputResult::Canceled;
        }

        if let Event::WindowResized(_, _) = ev {
            self.recalculate_geom(canvas);
        }

        InputResult::StillActive
    }

    pub fn draw(&self, g: &mut GfxCtx) {
        g.draw_text_at_screenspace_topleft(&self.prompt, self.top_left);
        if self.hidden {
            let mut batch = GeomBatch::new();
            // Draw the expand icon. Hopefully it doesn't clobber the prompt.
            let icon = self.get_expand_icon(g.canvas);
            batch.push(
                if self.icon_selected {
                    ICON_BACKGROUND_SELECTED
                } else {
                    ICON_BACKGROUND
                },
                icon.to_polygon(),
            );
            batch.push(
                if self.icon_selected {
                    ICON_SYMBOL_SELECTED
                } else {
                    ICON_SYMBOL
                },
                Polygon::rectangle(icon.center, 1.5 * icon.radius, 0.5 * icon.radius),
            );
            batch.push(
                if self.icon_selected {
                    ICON_SYMBOL_SELECTED
                } else {
                    ICON_SYMBOL
                },
                Polygon::rectangle(icon.center, 0.5 * icon.radius, 1.5 * icon.radius),
            );
            g.fork_screenspace();
            batch.draw(g);
            g.unfork();

            // TODO This doesn't look great:
            // 1) Partly covers up the icon
            // 2) Doesn't properly cover menu text underneath
            // 3) Color blends in with menu
            if self.icon_selected {
                let mut txt = Text::new();
                txt.add_appended(vec![
                    Line(Key::Tab.describe()).fg(text::HOTKEY_COLOR),
                    Line(" - expand menu"),
                ]);
                g.draw_mouse_tooltip(&txt);
            }

            return;
        }

        let base_y = self.top_left.y + (self.prompt.num_lines() as f64) * g.canvas.line_height;

        let mut batch = GeomBatch::new();

        if let Some(c) = self.choices.last() {
            let choices_total_height = c.dy1 + g.canvas.line_height;

            batch.push(
                text::BG_COLOR,
                Polygon::rectangle_topleft(
                    Pt2D::new(self.top_left.x, base_y),
                    Distance::meters(self.total_width),
                    Distance::meters(choices_total_height),
                ),
            );
            g.canvas.mark_covered_area(ScreenRectangle {
                x1: self.top_left.x,
                y1: base_y,
                x2: self.top_left.x + self.total_width,
                y2: base_y + choices_total_height,
            });
        }

        for dy1 in &self.separators {
            batch.push(
                Color::grey(0.4),
                Polygon::rectangle_topleft(
                    Pt2D::new(
                        self.top_left.x,
                        base_y + *dy1 + (g.canvas.line_height / 4.0),
                    ),
                    Distance::meters(self.total_width),
                    Distance::meters(g.canvas.line_height / 4.0),
                ),
            );
        }

        // Draw the minimize icon. Hopefully it doesn't clobber the prompt.
        if self.hideable {
            let icon = self.get_expand_icon(g.canvas);
            batch.push(
                if self.icon_selected {
                    ICON_BACKGROUND_SELECTED
                } else {
                    ICON_BACKGROUND
                },
                icon.to_polygon(),
            );
            batch.push(
                if self.icon_selected {
                    ICON_SYMBOL_SELECTED
                } else {
                    ICON_SYMBOL
                },
                Polygon::rectangle(icon.center, 1.5 * icon.radius, 0.5 * icon.radius),
            );
        }

        g.fork_screenspace();
        batch.draw(g);
        g.unfork();

        if self.icon_selected {
            let mut txt = Text::new();
            txt.add_appended(vec![
                Line(Key::Tab.describe()).fg(text::HOTKEY_COLOR),
                Line(" - hide menu"),
            ]);
            g.draw_mouse_tooltip(&txt);
        }

        for (idx, choice) in self.choices.iter().enumerate() {
            let mut txt = Text::with_bg_color(if Some(idx) == self.current_idx {
                Some(text::SELECTED_COLOR)
            } else {
                None
            });
            txt.override_width = Some(self.total_width);
            if choice.active {
                if let Some(key) = choice.hotkey {
                    txt.add_appended(vec![
                        Line(key.describe()).fg(text::HOTKEY_COLOR),
                        Line(format!(" - {}", choice.label)),
                    ]);
                } else {
                    txt.add(Line(&choice.label));
                }
            } else {
                if let Some(key) = choice.hotkey {
                    txt.add(
                        Line(format!("{} - {}", key.describe(), choice.label))
                            .fg(text::INACTIVE_CHOICE_COLOR),
                    );
                } else {
                    txt.add(Line(&choice.label).fg(text::INACTIVE_CHOICE_COLOR));
                }
            }
            // Is drawing each row individually slower?
            g.draw_text_at_screenspace_topleft(
                &txt,
                ScreenPt::new(self.top_left.x, base_y + choice.dy1),
            );
        }
    }

    pub fn current_choice(&self) -> Option<&T> {
        let idx = self.current_idx?;
        Some(&self.choices[idx].data)
    }

    pub fn active_choices(&self) -> Vec<&T> {
        self.choices
            .iter()
            .filter_map(|choice| {
                if choice.active {
                    Some(&choice.data)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn mark_active(&mut self, label: &str, is_active: bool) {
        for choice in self.choices.iter_mut() {
            if choice.label == label {
                if choice.active == is_active {
                    panic!(
                        "Menu choice for {} already had active={}",
                        choice.label, is_active
                    );
                }
                choice.active = is_active;
                return;
            }
        }
        panic!(
            "Menu with prompt {:?} has no choice {} to mark active",
            self.prompt, label
        );
    }

    pub fn mark_all_inactive(&mut self) {
        for choice in self.choices.iter_mut() {
            choice.active = false;
        }
    }

    pub fn make_hidden(&mut self, canvas: &Canvas) {
        assert!(!self.hidden);
        assert!(self.hideable);
        self.hidden = true;
        self.recalculate_geom(canvas);
    }

    pub fn change_prompt(&mut self, prompt: Text, canvas: &Canvas) {
        self.prompt = prompt;
        self.recalculate_geom(canvas);
    }

    fn recalculate_geom(&mut self, canvas: &Canvas) {
        let mut txt = self.prompt.clone();
        if !self.hidden {
            for choice in &self.choices {
                if let Some(key) = choice.hotkey {
                    txt.add(Line(format!("{} - {}", key.describe(), choice.label)));
                } else {
                    txt.add(Line(&choice.label));
                }
            }
        }
        let (total_width, total_height) = canvas.text_dims(&txt);
        self.top_left = self
            .pos
            .get_top_left(canvas, ScreenDims::new(total_width, total_height));
        self.total_width = total_width;
        self.prompt.override_width = Some(total_width);
    }

    fn get_expand_icon(&self, canvas: &Canvas) -> Circle {
        let radius = canvas.line_height / 2.0;
        Circle::new(
            Pt2D::new(
                self.top_left.x + self.total_width - radius,
                self.top_left.y + radius,
            ),
            Distance::meters(radius),
        )
    }

    pub fn get_total_width(&self) -> f64 {
        self.total_width
    }
}

impl Position {
    fn get_top_left(&self, canvas: &Canvas, menu_dims: ScreenDims) -> ScreenPt {
        match self {
            Position::SomeCornerAt(pt) => menu_dims.top_left_for_corner(*pt, canvas),
            Position::TopLeftAt(pt) => *pt,
            Position::ScreenCenter => {
                let mut pt = canvas.center_to_screen_pt();
                pt.x -= menu_dims.width / 2.0;
                pt.y -= menu_dims.height / 2.0;
                pt
            }
            Position::TopRightOfScreen => ScreenPt::new(canvas.window_width - menu_dims.width, 0.0),
        }
    }
}
