//! What the interface is currently showing, and what a keypress does to it.
//!
//! Deliberately free of terminal and process code so the whole model can be
//! tested by pressing keys and reading state.

use crate::action::{flattened, Action, Danger, Section};

pub const DEFAULT_HOST: &str = "10.11.99.1";

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// Moving around the menu.
    Browsing,
    /// Typing the device's address.
    EditingHost,
    /// "This removes things. Run it?"
    Confirming,
    /// Showing the output of something that ran here.
    Output,
}

pub struct App {
    pub mode: Mode,
    pub selected: usize,
    pub host: String,
    pub host_draft: String,
    pub output: String,
    pub status: String,
    pub should_quit: bool,
    /// Set when an action wants the terminal. The event loop takes it, drops
    /// out of the interface, runs it, and comes back.
    pub pending_handover: Option<&'static Action>,
}

impl App {
    pub fn new(host: String) -> Self {
        Self {
            mode: Mode::Browsing,
            selected: 0,
            host_draft: host.clone(),
            host,
            output: String::new(),
            status: String::new(),
            should_quit: false,
            pending_handover: None,
        }
    }

    pub fn entries(&self) -> Vec<(&'static Section, &'static Action)> {
        flattened()
    }

    pub fn current(&self) -> Option<&'static Action> {
        self.entries().get(self.selected).map(|(_, a)| *a)
    }

    pub fn move_down(&mut self) {
        let n = self.entries().len();
        if n > 0 {
            self.selected = (self.selected + 1) % n;
        }
    }

    pub fn move_up(&mut self) {
        let n = self.entries().len();
        if n > 0 {
            self.selected = (self.selected + n - 1) % n;
        }
    }

    /// Enter on the highlighted entry.
    ///
    /// Anything that removes stops here for a confirmation first. That is the
    /// interface's own question; the script asks its own afterwards, and this
    /// one never answers it.
    pub fn activate(&mut self) {
        let Some(action) = self.current() else { return };
        match action.danger {
            Danger::Removes => self.mode = Mode::Confirming,
            Danger::Ordinary => self.run(action),
        }
    }

    pub fn confirm(&mut self) {
        self.mode = Mode::Browsing;
        if let Some(action) = self.current() {
            self.run(action);
        }
    }

    pub fn cancel(&mut self) {
        self.mode = Mode::Browsing;
        self.status = "Cancelled. Nothing was run.".into();
    }

    fn run(&mut self, action: &'static Action) {
        self.pending_handover = Some(action);
    }

    pub fn begin_editing_host(&mut self) {
        self.host_draft = self.host.clone();
        self.mode = Mode::EditingHost;
    }

    pub fn commit_host(&mut self) {
        let trimmed = self.host_draft.trim();
        if trimmed.is_empty() {
            self.status = "An address is required; keeping the previous one.".into();
        } else {
            self.host = trimmed.to_string();
            self.status = format!("Device address set to {}", self.host);
        }
        self.mode = Mode::Browsing;
    }

    pub fn show_output(&mut self, text: String) {
        self.output = text;
        self.mode = Mode::Output;
    }

    pub fn dismiss_output(&mut self) {
        self.mode = Mode::Browsing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new(DEFAULT_HOST.to_string())
    }

    #[test]
    fn moving_wraps_at_both_ends() {
        let mut a = app();
        let n = a.entries().len();
        a.move_up();
        assert_eq!(a.selected, n - 1);
        a.move_down();
        assert_eq!(a.selected, 0);
    }

    /// The property the whole interface rests on: a destructive entry cannot be
    /// run by one keypress.
    #[test]
    fn a_removal_asks_before_it_runs() {
        let mut a = app();
        let index = a
            .entries()
            .iter()
            .position(|(_, act)| act.danger == Danger::Removes)
            .expect("there is a removal action");
        a.selected = index;

        a.activate();
        assert_eq!(a.mode, Mode::Confirming);
        assert!(
            a.pending_handover.is_none(),
            "the removal was queued before it was confirmed"
        );

        a.confirm();
        assert!(a.pending_handover.is_some());
    }

    #[test]
    fn declining_runs_nothing() {
        let mut a = app();
        a.selected = a
            .entries()
            .iter()
            .position(|(_, act)| act.danger == Danger::Removes)
            .unwrap();
        a.activate();
        a.cancel();

        assert_eq!(a.mode, Mode::Browsing);
        assert!(a.pending_handover.is_none());
        assert!(a.status.contains("Nothing was run"));
    }

    #[test]
    fn an_ordinary_action_runs_without_a_prompt() {
        let mut a = app();
        a.selected = a
            .entries()
            .iter()
            .position(|(_, act)| act.danger == Danger::Ordinary)
            .unwrap();
        a.activate();
        assert_eq!(a.mode, Mode::Browsing);
        assert!(a.pending_handover.is_some());
    }

    #[test]
    fn the_device_address_can_be_changed() {
        let mut a = app();
        a.begin_editing_host();
        a.host_draft = "192.168.1.42".into();
        a.commit_host();
        assert_eq!(a.host, "192.168.1.42");
        assert_eq!(a.mode, Mode::Browsing);
    }

    /// An empty address would produce `ssh root@` and a baffling error.
    #[test]
    fn an_empty_address_is_refused_and_the_old_one_kept() {
        let mut a = app();
        a.begin_editing_host();
        a.host_draft = "   ".into();
        a.commit_host();
        assert_eq!(a.host, DEFAULT_HOST);
        assert!(a.status.contains("required"));
    }

    #[test]
    fn output_is_shown_and_then_dismissed() {
        let mut a = app();
        a.show_output("some text".into());
        assert_eq!(a.mode, Mode::Output);
        a.dismiss_output();
        assert_eq!(a.mode, Mode::Browsing);
    }
}
