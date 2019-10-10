use crate::{text, widgets, Canvas, Event, EventCtx, GfxCtx, Prerender, UserInput};
use glium::glutin;
use glium_glyph::glyph_brush::rusttype::Font;
use glium_glyph::{GlyphBrush, GlyphBrushBuilder};
use std::cell::Cell;
use std::time::{Duration, Instant};
use std::{panic, process, thread};

// 30fps is 1000 / 30
const SLEEP_BETWEEN_FRAMES: Duration = Duration::from_millis(33);

pub trait GUI {
    fn event(&mut self, ctx: &mut EventCtx) -> EventLoopMode;
    fn draw(&self, g: &mut GfxCtx);
    // Will be called if event or draw panics.
    fn dump_before_abort(&self, _canvas: &Canvas) {}
    // Only before a normal exit, like window close
    fn before_quit(&self, _canvas: &Canvas) {}
}

#[derive(Clone, PartialEq)]
pub enum EventLoopMode {
    Animation,
    InputOnly,
    ScreenCaptureEverything {
        dir: String,
        zoom: f64,
        max_x: f64,
        max_y: f64,
    },
    ScreenCaptureCurrentShot,
}

pub(crate) struct State<G: GUI> {
    pub(crate) gui: G,
    pub(crate) canvas: Canvas,
}

impl<G: GUI> State<G> {
    // The bool indicates if the input was actually used.
    fn event(
        mut self,
        ev: Event,
        prerender: &Prerender,
        program: &glium::Program,
    ) -> (State<G>, EventLoopMode, bool) {
        // It's impossible / very unlikey we'll grab the cursor in map space before the very first
        // start_drawing call.
        let mut input = UserInput::new(ev, &mut self.canvas);
        let mut gui = self.gui;
        let mut canvas = self.canvas;
        let event_mode = match panic::catch_unwind(panic::AssertUnwindSafe(|| {
            gui.event(&mut EventCtx {
                input: &mut input,
                canvas: &mut canvas,
                prerender,
                program,
            })
        })) {
            Ok(pair) => pair,
            Err(err) => {
                gui.dump_before_abort(&canvas);
                panic::resume_unwind(err);
            }
        };
        self.gui = gui;
        self.canvas = canvas;
        // TODO We should always do has_been_consumed, but various hacks prevent this from being
        // true. For now, just avoid the specific annoying redraw case when a KeyRelease or Update
        // event is unused.
        let input_used = match ev {
            Event::KeyRelease(_) | Event::Update => input.has_been_consumed(),
            _ => true,
        };

        (self, event_mode, input_used)
    }

    // Returns naming hint. Logically consumes the number of uploads.
    pub(crate) fn draw(
        &mut self,
        display: &glium::Display,
        program: &glium::Program,
        prerender: &Prerender,
        screenshot: bool,
    ) -> Option<String> {
        let mut target = display.draw();
        let mut g = GfxCtx::new(&self.canvas, &prerender, &mut target, program, screenshot);

        self.canvas.start_drawing();

        if let Err(err) = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            self.gui.draw(&mut g);
        })) {
            self.gui.dump_before_abort(&self.canvas);
            panic::resume_unwind(err);
        }
        let naming_hint = g.naming_hint.take();

        // Flush text just once, so that GlyphBrush's internal caching works. We have to assume
        // nothing will ever cover up text.
        {
            let top_left = self
                .canvas
                .screen_to_map(crate::screen_geom::ScreenPt::new(0.0, 0.0));
            let bottom_right = self.canvas.screen_to_map(crate::screen_geom::ScreenPt::new(
                self.canvas.window_width,
                self.canvas.window_height,
            ));
            let transform = ortho(
                (top_left.x() as f32, bottom_right.x() as f32),
                (top_left.y() as f32, bottom_right.y() as f32),
            );
            self.canvas
                .mapspace_glyphs
                .borrow_mut()
                .draw_queued_with_transform(transform, display, &mut target);
        }
        // The depth buffer doesn't seem to work between mapspace_glyphs and screenspace_glyphs. :\
        // So draw screenspace_glyphs last.
        self.canvas
            .screenspace_glyphs
            .borrow_mut()
            .draw_queued(display, &mut target);

        target.finish().unwrap();
        naming_hint
    }
}

pub struct Settings {
    window_title: String,
    initial_dims: (f64, f64),
    profiling_enabled: bool,
    default_font_size: usize,
}

impl Settings {
    pub fn new(window_title: &str, initial_dims: (f64, f64)) -> Settings {
        Settings {
            window_title: window_title.to_string(),
            initial_dims,
            profiling_enabled: false,
            default_font_size: 30,
        }
    }

    pub fn enable_profiling(&mut self) {
        assert!(!self.profiling_enabled);
        self.profiling_enabled = true;
    }

    pub fn default_font_size(&mut self, size: usize) {
        self.default_font_size = size;
    }
}

pub fn run<G: GUI, F: FnOnce(&mut EventCtx) -> G>(settings: Settings, make_gui: F) {
    let events_loop = glutin::EventsLoop::new();
    let window = glutin::WindowBuilder::new()
        .with_title(settings.window_title)
        .with_dimensions(glutin::dpi::LogicalSize::from_physical(
            glutin::dpi::PhysicalSize::new(settings.initial_dims.0, settings.initial_dims.1),
            events_loop.get_primary_monitor().get_hidpi_factor(),
        ));
    // multisampling: 2 looks bad, 4 looks fine
    //
    // The Z values are very simple:
    // 1.0: The buffer is reset every frame
    // 0.5: Map-space geometry and text
    // 0.1: Screen-space text
    // 0.0: Screen-space geometry
    // Had weird issues with Z buffering not working as intended, so this is slightly more
    // complicated than necessary to work.
    let context = glutin::ContextBuilder::new()
        .with_multisampling(4)
        .with_depth_buffer(2);
    let display = glium::Display::new(window, context, &events_loop).unwrap();

    let (vertex_shader, fragment_shader) =
        if display.is_glsl_version_supported(&glium::Version(glium::Api::Gl, 1, 4)) {
            (
                include_str!("assets/vertex_140.glsl"),
                include_str!("assets/fragment_140.glsl"),
            )
        } else {
            panic!(
                "GLSL 140 not supported. Try {:?} or {:?}",
                display.get_opengl_version(),
                display.get_supported_glsl_version()
            );
        };

    // To quickly iterate on shaders without recompiling...
    /*let mut vert = String::new();
    let mut frag = String::new();
    let (vertex_shader, fragment_shader) = {
        use std::io::Read;

        let mut f1 = std::fs::File:: open("../ezgui/src/assets/vertex_140.glsl").unwrap();
        f1.read_to_string(&mut vert).unwrap();

        let mut f2 = std::fs::File:: open("../ezgui/src/assets/fragment_140.glsl").unwrap();
        f2.read_to_string(&mut frag).unwrap();

        (&vert, &frag)
    };*/

    let program = glium::Program::new(
        &display,
        glium::program::ProgramCreationInput::SourceCode {
            vertex_shader,
            tessellation_control_shader: None,
            tessellation_evaluation_shader: None,
            geometry_shader: None,
            fragment_shader,
            transform_feedback_varyings: None,
            // Without this, SRGB gets enabled and post-processes the color from the fragment
            // shader.
            outputs_srgb: true,
            uses_point_size: false,
        },
    )
    .unwrap();

    let dejavu: &[u8] = include_bytes!("assets/DejaVuSans.ttf");
    let screenspace_glyphs = GlyphBrush::new(&display, vec![Font::from_bytes(dejavu).unwrap()]);
    let mapspace_glyphs = GlyphBrushBuilder::using_font_bytes(dejavu)
        .params(glium::DrawParameters {
            blend: glium::Blend::alpha_blending(),
            depth: glium::Depth {
                test: glium::DepthTest::IfLessOrEqual,
                write: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .build(&display);

    let mut canvas = Canvas::new(
        settings.initial_dims.0,
        settings.initial_dims.1,
        screenspace_glyphs,
        mapspace_glyphs,
        settings.default_font_size,
    );
    let prerender = Prerender {
        display: &display,
        num_uploads: Cell::new(0),
        total_bytes_uploaded: Cell::new(0),
    };

    let gui = make_gui(&mut EventCtx {
        input: &mut UserInput::new(Event::NoOp, &mut canvas),
        canvas: &mut canvas,
        prerender: &prerender,
        program: &program,
    });

    let state = State { canvas, gui };

    loop_forever(
        state,
        events_loop,
        program,
        prerender,
        settings.profiling_enabled,
    );
}

fn loop_forever<G: GUI>(
    mut state: State<G>,
    mut events_loop: glutin::EventsLoop,
    program: glium::Program,
    prerender: Prerender,
    profiling_enabled: bool,
) {
    if profiling_enabled {
        #[cfg(feature = "profiler")]
        {
            cpuprofiler::PROFILER
                .lock()
                .unwrap()
                .start("./profile")
                .unwrap();
        }
    }

    let mut wait_for_events = false;

    let hidpi_factor = events_loop.get_primary_monitor().get_hidpi_factor();
    loop {
        let start_frame = Instant::now();

        let mut new_events: Vec<Event> = Vec::new();
        events_loop.poll_events(|event| {
            if let glutin::Event::WindowEvent { event, .. } = event {
                if event == glutin::WindowEvent::CloseRequested {
                    if profiling_enabled {
                        #[cfg(feature = "profiler")]
                        {
                            cpuprofiler::PROFILER.lock().unwrap().stop().unwrap();
                        }
                    }
                    state.gui.before_quit(&state.canvas);
                    process::exit(0);
                }
                if let Some(ev) = Event::from_glutin_event(event, hidpi_factor) {
                    new_events.push(ev);
                }
            }
        });
        if !wait_for_events {
            new_events.push(Event::Update);
        }

        let mut any_input_used = false;

        for event in new_events {
            let (new_state, mode, input_used) = state.event(event, &prerender, &program);
            if input_used {
                any_input_used = true;
            }
            state = new_state;
            wait_for_events = mode == EventLoopMode::InputOnly;
            match mode {
                EventLoopMode::ScreenCaptureEverything {
                    dir,
                    zoom,
                    max_x,
                    max_y,
                } => {
                    state = widgets::screenshot_everything(
                        &dir,
                        state,
                        &prerender.display,
                        &program,
                        &prerender,
                        zoom,
                        max_x,
                        max_y,
                    );
                }
                EventLoopMode::ScreenCaptureCurrentShot => {
                    widgets::screenshot_current(
                        &mut state,
                        &prerender.display,
                        &program,
                        &prerender,
                    );
                }
                _ => {}
            };
        }

        // Don't draw if an event was ignored and we're not in Animation mode. Every keypress also
        // fires a release event, most of which are ignored.
        if any_input_used || !wait_for_events {
            if any_input_used {
                // But if the event caused a state-change, the drawing state might be different
                // too. Need to recalculate what menu entries and such are valid. So send through
                // a no-op event.
                let (new_state, _, _) = state.event(Event::NoOp, &prerender, &program);
                state = new_state;
            }

            state.draw(&prerender.display, &program, &prerender, false);
            prerender.num_uploads.set(0);
        }

        // Primitive event loop.
        // TODO Read http://gameprogrammingpatterns.com/game-loop.html carefully.
        let this_frame = Instant::now().duration_since(start_frame);
        if SLEEP_BETWEEN_FRAMES > this_frame {
            thread::sleep(SLEEP_BETWEEN_FRAMES - this_frame);
        }
    }
}

fn ortho((left, right): (f32, f32), (bottom, top): (f32, f32)) -> [[f32; 4]; 4] {
    let s_x = 2.0 / (right - left) / (text::SCALE_DOWN as f32);
    let s_y = 2.0 / (top - bottom) / (text::SCALE_DOWN as f32);
    let t_x = -(right + left) / (right - left);
    let t_y = -(top + bottom) / (top - bottom);
    [
        [s_x, 0.0, 0.0, 0.0],
        [0.0, s_y, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [t_x, t_y, 0.0, 1.0],
    ]
}
