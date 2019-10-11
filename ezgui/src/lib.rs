mod canvas;
mod color;
mod drawing;
mod event;
mod event_ctx;
mod input;
mod runner;
mod screen_geom;
mod text;
mod widgets;
pub mod world;

pub use crate::canvas::{Canvas, HorizontalAlignment, VerticalAlignment, BOTTOM_RIGHT, CENTERED};
pub use crate::color::Color;
pub use crate::drawing::{Drawable, GeomBatch, GfxCtx, MultiText, Prerender};
pub use crate::event::{hotkey, lctrl, Event, Key, MultiKey};
pub use crate::event_ctx::EventCtx;
pub use crate::input::UserInput;
pub use crate::runner::{run, EventLoopMode, Settings, GUI};
pub use crate::screen_geom::{ScreenDims, ScreenPt, ScreenRectangle};
pub use crate::text::{Line, Text, TextSpan, HOTKEY_COLOR};
pub use crate::widgets::{
    Autocomplete, Choice, ContextMenu, ItemSlider, ModalMenu, Scroller, SidebarPos, Slider,
    SliderWithTextBox, Warper, WarpingItemSlider, Wizard, WrappedWizard,
};

pub enum InputResult<T: Clone> {
    Canceled,
    StillActive,
    Done(String, T),
}
