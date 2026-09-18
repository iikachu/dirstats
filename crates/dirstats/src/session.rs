// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! Whether a window can be shown, so the binary can pick the terminal
//! interface on a Linux console or over SSH instead of failing to open one.

/// Reads one environment variable; a stand-in in tests.
type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Whether this session looks able to show a window.
///
/// This is a guess from the environment, not a promise: a stale `DISPLAY`
/// passes it, so callers still fall back when the window fails to open.
#[must_use]
pub fn gui_available() -> bool {
    gui_available_in(&|name| std::env::var(name).ok())
}

fn gui_available_in(env: Env) -> bool {
    let set = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    // A window from an SSH session opens on the machine's own desktop, or
    // on none (Windows runs it where nobody sees it), not in front of the
    // person typing. X11 forwarding sets DISPLAY and is honoured below.
    let ssh = set("SSH_CONNECTION") || set("SSH_CLIENT") || set("SSH_TTY");
    if cfg!(any(target_os = "macos", windows)) {
        !ssh
    } else {
        // Linux and the BSDs: no display server means a text console,
        // SSH without forwarding, or a container.
        set("WAYLAND_DISPLAY") || set("DISPLAY")
    }
}

/// Whether standard input and output are both a terminal, which the
/// terminal interface needs.
#[must_use]
pub fn is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::gui_available_in;

    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn local_desktop() {
        let vars: &[_] = if cfg!(any(target_os = "macos", windows)) { &[] } else { &[("WAYLAND_DISPLAY", "wayland-0")] };
        assert!(gui_available_in(&env(vars)));
    }

    #[test]
    fn plain_ssh_has_no_gui() {
        assert!(!gui_available_in(&env(&[("SSH_CONNECTION", "10.0.0.2 5000 10.0.0.1 22")])));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn unix_console_and_forwarding() {
        assert!(!gui_available_in(&env(&[])));
        assert!(!gui_available_in(&env(&[("DISPLAY", "")])));
        assert!(gui_available_in(&env(&[("SSH_TTY", "/dev/pts/0"), ("DISPLAY", "localhost:10.0")])));
    }
}
