use anyhow::Context;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout},
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::Block,
    DefaultTerminal, Frame,
};

pub fn main() -> anyhow::Result<()> {
    let file = std::env::args().nth(1).context("Usage: hexedit <path>")?;
    let file = std::fs::read(file)?;
    let terminal = ratatui::init();
    let app_result = State::new(file).run(terminal);
    ratatui::restore();
    app_result
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Replace,
}

impl Mode {
    fn allow_motion(self) -> bool {
        match self {
            Mode::Normal => true,
            Mode::Insert | Mode::Replace => false,
        }
    }
}

#[derive(Copy, Clone)]
enum Position {
    Hex(usize),
    Ascii(usize),
}

impl Position {
    fn add_assign(&mut self, rhs: isize, max: usize) {
        match self {
            Position::Hex(ref mut n) => *n = std::cmp::min(n.saturating_add_signed(rhs), max),
            Position::Ascii(ref mut n) => *n = std::cmp::min(n.saturating_add_signed(rhs), max),
        }
    }

    fn switch(&mut self) {
        match self {
            Position::Hex(n) => *self = Position::Ascii(*n),
            Position::Ascii(n) => *self = Position::Hex(*n),
        }
    }

    fn is_hex(self) -> bool {
        match self {
            Position::Hex(_) => true,
            Position::Ascii(_) => false,
        }
    }

    fn is_ascii(self) -> bool {
        match self {
            Position::Hex(_) => false,
            Position::Ascii(_) => true,
        }
    }

    fn inner(self) -> usize {
        match self {
            Position::Hex(n) => n,
            Position::Ascii(n) => n,
        }
    }

    fn set(&mut self, value: usize) {
        match self {
            Position::Hex(n) => *n = value,
            Position::Ascii(n) => *n = value,
        }
    }

    fn max(&mut self, max: usize) {
        self.set(self.inner().min(max));
    }
}

struct State {
    mode: Mode,
    position: Position,
    top: bool,
    file: Vec<u8>,
}

impl State {
    fn new(file: Vec<u8>) -> Self {
        Self {
            mode: Mode::Normal,
            position: Position::Hex(0),
            top: true,
            file,
        }
    }

    fn run(mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    match key.code {
                        KeyCode::Esc => {
                            self.mode = Mode::Normal;
                        }
                        KeyCode::Char('i') if self.mode == Mode::Normal => {
                            self.mode = Mode::Insert;
                        }
                        KeyCode::Char(c @ 'a'..='f' | c @ '0'..='9')
                            if self.mode == Mode::Insert && self.position.is_hex() =>
                        {
                            let x = self.position.inner();
                            let val = if c >= 'a' {
                                c as u8 - b'a' + 10
                            } else {
                                c as u8 - b'0'
                            };
                            if self.top {
                                self.file.insert(x, val << 4);
                                self.top ^= true;
                            } else {
                                self.file[x] |= val;
                                self.top = true;
                                self.position.add_assign(1, self.file.len() - 1);
                            }
                        }
                        KeyCode::Backspace
                            if self.mode == Mode::Insert && self.position.is_ascii() =>
                        {
                            let x = self.position.inner();
                            if x > 0 {
                                self.file.remove(x - 1);
                                self.position.add_assign(-1, self.file.len() - 1);
                            }
                        }
                        KeyCode::Enter if self.mode == Mode::Insert && self.position.is_ascii() => {
                            let x = self.position.inner();
                            self.file.insert(x, b'\n');
                            self.position.add_assign(1, self.file.len() - 1);
                        }
                        KeyCode::Char(c)
                            if self.mode == Mode::Insert && self.position.is_ascii() =>
                        {
                            let x = self.position.inner();
                            self.file.insert(x, c as u8);
                            self.position.add_assign(1, self.file.len() - 1);
                        }
                        KeyCode::Char('q') if self.mode == Mode::Normal => {
                            return Ok(());
                        }
                        KeyCode::Char('l') if self.mode.allow_motion() => {
                            self.position.add_assign(1, self.file.len() - 1);
                        }
                        KeyCode::Char('h') if self.mode.allow_motion() => {
                            self.position.add_assign(-1, self.file.len() - 1);
                        }
                        KeyCode::Char('j') if self.mode.allow_motion() => {
                            if self.position.inner() < self.file.len() - 16 {
                                self.position.add_assign(16, self.file.len() - 1);
                            }
                        }
                        KeyCode::Char('k') if self.mode.allow_motion() => {
                            if self.position.inner() >= 16 {
                                self.position.add_assign(-16, self.file.len() - 1);
                            }
                        }
                        KeyCode::Char('x') if self.mode == Mode::Normal => {
                            let n = self.position.inner();
                            self.file.remove(n);
                            self.position.max(self.file.len() - 1);
                        }
                        // TODO: I'm not sure I like H/L for this.
                        KeyCode::Char('H') if self.mode.allow_motion() => {
                            self.position.switch();
                        }
                        KeyCode::Char('L') if self.mode.allow_motion() => {
                            self.position.switch();
                        }
                        // TODO: w/b in ascii mode
                        // TODO: / in ascii mode
                        // TODO: : to goto an index
                        //           - should goto the exact byte
                        //           - should allow either hex or decimal
                        //           - should also be for command mode in the future
                        // TODO: i for insert mode
                        // TODO: r for replace char
                        // TODO: R for replace mode
                        _ => {}
                    }
                }
                e => {
                    dbg!(e);
                }
            }
        }
    }

    fn half(&self, buf: &[u8], offset: usize) -> Vec<Span<'static>> {
        let mut out: Vec<Span<'static>> = vec![];
        let (hl, main_hl) = match self.position {
            Position::Hex(n) => (n.checked_sub(offset), true),
            Position::Ascii(n) => (n.checked_sub(offset), false),
        };
        for (i, c) in buf.iter().enumerate() {
            if Some(i) == hl {
                out.push(if main_hl {
                    format!("{:02x}", c).bg(Color::Gray).fg(Color::Black).bold()
                } else {
                    format!("{:02x}", c).fg(Color::Magenta).bold()
                });
            } else {
                out.push(format!("{:02x}", c).into());
            }
            out.push(" ".into());
        }
        for _ in 0..(8 - buf.len()) {
            out.push("   ".into());
        }
        out
    }

    fn text(&self, buf: &[u8], offset: usize) -> Vec<Span<'static>> {
        let (hl, main_hl) = match self.position {
            Position::Hex(n) => (n.checked_sub(offset), false),
            Position::Ascii(n) => (n.checked_sub(offset), true),
        };
        let mut out = vec![];
        out.push("|".into());
        for (i, c) in buf.iter().enumerate() {
            let c = match c {
                b'"'..=b'}' | b' ' | b'!'..=b'~' => *c as char,
                _ => '.',
            };
            if Some(i) == hl {
                out.push(if main_hl {
                    c.to_string().bg(Color::Gray).fg(Color::Black).bold()
                } else {
                    c.to_string().fg(Color::Magenta).bold()
                });
            } else {
                out.push(c.to_string().into());
            }
        }
        out.push("|".into());
        out
    }

    fn render_text(&self, count: usize, buf: &[u8]) -> Line<'_> {
        let mut line = vec![format!("{:08x}", count).fg(Color::Green), "  ".into()];
        line.extend_from_slice(&self.half(&buf[..(std::cmp::min(8, buf.len()))], count));
        line.push(" ".into());
        line.extend_from_slice(&if buf.len() > 8 {
            self.half(&buf[8..(std::cmp::min(16, buf.len()))], count + 8)
        } else {
            self.half(&[], count + 8)
        });
        line.push(" ".into());
        line.extend_from_slice(&self.text(buf, count));
        Line::from(line)
    }

    // 00000000  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|
    fn draw(&self, frame: &mut Frame) {
        let layout = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(78),
            Constraint::Fill(1),
        ])
        .spacing(2)
        .split(frame.area());

        // let block = title_block("Address");
        // let addr_inner = block.inner(layout[0]);
        // frame.render_widget(block, layout[0]);

        let block = title_block("");
        let hex_inner = block.inner(layout[1]);
        frame.render_widget(block, layout[1]);

        let lines =
            Layout::vertical(vec![Constraint::Length(1); hex_inner.height.into()]).split(hex_inner);

        let mut count = 0;
        for (i, chunk) in self.file.chunks(16).enumerate().take(lines.len()) {
            frame.render_widget(self.render_text(count, chunk), lines[i]);
            count += chunk.len();
        }

        // let block = title_block("Ascii");
        // let ascii_inner = block.inner(layout[2]);
        // frame.render_widget(block, layout[2]);
    }
}

fn title_block(title: &str) -> Block<'static> {
    Block::new()
    // .borders(Borders::ALL)
    // .title_alignment(Alignment::Center)
    // .border_style(Style::new().dark_gray())
    // .title_style(Style::new().reset())
    // .padding(ratatui::widgets::Padding::symmetric(3, 1))
    // .title(format!(" {} ", title))
}
