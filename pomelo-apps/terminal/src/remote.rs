//! A session on the machine at the other end of the network: the bytes, and the page they drew.
//!
//! The app has two shells and they are not the same kind of thing. [`crate::shell`] is a shell *in this
//! app*: it parses a line this crate typed and answers with a [`String`], and nothing about it is
//! asynchronous. This is the other one — a shell on a NAS, behind an SSH client, on the far side of a
//! network — and everything about it is asynchronous: it is opened, it answers when it feels like it, it
//! can be lost, and it prints bytes rather than text.
//!
//! # One line at a time, and why that is enough
//!
//! What is typed here is kept as a line and sent as a line ([`Remote::run`]) rather than being streamed
//! a keystroke at a time, and the page is fed with whatever the machine has said ([`Remote::poll`]).
//! That is a terminal over a *pseudo-terminal*, which sounds like it should need a byte stream in both
//! directions and does not: a pseudo-terminal's other end is a shell with line editing in it, so it
//! echoes and redraws what it receives by itself. Sending `ls\r` gets back `ls` and a listing, which is
//! what a terminal is supposed to show — and the echo coming from the machine rather than from this app
//! is what keeps a session's echo and a session's output in one order.
//!
//! So the prompt is not this app's either. A real shell prints `user@host:cwd$ ` before it reads, that
//! reaches [`Screen`] as bytes like everything else, and what is being typed is drawn after it. The page
//! is the machine's; what this app draws is the frame around it.
//!
//! # A session is a session and not a reading
//!
//! Deliberately a *second* connection to the same machine, and the backend is built that way: the page
//! watching the numbers keeps watching them at its own interval while somebody types in here. The fault
//! states are the same ones — [`NasFault`] — but the meanings are this app's: a session that has not
//! answered *yet* is [`NasShellStatus::Opening`] rather than broken, and an `exit` is
//! [`NasShellStatus::Closed`] rather than a failure. See [`crate::screen`] for what the bytes do to the
//! page once they arrive.
//!
//! The backend is the one dependency, and it is taken as [`NasBackend`] rather than as a board: this
//! module's subject is the far side of a wire, and a test that wants a machine that is off should not
//! have to build a whole board to get one.

use pomelo_hal::{HalError, NasBackend, NasCredentials, NasFault, NasShellStatus};

use crate::screen::Screen;
use crate::shell::KeyAction;

/// A session on `host`, as the app has it: what it is called, where it stands, and what it has drawn.
///
/// The password is *not* here, deliberately. It went into the SSH client and stays behind
/// [`NasBackend`]; what is left on this side is the address to name on the page and the account to draw
/// with, and a page's state is the wrong place for a credential even when the file it came from keeps
/// one in the clear.
pub struct Remote {
    /// The machine, as the credentials that opened it name it.
    host: String,
    /// The account it was opened as. For the words on the page only — nothing here logs in.
    user: String,
    /// Where the session stands, as the last word the backend had on it.
    status: NasShellStatus,
    /// The page the machine's bytes have been drawn into.
    screen: Screen,
    /// The line being typed, which is this app's copy and not the machine's: it is what is drawn at the
    /// prompt, and the machine's own echo is what ends up on the page.
    input: String,
}

impl Remote {
    /// Opens a session on `device` and draws it into a page `columns` × `rows` cells.
    ///
    /// A failure to open is *not* an error here, because there is nothing the caller could usefully do
    /// with one: it is a state the page reports — a board with no SSH client, a machine that is off —
    /// and the same one the backend would have reached a second later by itself. What it is not is
    /// unconfigured: this session was asked for, so it is either opening, up, or gone.
    pub fn open(nas: &mut dyn NasBackend, device: &NasCredentials, columns: u16, rows: u16) -> Self {
        let status = match nas.shell_open(&device.host, columns, rows) {
            Ok(()) => NasShellStatus::Opening,
            Err(error) => NasShellStatus::Lost(fault_of(error)),
        };

        Self {
            host: device.host.clone(),
            user: device.user.clone(),
            status,
            screen: Screen::new(),
            input: String::new(),
        }
    }

    /// The machine, as the page names it.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The account it was opened as.
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Where the session stands.
    pub fn status(&self) -> NasShellStatus {
        self.status
    }

    /// Whether the session is *there*: opening, or up and answering.
    ///
    /// What this is for is deciding whether it is worth asking again — see [`Remote::poll`] — so a
    /// session that has said goodbye and one that was lost answer the same thing: both have nothing more
    /// to say.
    pub fn is_live(&self) -> bool {
        matches!(
            self.status,
            NasShellStatus::Opening | NasShellStatus::Ready
        )
    }

    /// The page the machine drew.
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// The line being typed.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Types one key at the session.
    ///
    /// Nothing goes anywhere unless the session is live: a key pressed at a session that has been lost or
    /// has said goodbye is not an error to report, it is a key with nowhere to go, and a page that
    /// complained about every one would be shouting at somebody who is only trying to see whether it is
    /// still there.
    pub fn key(&mut self, nas: &mut dyn NasBackend, action: KeyAction) {
        if !self.is_live() {
            return;
        }

        match action {
            KeyAction::Char(ch) => self.input.push(ch),
            KeyAction::Space => self.input.push(' '),
            KeyAction::Backspace => {
                self.input.pop();
            }
            KeyAction::Enter => self.run(nas),
            // The keyboard's mode is the app's, and both of its shells type on the same one keyboard.
            KeyAction::SwitchMode(_) => {}
        }
    }

    /// Takes whatever the machine has said since the last look, and where it says the session stands.
    ///
    /// Both in one call and in one order, because the backend keeps them together: what a session said
    /// and whether it is still there are one answer, and taking them apart would let the app draw the
    /// output of a session it had already decided was gone.
    pub fn poll(&mut self, nas: &mut dyn NasBackend) {
        let said = nas.shell_take();
        self.status = said.status;

        if !said.output.is_empty() {
            self.screen.feed(&said.output);
        }
    }

    /// Sends the line that has been typed, and forgets it.
    ///
    /// Forgotten, because what comes back is the machine's echo of it and a terminal that drew its own
    /// copy as well would have every line twice. A carriage return rather than a newline: that is what a
    /// terminal's Return key sends and what a pseudo-terminal's line discipline acts on. An empty line is
    /// sent as the return alone, which is a Return pressed at a prompt and not nothing.
    fn run(&mut self, nas: &mut dyn NasBackend) {
        let mut bytes = std::mem::take(&mut self.input).into_bytes();
        bytes.push(b'\r');

        // A write that fails is a session that has just gone, and there is nothing to say about it here:
        // the next poll asks the backend where it stands and the page draws that. Reporting it twice, in
        // two different voices, is how a page ends up contradicting itself.
        let _ = nas.shell_write(&bytes);
    }
}

/// The fault a refused call to a machine is reported as.
///
/// [`HalError`] is about *this* board — an argument that made no sense, a backend that cannot do this at
/// all — and [`NasFault`] is about the machine, which is what the page has words for. The two overlap at
/// one end and not at the other, so what lands here is the machine's reading of it: a board with no SSH
/// client is [`NasFault::Unsupported`], and anything else a call was refused on is a machine that could
/// not be reached, because from the page's side there is one machine and it is not answering.
fn fault_of(error: HalError) -> NasFault {
    match error {
        HalError::NotSupported => NasFault::Unsupported,
        _ => NasFault::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_hal::sim::SimNas;

    /// A machine this backend has been told about.
    fn device() -> NasCredentials {
        NasCredentials {
            host: String::from("192.168.1.10"),
            port: 22,
            user: String::from("nasstat"),
            password: String::from("hunter2"),
            interval_secs: 5,
            host_key: None,
        }
    }

    fn nas() -> SimNas {
        let mut nas = SimNas::new();
        nas.remember(&device()).expect("the machine is taken");
        nas
    }

    #[test]
    fn a_session_starts_opening_and_is_up_once_the_machine_answers() {
        let mut nas = nas();
        let mut remote = Remote::open(&mut nas, &device(), 80, 24);

        assert_eq!(remote.status(), NasShellStatus::Opening);
        assert!(remote.is_live(), "a session on its way up is worth asking");

        remote.poll(&mut nas);

        assert_eq!(remote.status(), NasShellStatus::Ready);
        assert!(
            !remote.screen().is_empty(),
            "a shell says hello and prints a prompt: {:?}",
            remote.screen().lines()
        );
    }

    /// The machine's own prompt, which is what the app does not draw: `user@host:cwd$ `.
    #[test]
    fn the_prompt_is_the_machines() {
        let mut nas = nas();
        let mut remote = Remote::open(&mut nas, &device(), 80, 24);
        remote.poll(&mut nas);

        let page = remote.screen().lines();
        let prompt = page.last().expect("a page has a line").clone();

        assert!(
            prompt.starts_with("nasstat@192.168.1.10:"),
            "the prompt names the account and the machine: {prompt:?}"
        );
        assert!(
            prompt.ends_with("$ "),
            "and ends at a shell's `$ `: {prompt:?}"
        );
    }

    /// A line is sent as a line, and what comes back is the machine's echo and its answer.
    #[test]
    fn a_line_is_sent_and_the_answer_is_drawn() {
        let mut nas = nas();
        let mut remote = Remote::open(&mut nas, &device(), 80, 24);
        remote.poll(&mut nas);

        for ch in "pwd".chars() {
            remote.key(&mut nas, KeyAction::Char(ch));
        }
        assert_eq!(remote.input(), "pwd", "the app keeps the line being typed");

        remote.key(&mut nas, KeyAction::Enter);

        assert_eq!(remote.input(), "", "the line is the machine's now");

        remote.poll(&mut nas);
        let page = remote.screen().lines();

        assert!(
            page.iter().any(|line| line.contains("pwd")),
            "the echo is on the page: {page:?}"
        );
        assert!(
            page.iter().any(|line| line.ends_with("/volume1")),
            "so is what `pwd` printed: {page:?}"
        );
    }

    /// A session that stays where it was left: the whole reason there is one instead of a key per command.
    #[test]
    fn the_session_remembers_where_it_went() {
        let mut nas = nas();
        let mut remote = Remote::open(&mut nas, &device(), 80, 24);
        remote.poll(&mut nas);

        send(&mut nas, &mut remote, "cd /volume2");
        send(&mut nas, &mut remote, "pwd");

        let page = remote.screen().lines();
        let last = page.last().expect("a page has a line").clone();

        assert!(
            last.starts_with("nasstat@192.168.1.10:/volume2"),
            "the prompt moved with the shell: {last:?}"
        );
    }

    /// `exit` is a clean goodbye and not a fault, and afterwards it stops being worth asking.
    #[test]
    fn exiting_closes_the_session() {
        let mut nas = nas();
        let mut remote = Remote::open(&mut nas, &device(), 80, 24);
        remote.poll(&mut nas);

        send(&mut nas, &mut remote, "exit");

        assert_eq!(remote.status(), NasShellStatus::Closed);
        assert!(!remote.is_live(), "a closed session is not asked again");
    }

    /// A machine that cannot be reached says so on the page, in the reading of it the app has words for.
    ///
    /// At the login and not at the asking, which is where the backend puts it: the request to open is
    /// only a request, and a machine that is off is one whose *session* comes back lost. That is why
    /// opening a session is not an operation that can fail on the page — the page is told, one look later.
    #[test]
    fn a_machine_that_is_off_is_a_fault_and_not_a_session() {
        let mut nas = SimNas::unreachable();
        nas.remember(&device()).expect("the machine is taken");

        let mut remote = Remote::open(&mut nas, &device(), 80, 24);

        assert_eq!(
            remote.status(),
            NasShellStatus::Opening,
            "the request is taken; whether it comes up is the session's business"
        );

        remote.poll(&mut nas);

        assert_eq!(remote.status(), NasShellStatus::Lost(NasFault::Unreachable));
        assert!(!remote.is_live(), "a session that was lost is not asked again");
        assert!(
            remote.screen().is_empty(),
            "and a machine that is off has drawn nothing: {:?}",
            remote.screen().lines()
        );
    }

    /// A host this backend has never been told about is refused, and refused as a machine it cannot reach:
    /// from the page's side there is one machine and there is nothing at the other end of it.
    #[test]
    fn a_host_nobody_knows_is_refused() {
        let mut nas = nas();
        let stranger = NasCredentials {
            host: String::from("10.0.0.9"),
            ..device()
        };

        let remote = Remote::open(&mut nas, &stranger, 80, 24);

        assert_eq!(remote.status(), NasShellStatus::Lost(NasFault::Unreachable));
    }

    /// Typing at a session that is gone goes nowhere, and says nothing about it.
    #[test]
    fn a_key_at_a_closed_session_is_ignored() {
        let mut nas = nas();
        let mut remote = Remote::open(&mut nas, &device(), 80, 24);
        remote.poll(&mut nas);
        send(&mut nas, &mut remote, "exit");

        let before = remote.screen().lines();
        for ch in "ls".chars() {
            remote.key(&mut nas, KeyAction::Char(ch));
        }
        remote.key(&mut nas, KeyAction::Enter);
        remote.poll(&mut nas);

        assert_eq!(remote.input(), "", "nothing was typed into a closed session");
        assert_eq!(remote.screen().lines(), before, "and nothing came back");
    }

    /// The page keeps a scrollback and does not grow without end: a `yes` would otherwise fill the heap.
    #[test]
    fn the_page_does_not_grow_without_end() {
        let mut nas = nas();
        let mut remote = Remote::open(&mut nas, &device(), 80, 24);
        remote.poll(&mut nas);

        for _ in 0..40 {
            send(&mut nas, &mut remote, "df -h");
        }

        assert!(
            remote.screen().len() <= crate::screen::MAX_LINES,
            "the page is capped: {} lines",
            remote.screen().len()
        );
    }

    /// Types a line at a session and lets it answer.
    fn send(nas: &mut SimNas, remote: &mut Remote, line: &str) {
        for ch in line.chars() {
            remote.key(nas, KeyAction::Char(ch));
        }
        remote.key(nas, KeyAction::Enter);
        remote.poll(nas);
    }
}
