//! The shell on the machine that is not there.

/// The simulated machine's file system, as far as a listing needs it: the directories there are, and
/// what is inside each of them.
///
/// A tree and not a list per directory, because `cd ..` has to land somewhere: a simulator whose shell
/// could not be walked around in would not be modelling the one thing a session has over a command per
/// keystroke, which is that it stays where it was left.
const DIRECTORIES: &[(&str, &[&str])] = &[
    ("/", &["volume1", "volume2", "usb"]),
    ("/volume1", &["docker", "homes", "media", "photos"]),
    ("/volume1/docker", &["jellyfin", "sonarr"]),
    ("/volume1/docker/jellyfin", &[]),
    ("/volume1/docker/sonarr", &[]),
    ("/volume1/homes", &["admin"]),
    ("/volume1/homes/admin", &[]),
    ("/volume1/media", &["movies", "music", "tv"]),
    ("/volume1/media/movies", &[]),
    ("/volume1/media/music", &[]),
    ("/volume1/media/tv", &[]),
    ("/volume1/photos", &["2026"]),
    ("/volume1/photos/2026", &[]),
    ("/volume2", &["backup", "media"]),
    ("/volume2/backup", &["homes"]),
    ("/volume2/backup/homes", &[]),
    ("/volume2/media", &["movies"]),
    ("/volume2/media/movies", &[]),
    ("/usb", &[]),
];

/// Where a shell's prompt starts.
const HOME: &str = "/volume1";

/// A shell on a machine that is not there, being typed at.
///
/// It models the *machine* and not the screen. A screen — carriage returns, escape sequences, a cursor
/// that moves — belongs to whoever is drawing the terminal, and on this side of the wire there is no
/// screen: there is a shell, and it prints bytes. What it prints are a real shell's bytes, `\r\n` line
/// endings and all, because an app built against this is an app that will meet a real one.
///
/// What it has is a handful of commands rather than a shell, and deliberately the handful somebody
/// sitting next to a NAS types. `cd` is the one that matters — a directory that stays changed across
/// two commands is the whole reason a session exists — and the rest are here so that the pages of
/// output a person actually reads have something in them.
pub struct SimShell {
    /// The account, as the prompt writes it.
    user: String,
    /// The machine, as the prompt writes it.
    host: String,
    /// Where the shell is, kept across commands like a real one's.
    cwd: String,
    /// The line being typed, which is what a pseudo-terminal is echoing. Bytes and not a `String`: a
    /// character off the panel's keyboard arrives as one to four of them, and a half-typed one is not
    /// text yet.
    line: Vec<u8>,
    /// What has been printed and not read yet.
    output: Vec<u8>,
    /// The shell has hung up — an `exit`, or a `D` on an empty line.
    ended: bool,
    /// How far into an escape sequence the input is: 0 for not in one, 1 for just past an `ESC`, and 2
    /// for inside the `ESC [ …` that a key like an arrow is made of. State because the bytes only mean
    /// that together: the `[` and the `A` of `ESC [ A` are ordinary characters on their own.
    escape: u8,
}

impl SimShell {
    /// A shell on `host`, logged in as `user` and already talking: what it has printed so far is its
    /// welcome and its first prompt.
    pub fn new(user: &str, host: &str) -> Self {
        let mut shell = Self {
            user: String::from(user),
            host: String::from(host),
            cwd: String::from(HOME),
            line: Vec::new(),
            output: Vec::new(),
            ended: false,
            escape: 0,
        };

        shell.print("Welcome to the Pomelo simulator's NAS — a machine that is not there.\r\n");
        shell.prompt();

        shell
    }

    /// Whether the shell has hung up.
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Takes what it has printed since the last call.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Types `bytes` at it: the echo, and then whatever they did.
    ///
    /// A pseudo-terminal echoes what it is typed, so the echo is printed here rather than assumed by
    /// the caller — which is what keeps a session's echo and a session's output in one order.
    pub fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
    }

    /// Says or does what one byte does, which is the whole of what a terminal's input is.
    fn byte(&mut self, byte: u8) {
        // The bytes that are not a key being typed go first, because they only mean anything together:
        // an arrow key is `ESC [ A`, and the `[` and the `A` of it are ordinary characters on their own.
        // Skipped rather than drawn, so that pressing one upsets the line less than printing `^[[A` in
        // the middle of it would.
        if self.escape > 0 {
            self.escape = match (self.escape, byte) {
                // `ESC [` opens one that runs until a byte a sequence like this can end on.
                (1, b'[') => 2,
                // An `ESC` and one other byte: a key the other way of writing, and over already.
                (1, _) => 0,
                (_, byte) if (0x40..=0x7e).contains(&byte) => 0,
                (inside, _) => inside,
            };

            return;
        }

        match byte {
            0x1b => self.escape = 1,
            // A pseudo-terminal turns Return into a carriage return, and the app sends that. Taking a
            // newline as well costs nothing and is what a serial line would have sent.
            b'\r' | b'\n' => self.run(),
            // The terminal's backspace and the one a serial line sends, which are two bytes apart.
            0x7f | 0x08 => self.erase(),
            // ^C: abandon the line. The one control character nobody can do without, because it is the
            // only way out of a command that will not stop.
            0x03 => self.interrupt(),
            // ^D on an empty line: the end of the input, which is how a shell is asked to leave.
            0x04 => self.end_of_input(),
            // Everything a keyboard can make. A byte above 0x7f is a piece of a character and is kept
            // as it came: decoding per byte would turn somebody's Chinese into replacement characters.
            0x20..=0x7f | 0x80..=0xff => {
                self.line.push(byte);
                self.output.push(byte);
            }
            // Left over: the rest of the control characters, none of which a shell's line discipline
            // would have kept either.
            _ => {}
        }
    }

    /// Erases the character before the cursor, and the columns it took to draw it.
    fn erase(&mut self) {
        if self.line.is_empty() {
            return;
        }

        // A character can be several bytes, and erasing half of one leaves the line invalid. So the
        // continuation bytes go first, and then the byte that started it.
        while self
            .line
            .last()
            .is_some_and(|byte| byte & 0b1100_0000 == 0b1000_0000)
        {
            self.line.pop();
            self.echo_backspace();
        }

        self.line.pop();
        self.echo_backspace();
    }

    /// The three bytes that un-draw one column: back, over it, and forward again.
    fn echo_backspace(&mut self) {
        self.output.extend_from_slice(b"\x08 \x08");
    }

    /// ^C: throws away the line and says so, the way a shell does.
    fn interrupt(&mut self) {
        self.line.clear();
        self.print("^C\r\n");
        self.prompt();
    }

    /// ^D: the end of the input. On an empty line that is a shell leaving; on a line somebody was in
    /// the middle of, a real shell does nothing at all.
    fn end_of_input(&mut self) {
        if self.line.is_empty() {
            self.hang_up();
        }
    }

    /// Return: runs the line, which is where everything above has been leading.
    fn run(&mut self) {
        self.print("\r\n");

        let line = String::from_utf8_lossy(&self.line).trim().to_string();
        self.line.clear();

        // An empty line is a new `$ ` and nothing else, exactly as a shell does it.
        if !line.is_empty() {
            self.run_line(&line);
        }

        if !self.ended {
            self.prompt();
        }
    }

    /// Runs one line of the handful this shell knows.
    fn run_line(&mut self, line: &str) {
        let (command, argument) = match line.split_once(' ') {
            Some((command, rest)) => (command, rest.trim()),
            None => (line, ""),
        };

        match (command, argument) {
            ("ls", target) => self.list(target),
            ("cd", target) => self.change_directory(target),
            ("pwd", _) => {
                let cwd = self.cwd.clone();
                self.print(&format!("{cwd}\r\n"));
            }
            ("df", "-h") => self.disks(),
            ("docker", "ps") => self.containers(),
            ("uname", "-a") => {
                let host = self.host.clone();
                self.print(&format!(
                    "Linux {host} 6.1.0-pomelo #1 SMP x86_64 GNU/Linux\r\n"
                ));
            }
            ("exit" | "logout", _) => self.hang_up(),
            ("help", _) => {
                self.print("pomelo sim: ls, cd, pwd, df -h, docker ps, uname -a, exit\r\n");
            }
            // The answer a shell gives, and the reason `help` above is worth having at all: a terminal
            // whose only reply to a wrong guess is silence is a terminal that looks broken.
            (command, _) => self.print(&format!("sh: {command}: command not found\r\n")),
        }
    }

    /// `ls`, of where the shell is or of wherever it was pointed.
    fn list(&mut self, target: &str) {
        let path = if target.is_empty() {
            Some(self.cwd.clone())
        } else {
            resolve(&self.cwd, target)
        };

        let Some(path) = path else {
            self.print(&format!("ls: {target}: No such file or directory\r\n"));
            return;
        };

        let entries = DIRECTORIES
            .iter()
            .find(|(directory, _)| *directory == path)
            .map(|(_, entries)| *entries)
            .unwrap_or(&[]);

        match entries {
            [] => self.print(""),
            entries => self.print(&format!("{}\r\n", entries.join("  "))),
        }
    }

    /// `cd`, which is the command this whole simulator exists for.
    fn change_directory(&mut self, target: &str) {
        // A bare `cd` is home, which is what a shell does with it.
        let target = if target.is_empty() { HOME } else { target };

        match resolve(&self.cwd, target) {
            Some(path) => self.cwd = path,
            None => self.print(&format!("sh: cd: {target}: No such file or directory\r\n")),
        }
    }

    /// `df -h`: the machine's own disks, which are the ones the monitor's table lists — one system SSD
    /// and two data drives, so that a number read here and a number read there are the same number.
    fn disks(&mut self) {
        self.print(
            "Filesystem      Size  Used Avail Use% Mounted on\r\n\
             /dev/sda1       457G   88G  346G  21% /\r\n\
             /dev/sdb1       3.6T  1.4T  2.1T  41% /volume1\r\n\
             /dev/sdc1       3.6T  1.2T  2.3T  35% /volume2\r\n",
        );
    }

    /// `docker ps`: a NAS doing the thing most of them are doing.
    fn containers(&mut self) {
        self.print(
            "CONTAINER ID   IMAGE                STATUS      NAMES\r\n\
             3f2a1c9b7d10   jellyfin/jellyfin    Up 4 days   jellyfin\r\n\
             a91b0c2d4e55   linuxserver/sonarr   Up 4 days   sonarr\r\n",
        );
    }

    /// Says goodbye, which is what a shell says when it is asked to leave.
    fn hang_up(&mut self) {
        self.print("logout\r\n");
        self.ended = true;
    }

    /// Draws the prompt, which is where every line begins.
    fn prompt(&mut self) {
        let prompt = format!("{}@{}:{}$ ", self.user, self.host, self.cwd);
        self.print(&prompt);
    }

    /// Prints, in the machine's bytes.
    fn print(&mut self, text: &str) {
        self.output.extend_from_slice(text.as_bytes());
    }
}

/// Where `target` lands when typed from `cwd`, or `None` when nothing is there.
///
/// Resolved the way a shell resolves one — a leading `/` starts at the root and everything else starts
/// where the shell already is — because `cd ..` and `cd /volume2` are the two a person types, they have
/// to land in different places, and a simulator that got that wrong would hide a whole class of bug in
/// whatever is sending the bytes.
fn resolve(cwd: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        cwd.split('/').filter(|part| !part.is_empty()).collect()
    };

    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }

    let path = format!("/{}", parts.join("/"));

    DIRECTORIES
        .iter()
        .any(|(directory, _)| *directory == path)
        .then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shell whose welcome has been read, so that a test sees only what it caused.
    fn shell() -> SimShell {
        let mut shell = SimShell::new("admin", "192.168.1.10");
        shell.take();

        shell
    }

    /// What it has printed since the last call, as text. Lossy on purpose: these tests are about what
    /// a screen would draw, and a screen reads characters.
    fn said(shell: &mut SimShell) -> String {
        String::from_utf8_lossy(&shell.take()).into_owned()
    }

    /// A line comes back twice: once as the echo and once as the answer, and then there is a prompt.
    #[test]
    fn a_line_is_echoed_and_answered() {
        let mut shell = shell();

        shell.write(b"pwd\r");

        let said = said(&mut shell);
        assert!(said.starts_with("pwd\r\n"), "the echo comes first: {said:?}");
        assert!(said.contains("/volume1"), "and then the answer: {said:?}");
        assert!(said.ends_with("$ "), "and then a prompt: {said:?}");
    }

    /// The one thing a session has over a command per keystroke: where it was told to go stays gone to.
    #[test]
    fn where_the_shell_is_stays_changed() {
        let mut shell = shell();

        shell.write(b"cd /volume2\r");
        let moved = said(&mut shell);
        assert!(moved.contains("/volume2$"), "the prompt moved: {moved:?}");

        shell.write(b"pwd\r");
        assert!(said(&mut shell).contains("/volume2"), "and stayed put");

        // And a relative step from there, which is the other half of what `cd` means.
        shell.write(b"cd media\r");
        said(&mut shell);
        shell.write(b"pwd\r");
        assert!(said(&mut shell).contains("/volume2/media"));

        // And back up one, which is where a tree is worth being one.
        shell.write(b"cd ..\r");
        assert!(said(&mut shell).contains("/volume2$ "));
    }

    /// A directory that is not there is refused, in a shell's own words.
    #[test]
    fn a_directory_that_is_not_there_is_refused() {
        let mut shell = shell();

        shell.write(b"cd nowhere\r");
        assert!(said(&mut shell).contains("No such file or directory"));

        shell.write(b"ls nowhere\r");
        assert!(said(&mut shell).contains("No such file or directory"));

        shell.write(b"pwd\r");
        assert!(
            said(&mut shell).contains("/volume1"),
            "and the shell has not moved"
        );
    }

    /// Backspace takes a character back off the line and off the screen.
    #[test]
    fn backspace_takes_a_character_back() {
        let mut shell = shell();

        shell.write(b"pwx\x7f");
        let erased = said(&mut shell);
        assert!(erased.ends_with("\x08 \x08"), "the column is un-drawn: {erased:?}");

        shell.write(b"d\r");
        assert!(
            said(&mut shell).contains("/volume1"),
            "`pwx`, erased, then `d`: the line ran as `pwd`"
        );
    }

    /// A character that is not one byte is erased as one character, which is the bug a per-byte
    /// backspace would leave behind.
    #[test]
    fn a_character_above_ascii_is_erased_whole() {
        let mut shell = shell();

        shell.write("你".as_bytes());
        assert_eq!(said(&mut shell), "你", "it is echoed as it came");

        shell.write(b"\x7f");
        assert_eq!(
            said(&mut shell),
            "\x08 \x08\x08 \x08\x08 \x08",
            "three columns, for the three bytes of it"
        );
    }

    /// ^C abandons the line, which is what it is for, and the shell carries on afterwards.
    #[test]
    fn interrupting_abandons_the_line() {
        let mut shell = shell();

        shell.write(b"df -h\x03");

        let interrupted = said(&mut shell);
        assert!(interrupted.contains("^C"));
        assert!(
            !interrupted.contains("Filesystem"),
            "and nothing was run: {interrupted:?}"
        );

        shell.write(b"pwd\r");
        assert!(
            said(&mut shell).contains("/volume1"),
            "and the next line is a new one"
        );
    }

    /// A wrong guess is answered, because a terminal that went quiet would look broken.
    #[test]
    fn an_unknown_command_says_so() {
        let mut shell = shell();

        shell.write(b"frobnicate\r");

        assert!(said(&mut shell).contains("frobnicate: command not found"));
    }

    /// `exit` says goodbye and hangs up, and says nothing after it.
    #[test]
    fn exit_hangs_up() {
        let mut shell = shell();

        shell.write(b"exit\r");

        let said = said(&mut shell);
        assert!(said.contains("logout"));
        assert!(shell.ended());
        assert!(!said.ends_with("$ "), "and no prompt after it: {said:?}");
    }

    /// ^D on an empty line leaves, and ^D in the middle of a line does nothing — which is what a real
    /// shell does with both, and the pair is easy to get backwards.
    #[test]
    fn end_of_input_leaves_only_on_an_empty_line() {
        let mut shell = shell();

        shell.write(b"ls\x04");
        assert!(!shell.ended(), "a half-typed line is not the end");

        shell.write(b"\x03\x04");
        assert!(shell.ended(), "and an empty one is");
    }

    /// The bytes that are not a key being typed are dropped rather than drawn: an arrow key is three of
    /// them, and `^[[A` in the middle of a line would be worse than nothing.
    #[test]
    fn an_escape_sequence_is_not_drawn() {
        let mut shell = shell();

        shell.write(b"\x1b[A");

        assert_eq!(said(&mut shell), "", "neither typed nor drawn");
    }

    /// The commands a person actually types at a NAS answer with something worth reading.
    #[test]
    fn the_commands_that_read_things_answer_with_them() {
        let mut shell = shell();

        shell.write(b"df -h\r");
        assert!(said(&mut shell).contains("/dev/sdb1"));

        shell.write(b"docker ps\r");
        assert!(said(&mut shell).contains("jellyfin"));

        shell.write(b"ls\r");
        assert!(said(&mut shell).contains("photos"), "where the shell starts");

        shell.write(b"uname -a\r");
        assert!(said(&mut shell).contains("Linux 192.168.1.10"));
    }
}
