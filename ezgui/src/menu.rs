use piston::input::{Button, Key, PressEvent};
use {InputResult, TextOSD, UserInput};

pub struct Menu {
    choices: Vec<String>,
    current_idx: usize,
}

impl Menu {
    pub fn new(choices: Vec<String>) -> Menu {
        Menu {
            choices,
            current_idx: 0,
        }
    }

    pub fn event(&mut self, input: &mut UserInput) -> InputResult {
        let ev = input.use_event_directly().clone();
        input.consume_event();

        if let Some(Button::Keyboard(Key::Escape)) = ev.press_args() {
            return InputResult::Canceled;
        }

        if let Some(Button::Keyboard(Key::Return)) = ev.press_args() {
            return InputResult::Done(self.choices[self.current_idx].clone());
        }

        if let Some(Button::Keyboard(Key::Up)) = ev.press_args() {
            if self.current_idx > 0 {
                self.current_idx -= 1;
            }
        }
        if let Some(Button::Keyboard(Key::Down)) = ev.press_args() {
            if self.current_idx < self.choices.len() - 1 {
                self.current_idx += 1;
            }
        }

        InputResult::StillActive
    }

    // TODO different API... handle menus bigger than the screen, actually do scroll. maybe always
    // display one size for the menu, just dont fill everything out
    pub fn get_osd(&self) -> TextOSD {
        let mut osd = TextOSD::new();
        for (idx, line) in self.choices.iter().enumerate() {
            if self.current_idx == idx {
                osd.add_highlighted_line(line.clone());
            } else {
                osd.add_line(line.clone());
            }
        }
        osd
    }

    pub fn current_choice(&self) -> &String {
        &self.choices[self.current_idx]
    }
}
