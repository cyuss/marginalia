//! What each menu entry actually runs.
//!
//! # The rule this module exists to keep
//!
//! The interface runs the tools; it does not reimplement them. Every device
//! action here shells out to a script in `tools/device/` or to the agent over
//! SSH — the same commands documented in `docs/USING_MARGINALIA.md`, with the
//! same checks, the same refusals and the same confirmations.
//!
//! That is deliberate. `install.sh` verifies a checksum and writes a manifest;
//! `reset.sh` refuses to remove anything outside `/home/root/.marginalia` and
//! makes you type `remove`. A second implementation of those rules living
//! behind a menu is a second place for them to be wrong. So there is one
//! implementation, and this is a front door to it.

/// Whether an action needs the real terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// Suspend the interface and hand the terminal over.
    ///
    /// Everything touching the device needs this: `ssh` prompts for the
    /// device's password, and `reset.sh` asks you to type `remove`. Capturing
    /// the output instead would hang on the first prompt with nothing on screen
    /// to explain why — and, worse, answering a safety prompt on your behalf is
    /// exactly what this interface must never do.
    HandOver,
    /// Runs here, shows its output in the pane. Local and read-only.
    Capture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Danger {
    /// Reads, or writes only inside Marginalia's own directory.
    Ordinary,
    /// Removes things. The interface asks before running it, and the script
    /// asks again.
    Removes,
}

pub struct Action {
    pub label: &'static str,
    /// One line, in plain words, shown under the menu.
    pub summary: &'static str,
    /// The shell command, run from the repository root.
    pub command: &'static str,
    pub terminal: Terminal,
    pub danger: Danger,
}

pub struct Section {
    pub title: &'static str,
    pub actions: &'static [Action],
}

pub const SECTIONS: &[Section] = &[
    Section {
        title: "Your reMarkable",
        actions: &[
            Action {
                label: "Check the device",
                summary: "Reads only. Reports firmware, free space, and what Marginalia will never touch.",
                command: "./tools/device/doctor.sh",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Show what installing would do",
                summary: "Every step of an install, performed on nothing.",
                command: "./tools/device/install.sh --dry-run",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Install or update",
                summary: "Builds for the device, copies into one directory, verifies by checksum.",
                command: "./tools/device/install.sh",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Ask the agent how it is",
                summary: "What it knows, and what it is permitted to do right now.",
                command: "ssh -t \"root@$RM_HOST\" '/home/root/.marginalia/bin/marginalia status'",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
        ],
    },
    Section {
        title: "Your reading",
        actions: &[
            Action {
                label: "List what you have highlighted",
                summary: "Every document with highlights, and how many.",
                command: "ssh -t \"root@$RM_HOST\" '/home/root/.marginalia/bin/marginalia highlights'",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Write the highlights to files",
                summary: "One Markdown file per document, inside Marginalia's own directory on the device.",
                command: "ssh -t \"root@$RM_HOST\" '/home/root/.marginalia/bin/marginalia highlights --export'",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Copy those files to this computer",
                summary: "Fetches them into ./highlights here. Nothing on the device is changed.",
                command: "scp -r \"root@$RM_HOST:/home/root/.marginalia/highlights/\" ./highlights && ls -1 ./highlights | head -n 40",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
        ],
    },
    Section {
        title: "Where documents come from",
        actions: &[
            Action {
                label: "Connect a Zotero library",
                summary: "Asks for an API key. Optional — a plain folder works as a source too.",
                command: "printf 'Zotero API key (from https://www.zotero.org/settings/keys): '; read -r key; \
                          ssh -t \"root@$RM_HOST\" \"/home/root/.marginalia/bin/marginalia zotero connect $key\"",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Bring metadata up to date",
                summary: "Titles, authors, collections, tags. Never moves a PDF.",
                command: "ssh -t \"root@$RM_HOST\" '/home/root/.marginalia/bin/marginalia sync'",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Forget the Zotero key",
                summary: "Removes it from the device. Does NOT revoke it at Zotero — do that yourself.",
                command: "ssh -t \"root@$RM_HOST\" '/home/root/.marginalia/bin/marginalia zotero disconnect'",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
        ],
    },
    Section {
        title: "Removing it",
        actions: &[
            Action {
                label: "List what removal would take",
                summary: "Reads only. Shows every file, and checks none is outside Marginalia's directory.",
                command: "./tools/device/reset.sh --dry-run",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "Remove Marginalia from the device",
                summary: "Returns your reMarkable to stock. You will be asked to type `remove`.",
                command: "./tools/device/reset.sh",
                terminal: Terminal::HandOver,
                danger: Danger::Removes,
            },
        ],
    },
    Section {
        title: "This project",
        actions: &[
            Action {
                label: "Run the tests",
                summary: "The whole suite, including the safety tests.",
                command: "cargo test --workspace --all-features",
                terminal: Terminal::HandOver,
                danger: Danger::Ordinary,
            },
            Action {
                label: "What it may and may never do",
                summary: "The safety policy, in short.",
                command: "cat docs/safety/DEVICE_WRITE_POLICY.md",
                terminal: Terminal::Capture,
                danger: Danger::Ordinary,
            },
        ],
    },
];

/// Flattened `(section, action)` pairs, in menu order.
pub fn flattened() -> Vec<(&'static Section, &'static Action)> {
    SECTIONS
        .iter()
        .flat_map(|s| s.actions.iter().map(move |a| (s, a)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_described_before_it_is_offered() {
        for (_, action) in flattened() {
            assert!(!action.label.is_empty());
            assert!(
                !action.summary.is_empty(),
                "{} has no summary; a menu entry that does not say what it does \
                 is how someone runs something they did not mean to",
                action.label
            );
            assert!(!action.command.is_empty());
        }
    }

    /// The interface must not grow its own copy of the install or removal
    /// logic. If one of these ever stops shelling out, this test says so.
    #[test]
    fn install_and_removal_go_through_the_scripts() {
        let commands: Vec<&str> = flattened().iter().map(|(_, a)| a.command).collect();
        assert!(commands
            .iter()
            .any(|c| c.contains("tools/device/install.sh")));
        assert!(commands.iter().any(|c| c.contains("tools/device/reset.sh")));
    }

    /// Anything that removes must be marked, because the marking is what makes
    /// the interface ask first.
    #[test]
    fn removal_is_the_only_thing_marked_dangerous_and_it_is_marked() {
        let dangerous: Vec<&str> = flattened()
            .iter()
            .filter(|(_, a)| a.danger == Danger::Removes)
            .map(|(_, a)| a.label)
            .collect();
        assert_eq!(dangerous, ["Remove Marginalia from the device"]);
    }

    /// A dry run that could modify something would be a lie.
    #[test]
    fn every_dry_run_is_actually_a_dry_run() {
        for (_, action) in flattened() {
            if action.label.contains("would") {
                assert!(
                    action.command.contains("--dry-run"),
                    "{} claims to change nothing but does not pass --dry-run",
                    action.label
                );
            }
        }
    }

    /// Every device action needs the real terminal, because ssh will ask for a
    /// password and a captured prompt is an invisible hang.
    #[test]
    fn anything_reaching_the_device_hands_over_the_terminal() {
        for (_, action) in flattened() {
            let reaches_device = action.command.contains("ssh")
                || action.command.contains("scp")
                || action.command.contains("tools/device/");
            if reaches_device {
                assert_eq!(
                    action.terminal,
                    Terminal::HandOver,
                    "{} talks to the device but would capture its output",
                    action.label
                );
            }
        }
    }
}
