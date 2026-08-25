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

use std::sync::mpsc::Receiver;

use brain_store::StorePaths;
use eframe::egui;

use crate::overlay::{Outcome, Overlay};

/// Roughly a sentence's worth, centred.
const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 96.0;

/// Run the overlay until the process is asked to stop.
pub fn run(paths: StorePaths, shows: Receiver<()>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([WIDTH, HEIGHT])
            .with_decorations(false)
            .with_always_on_top()
            .with_resizable(false)
            // Built now, shown later. The window exists from startup so that
            // the chord only ever has to change its visibility.
            .with_visible(false),
        ..Default::default()
    };

    eframe::run_native(
        "brain capture",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(paths, shows)))),
    )
}

struct App {
    overlay: Overlay,
    /// Whether the text field should be focused on the next frame.
    focus: bool,
    /// Text committed but not yet written, handed from the drawing pass to the
    /// next logic pass so the window closes first.
    pending: Option<String>,
}

impl App {
    fn new(paths: StorePaths, shows: Receiver<()>) -> App {
        App {
            overlay: Overlay::new(paths, shows),
            focus: false,
            pending: None,
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
        // The socket thread cannot wake the event loop, so the loop looks. A
        // tenth of a second is far below the point where a gesture stops
        // feeling immediate, and costs nothing while idle.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        if self.overlay.poll() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
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

        ui.add_space(8.0);

        let field = ui.add_sized(
            [ui.available_width(), 32.0],
            egui::TextEdit::singleline(self.overlay.text_mut())
                .hint_text("capture")
                .font(egui::TextStyle::Heading),
        );

        // The field takes focus the moment the window appears. A window that
        // needs a click before it accepts typing loses the first characters of
        // every capture.
        if self.focus {
            field.request_focus();
            self.focus = false;
        }

        if let Some(said) = self.overlay.said() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(said).weak());
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
