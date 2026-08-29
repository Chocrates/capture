//! The eframe shell around the overlay.
//!
//! Everything about *what the overlay does* lives in `overlay.rs`, which has no
//! dependency on a toolkit and is tested without one. This file is the part
//! that cannot be tested without a display: turning a frame into pixels, and
//! turning keystrokes into the two decisions the overlay understands.
//!
//! # Hidden, not closed
//!
//! Dismissing sets the window invisible and leaves the process running with its
//! GPU context, fonts and layout intact. Closing it would return the next
//! capture to the cost this whole design exists to avoid.
//!
//! # Why this window is deliberately an X11 window
//!
//! Wayland gives a client no say over three things this overlay is built out
//! of. In winit's Wayland backend `set_visible` is a documented no-op ("Not
//! possible on Wayland"), `focus_window` is an empty function, and
//! `set_window_level` — always-on-top — is another. That is not a gap to work
//! around: it is the protocol's position that showing, raising and focusing
//! windows belong to the compositor, and a client that could take the keyboard
//! whenever it liked is exactly what the design forbids.
//!
//! Taken together those three no-ops describe a window that appears at login
//! and never leaves, sits behind whatever is in front of it, and cannot be
//! typed into — which is what happens if this runs natively on Wayland.
//!
//! So it asks winit for X11 and gets it through XWayland, where all three are
//! real: a mapped/unmapped toplevel, `_NET_WM_STATE_ABOVE`, and
//! `_NET_ACTIVE_WINDOW`. The cost is XWayland's scaling on a fractional-scale
//! display; the alternative is no overlay at all.
//!
//! # The two rows under the field
//!
//! Both come from `brain-parse`, which is where `DESIGN.md` §3.1.5 lives as
//! code. Nothing about the language is written down twice: this window shows
//! the sigils and shapes the recognizer will actually accept, or it shows
//! nothing, and it cannot drift from them by being edited on its own.
//!
//! `DESIGN.md` calls sigils an accelerant rather than an interface, and these
//! rows keep them that way — visible at the moment they could be used, never
//! required, and never treated as syntax here. What is typed is stored exactly
//! as typed, sigils and all, whether or not anything later recognises them.
//! Today nothing does: the recognizer is M2.

use std::sync::mpsc::Receiver;

use brain_store::StorePaths;
use eframe::egui;

use crate::overlay::{Outcome, Overlay};

/// Roughly a sentence's worth, centred.
const WIDTH: f32 = 720.0;
/// Tall enough to hold the whole language underneath the field.
///
/// This is not the size a capture overlay wants to be, and it is deliberate
/// while the grammar is being worked on: the whole of §3.1.5 is on screen so
/// that what the recognizer will accept can be read against what one is
/// actually typing. The reference scrolls, so nothing is lost if it outgrows
/// this — but a window this size costs the gesture the very unobtrusiveness the
/// design spends its budget on, and it should shrink back to the field and its
/// two summary rows once the vocabulary settles.
const HEIGHT: f32 = 620.0;

/// The shell's own search surface, more or less: a dark sheet, one rounded
/// field, and nothing else competing for the eye.
mod look {
    use eframe::egui::Color32;

    /// The sheet the field sits on.
    pub const BACKDROP: Color32 = Color32::from_rgb(0x24, 0x24, 0x24);
    /// The field itself, a shade up from the sheet so it reads as inset.
    pub const FIELD: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
    /// What is being typed.
    pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
    /// The prompt, and the magnifier: present, but not asking to be read.
    pub const MUTED: Color32 = Color32::from_rgb(0x9A, 0x9A, 0x9A);
    /// Dimmer still — the legend, which is there to be glanced at and
    /// otherwise ignored.
    pub const FAINT: Color32 = Color32::from_rgb(0x6E, 0x6E, 0x6E);
    /// The sigil characters themselves, the one place colour is spent.
    pub const SIGIL: Color32 = Color32::from_rgb(0x7C, 0xA8, 0xD8);
    /// The dots between the shapes — separators, not content.
    pub const DIVIDER: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);

    /// How wide the field is allowed to get, whatever the window does.
    pub const FIELD_WIDTH: f32 = 380.0;
    /// Its height, and — being a pill — twice its corner radius.
    pub const FIELD_HEIGHT: f32 = 32.0;
}

/// A handle the socket thread uses to wake the event loop.
///
/// The window is built by eframe, so its context does not exist until after the
/// socket is already listening. This is the slot the context lands in once it
/// does — empty before that, which is harmless: a request arriving in the
/// window between binding and the first frame is still waiting on the channel
/// when the loop starts.
#[derive(Clone, Default)]
pub struct Waker(std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>);

impl Waker {
    /// Wake the event loop, if there is one yet.
    pub fn wake(&self) {
        if let Ok(guard) = self.0.lock() {
            match guard.as_ref() {
                Some(ctx) => {
                    tracing::debug!("waking the event loop");
                    ctx.request_repaint();
                }
                None => tracing::debug!("no context yet; the loop will find it on the channel"),
            }
        }
    }

    fn hold(&self, ctx: &egui::Context) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(ctx.clone());
        }
    }
}

/// Run the overlay until the process is asked to stop.
/// How much bigger than the drawn sizes everything is.
///
/// Read once, here, and never again. The gesture's budget is spent on the
/// window already existing when the chord is pressed (§3.1), so a preference
/// consulted per frame — or worse, per keystroke — would be paying for
/// legibility out of exactly the wrong pocket.
///
/// A store that cannot be read gives the compiled-in default rather than an
/// error. An overlay that refused to open because a preference was unavailable
/// would have traded a legible window for no window.
fn text_scale(paths: &StorePaths) -> f32 {
    let store = brain_store::ops::Ops::open_read(&paths.ops)
        .and_then(|ops| ops.generation())
        .and_then(|g| {
            brain_store::projection::Store::open_read(&brain_store::projection::generation_path(
                &paths.state,
                g,
            ))
        })
        .ok();

    brain_store::settings::number(store.as_ref(), "capture.text_scale")
}

pub fn run(paths: StorePaths, shows: Receiver<()>, waker: Waker) -> eframe::Result {
    let scale = text_scale(&paths);
    let options = eframe::NativeOptions {
        event_loop_builder: Some(Box::new(force_x11)),
        viewport: egui::ViewportBuilder::default()
            // Scaled with the text. The reference below the field scrolls, so
            // nothing is lost if it does not fit — but a field whose text has
            // grown and whose box has not is a field you cannot see the end of
            // what you are typing in.
            .with_inner_size([WIDTH * scale, HEIGHT * scale])
            .with_decorations(false)
            .with_always_on_top()
            .with_resizable(false)
            // Not a window in the ordinary sense: no entry in the switcher, no
            // place in the taskbar, nothing to alt-tab into. It behaves like
            // the shell's own overview — something that is either in front of
            // you or not there at all.
            .with_taskbar(false)
            .with_active(true)
            // Built now, shown later. The window exists from startup so that
            // the chord only ever has to change its visibility, which is
            // exactly how the desktop's own overlay behaves: nothing is
            // launched when the key is pressed, something already loaded is
            // revealed.
            .with_visible(false),
        ..Default::default()
    };

    eframe::run_native(
        "brain capture",
        options,
        Box::new(move |cc| {
            install_a_font(&cc.egui_ctx);
            waker.hold(&cc.egui_ctx);
            Ok(Box::new(App::new(paths, shows, scale)))
        }),
    )
}

/// Where to look for something to draw text with.
///
/// In preference order, and every one of them a file that is present on a
/// desktop that has any text on it at all. The list is short on purpose: this
/// is not font selection, it is the difference between a window that can show
/// what was typed and one that cannot.
const FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/liberation-sans/LiberationSans-Regular.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
];

/// Where to look for a fixed-width face, for the grammar.
const MONO_FONTS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
];

/// Give egui something to lay text out with.
///
/// This build of eframe is taken without its default features, and one of the
/// things that turns off is `default_fonts` — the embedded face egui otherwise
/// falls back on. Without it egui does not fail loudly; it lays out every
/// string into an empty galley, which is far worse. The field draws as a bare
/// line, typing appears to do nothing, and the cursor — clamped each frame
/// against a galley with no characters in it — returns to zero between
/// keystrokes, so a note typed forwards is stored backwards.
///
/// The font is read at startup, before the window is ever shown, so this costs
/// nothing at the moment the chord is pressed.
fn install_a_font(ctx: &egui::Context) {
    let Some((path, bytes)) = FONTS.iter().find_map(|p| {
        std::fs::read(p)
            .inspect_err(|e| tracing::debug!(path = p, %e, "no font here"))
            .ok()
            .map(|b| (*p, b))
    }) else {
        // Not fatal, and not silent. The overlay still runs, still captures
        // from the socket and still stores what is committed — but it cannot
        // show any of it, and that is worth saying out loud rather than
        // leaving someone to work out from an empty window.
        tracing::error!("no usable font on this system; the overlay cannot draw text");
        return;
    };

    tracing::debug!(path, "loaded a font");

    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "system".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("system".to_owned());
    }

    // A grammar set in a proportional face is a grammar whose alignment lies.
    // Not fatal if it is missing — the fallback above already covers the
    // monospace family — so this is attempted and dropped rather than checked.
    if let Some(mono) = MONO_FONTS.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert(
            "mono".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(mono)),
        );
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "mono".to_owned());
    }

    ctx.set_fonts(fonts);
}

/// Take the X11 backend even when the session is Wayland.
///
/// winit picks Wayland whenever `WAYLAND_DISPLAY` is set, and under that
/// backend this window cannot be hidden, raised or focused — see the module
/// docs. Asking for X11 routes it through XWayland, which supports all three.
///
/// If there is no X server to reach, this is left alone: the event loop will
/// come up on Wayland and the overlay will be degraded but running, which is a
/// better answer than refusing to start.
#[cfg(all(unix, not(target_os = "macos")))]
fn force_x11(builder: &mut winit::event_loop::EventLoopBuilder<eframe::UserEvent>) {
    use winit::platform::x11::EventLoopBuilderExtX11 as _;

    let x11 = std::env::var("DISPLAY").is_ok_and(|d| !d.is_empty());
    if !x11 {
        tracing::warn!(
            "no X display; the overlay will run on Wayland, where it cannot hide, \
             raise itself or take the keyboard"
        );
        return;
    }
    tracing::debug!("using the X11 backend so the window can hide, raise and focus");
    builder.with_x11();
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn force_x11(_builder: &mut winit::event_loop::EventLoopBuilder<eframe::UserEvent>) {}

struct App {
    overlay: Overlay,
    /// Whether the text field should be focused on the next frame.
    focus: bool,
    /// Text committed but not yet written, handed from the drawing pass to the
    /// next logic pass so the window closes first.
    pending: Option<String>,
    /// How many logic passes are left in which to undo eframe's opening reveal.
    settling: u8,
    /// How much bigger than the drawn sizes everything is. Read once at
    /// startup, so nothing on the gesture's path consults a preference.
    scale: f32,
}

/// How many passes to spend taking the window back off the screen at startup.
///
/// eframe reveals the window itself once it has painted its first frame —
/// `post_rendering` calls `set_visible(true)` unconditionally, which is how it
/// avoids showing an unpainted window and which quietly overrules the
/// `with_visible(false)` this one was built with. Undoing that costs one pass;
/// the extra two are there so it cannot be lost to a frame that arrives in an
/// unexpected order, and they cost three no-op X requests at login.
const SETTLING: u8 = 3;

impl App {
    fn new(paths: StorePaths, shows: Receiver<()>, scale: f32) -> App {
        App {
            overlay: Overlay::new(paths, shows),
            focus: false,
            pending: None,
            settling: SETTLING,
            scale,
        }
    }
}

impl eframe::App for App {
    /// Runs even while the window is hidden.
    ///
    /// That is what makes a resident overlay possible: eframe stops running ui
    /// passes on a hidden window, so anything that has to keep watching — the
    /// socket, here — has to live somewhere that still gets called.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The scale, applied where egui takes it: one call that grows the text,
        // the spacing and the widgets together. The alternative was threading a
        // factor through every draw function and multiplying each of four
        // hardcoded sizes by hand, which is four chances to forget one and no
        // way to see that it happened except by looking.
        //
        // Idempotent, and egui only rebuilds fonts when the value changes.
        if ctx.zoom_factor() != self.scale {
            ctx.set_zoom_factor(self.scale);
        }

        // Nothing is polled here, and that is the point. An earlier version
        // asked for a repaint every hundred milliseconds so it could check the
        // channel — ten wakeups a second, forever, on a process whose whole
        // purpose is to sit still until a key is pressed. It cost a measurable
        // fraction of a core and would have shortened a laptop's battery life
        // for no observable benefit.
        //
        // The socket thread wakes the loop instead, through a `Waker` it holds.
        // Between presses this process is genuinely asleep.
        // A safety net, not the mechanism.
        //
        // The socket thread wakes this loop directly through `Waker`, and when
        // that works the chord is answered immediately. It is not guaranteed to
        // work on every compositor — a hidden window may receive no frame
        // callbacks at all — so a slow poll sits underneath it.
        //
        // Half a second is chosen against two failure modes. Too fast and the
        // process burns a measurable fraction of a core forever doing nothing;
        // an earlier version polled ten times a second and cost 0.4% of a core
        // continuously, on a program whose entire job is to sit still. Too slow
        // and a compositor where the wake does not work leaves the gesture
        // feeling broken rather than late.
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        // Asserted every pass rather than only on the passes that change it.
        // eframe shows this window on its own the moment it has painted (see
        // `SETTLING`), so "hidden" is not a state that can be set once and
        // relied upon — it is a state that has to keep being true.
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.overlay.visible()));

        // The opening reveal happens after the first frame is painted, so it
        // cannot be undone until the pass after that. These few immediate
        // repaints turn what would otherwise be half a second of window on
        // screen at every login into a few milliseconds.
        if self.settling > 0 {
            self.settling -= 1;
            ctx.request_repaint();
        }

        if self.overlay.poll() {
            tracing::debug!("showing the window");
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            // Re-stated on every appearance. A window that is mapped and
            // unmapped over and over does not reliably keep the state the
            // window manager was told about at creation, and a capture bar
            // behind the window being captured from is no bar at all.
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.focus = true;
        }

        // Storing happens here rather than in the drawing pass, so the window
        // is already gone by the time the disk is touched. Waiting for the
        // store would make the gesture's duration depend on it, which is what
        // the spool exists to prevent.
        if let Some(text) = self.pending.take() {
            self.overlay.store(&text);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.overlay.visible() {
            return;
        }

        // Measured before anything else in the frame: this is the first frame
        // of an appearance, and the appearance is what was being timed.
        self.overlay.drawn();

        // The sheet. The `Ui` eframe hands over has no background of its own,
        // so without this the window shows whatever the framebuffer last held.
        ui.painter().rect_filled(ui.max_rect(), 0.0, look::BACKDROP);

        ui.add_space(14.0);

        let field = ui.vertical_centered(|ui| self.search_field(ui)).inner;

        // The field takes focus the moment the window appears. A window that
        // needs a click before it accepts typing loses the first characters of
        // every capture.
        if self.focus {
            field.request_focus();
            self.focus = false;
        }

        ui.add_space(10.0);
        ui.vertical_centered(sigil_row);
        ui.add_space(3.0);
        ui.vertical_centered(shape_row);
        ui.add_space(10.0);
        reference(ui);

        if let Some(said) = self.overlay.said() {
            ui.add_space(4.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(said).color(look::FAINT).size(11.0));
            });
        }

        // Enter commits, Escape discards, and nothing else is interpreted.
        // There is no other decision to make here, and every additional one is
        // a decision at the moment the user is trying not to make any.
        let outcome = if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.overlay.commit()
        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            Some(self.overlay.discard())
        } else {
            None
        };

        match outcome {
            Some(Outcome::Captured(text)) => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(false));
                // Handed to the next logic pass rather than written here.
                self.pending = Some(text);
            }
            Some(Outcome::Discarded) => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            None => {}
        }
    }
}

impl App {
    /// One rounded field, and nothing in front of it.
    ///
    /// The pill is drawn rather than configured: egui's own text field frame
    /// cannot be made a capsule, and the shape is most of what makes this read
    /// as the shell's search rather than as a form.
    fn search_field(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let width = look::FIELD_WIDTH.min(ui.available_width() - 32.0);
        let size = egui::vec2(width, look::FIELD_HEIGHT);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same((look::FIELD_HEIGHT / 2.0) as u8),
            look::FIELD,
        );

        // Inside the pill: no frame of its own, no margin fighting the one the
        // pill already provides.
        let inner = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 16.0, rect.top()),
            egui::pos2(rect.right() - 16.0, rect.bottom()),
        );

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child.visuals_mut().override_text_color = Some(look::TEXT);
        child.style_mut().visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        child.add_sized(
            inner.size(),
            egui::TextEdit::singleline(self.overlay.text_mut())
                .frame(egui::Frame::NONE)
                .background_color(egui::Color32::TRANSPARENT)
                .margin(egui::Margin::ZERO)
                .hint_text(egui::RichText::new("Type to capture").color(look::MUTED))
                .font(egui::FontId::proportional(15.0))
                .vertical_align(egui::Align::Center),
        )
    }
}

/// The row of sigils under the field.
///
/// A glance, not a lesson: each is its marker and one example of what follows
/// it, and none of them has to be used for the capture to work.
fn sigil_row(ui: &mut egui::Ui) {
    let width: f32 = brain_parse::SIGILS
        .iter()
        .map(|s| 20.0 + s.example.len() as f32 * 5.5)
        .sum();

    centred_row(ui, width, |ui| {
        for sigil in brain_parse::SIGILS {
            ui.label(tiny(&sigil.sigil.to_string(), look::SIGIL));
            ui.label(tiny(sigil.example, look::FAINT));
            ui.add_space(7.0);
        }
    });
}

/// The row of shapes under the sigils.
///
/// Examples rather than rules, because §3.1.1 settles that the input language
/// is English and there is nothing to get wrong. A person reading `friday 2pm`
/// learns everything they need about how to write a date here, and learns it
/// without being told there is a syntax — which there is not.
fn shape_row(ui: &mut egui::Ui) {
    let width: f32 = brain_parse::SHAPES
        .iter()
        .map(|s| 14.0 + s.len() as f32 * 5.0)
        .sum();

    centred_row(ui, width, |ui| {
        for (i, shape) in brain_parse::SHAPES.iter().enumerate() {
            if i > 0 {
                ui.label(tiny("·", look::DIVIDER));
                ui.add_space(3.0);
            }
            ui.label(tiny(shape, look::FAINT));
            ui.add_space(3.0);
        }
    });
}

/// The whole of the language, under the field.
///
/// A working surface rather than a finished one, and the module docs say so:
/// this exists so the vocabulary can be read against what is being typed while
/// it is still being decided. Everything in it comes from `brain-parse` — the
/// classes, the grammar, the guards and the bands — so it cannot describe a
/// language the crate does not hold.
fn reference(ui: &mut egui::Ui) {
    // The reference is the only thing in the window that runs edge to edge, so
    // it supplies its own margin rather than the window carrying one that the
    // centred field above would then have to fight.
    let inset = egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 0));

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            inset.show(ui, |ui| {
                ui.add_space(2.0);

                heading(ui, "vocabulary — every word tier 2 knows");
                for (class, words) in brain_parse::CLASSES {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(mono(&format!("{class:<12}"), look::SIGIL));
                        let joined: Vec<&str> = words.iter().map(|w| w.word).collect();
                        ui.label(tiny(&joined.join(" "), look::FAINT));
                    });
                }

                heading(ui, "sigils — always win over anything inferred");
                for sigil in brain_parse::SIGILS {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(mono(
                            &format!("{}{:<11}", sigil.sigil, sigil.example),
                            look::SIGIL,
                        ));
                        ui.label(tiny(sigil.means, look::FAINT));
                    });
                }

                heading(ui, "grammar");
                ui.label(mono(brain_parse::EBNF, look::MUTED));

                heading(ui, "guards — what stops a match");
                for guard in brain_parse::GUARDS {
                    ui.label(tiny(&format!("· {guard}"), look::FAINT));
                }

                heading(
                    ui,
                    &format!(
                        "confidence — below {:.2} it is a suggestion, not a promotion",
                        brain_parse::CONFIDENCE_FLOOR
                    ),
                );
                for (score, shape) in brain_parse::BANDS {
                    let promotes =
                        score.parse::<f32>().unwrap_or(0.0) >= brain_parse::CONFIDENCE_FLOOR;
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(mono(
                            &format!("{score:<6}"),
                            if promotes { look::SIGIL } else { look::DIVIDER },
                        ));
                        ui.label(tiny(shape, look::FAINT));
                    });
                }

                ui.add_space(8.0);
            });
        });
}

/// One line naming what follows it.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(9.0);
    ui.label(
        egui::RichText::new(text)
            .color(look::MUTED)
            .font(egui::FontId::proportional(11.0)),
    );
    ui.add_space(2.0);
}

/// Fixed width, for anything whose columns carry meaning.
fn mono(text: &str, colour: egui::Color32) -> egui::RichText {
    egui::RichText::new(text)
        .color(colour)
        .font(egui::FontId::monospace(10.5))
}

/// The one text size anything below the field is allowed to be.
fn tiny(text: &str, colour: egui::Color32) -> egui::RichText {
    egui::RichText::new(text)
        .color(colour)
        .font(egui::FontId::proportional(10.5))
}

/// Lay a row out from the middle rather than from the left.
///
/// `vertical_centered` centres each widget it is given, which for a row of them
/// means a column. Laying the row out horizontally and padding it by half the
/// difference is what actually puts it under the middle of the field.
fn centred_row(ui: &mut egui::Ui, width: f32, row: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        ui.spacing_mut().item_spacing.x = 3.0;
        row(ui);
    });
}
