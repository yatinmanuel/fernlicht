//! Top-down view of the car for `fernlicht preview`, nose up.

use std::io::{self, Write};
use std::time::Duration;

use fernlicht::show::{Driver, Lights, Output};

type Rgb = (u8, u8, u8);

const WHITE: Rgb = (255, 244, 214);
const AMBER: Rgb = (255, 150, 0);
const RED: Rgb = (255, 30, 20);

/// A dark lamp still shows faintly, so the outline of the car stays visible.
const DIM_FLOOR: f64 = 0.12;

const FRONT_LEFT: [Output; 5] =
    [Output::FlTurn, Output::FlDrl, Output::FlRing, Output::FlLow, Output::FlHigh];
const FRONT_RIGHT: [Output; 5] =
    [Output::FrHigh, Output::FrLow, Output::FrRing, Output::FrDrl, Output::FrTurn];
const REAR_LEFT: [Output; 3] = [Output::RlTurn, Output::RlTail, Output::RlBrake];
const REAR_RIGHT: [Output; 3] = [Output::RrBrake, Output::RrTail, Output::RrTurn];

/// Inner width: five lamps a side at the front, three spaced wider at the rear.
const INNER: usize = 32;
const HEIGHT: usize = 7;

fn colour(output: Output) -> Rgb {
    match output {
        Output::FlTurn | Output::FrTurn | Output::RlTurn | Output::RrTurn => AMBER,
        o if o.is_front() => WHITE,
        _ => RED,
    }
}

fn lamp(lights: &Lights, output: Output) -> String {
    let k = DIM_FLOOR + (1.0 - DIM_FLOOR) * f64::from(lights.get(output)) / 255.0;
    let (r, g, b) = colour(output);
    let scale = |c: u8| (f64::from(c) * k).round() as u8;
    format!("\x1b[38;2;{};{};{}m██\x1b[0m", scale(r), scale(g), scale(b))
}

fn row(lights: &Lights, left: &[Output], right: &[Output]) -> String {
    let side = |outputs: &[Output]| outputs.iter().map(|&o| lamp(lights, o)).collect::<Vec<_>>().join(" ");
    let used = 3 * (left.len() + right.len()) - 2;
    format!(" │ {}{}{} │", side(left), " ".repeat(INNER - used), side(right))
}

pub fn car(lights: &Lights) -> String {
    let blank = format!(" │{}│", " ".repeat(INNER + 2));
    [
        format!(" ╭{}╮", "─".repeat(INNER + 2)),
        row(lights, &FRONT_LEFT, &FRONT_RIGHT),
        blank.clone(),
        blank.clone(),
        blank,
        row(lights, &REAR_LEFT, &REAR_RIGHT),
        format!(" ╰{}╯", "─".repeat(INNER + 2)),
    ]
    .join("\n")
}

/// A driver that draws instead of talking to a car.
#[derive(Debug)]
pub struct Preview<W: Write> {
    out: W,
    drawn: bool,
}

impl<W: Write> Preview<W> {
    pub fn new(mut out: W) -> io::Result<Self> {
        write!(out, "\x1b[?25l")?;
        Ok(Self { out, drawn: false })
    }

    fn draw(&mut self, lights: &Lights) -> fernlicht::Result<()> {
        if self.drawn {
            write!(self.out, "\x1b[{HEIGHT}A")?;
        }
        writeln!(self.out, "{}", car(lights))?;
        self.out.flush()?;
        self.drawn = true;
        Ok(())
    }
}

impl<W: Write> Driver for Preview<W> {
    fn begin(&mut self) -> fernlicht::Result<()> {
        self.draw(&Lights::dark())
    }

    fn frame(&mut self, lights: &Lights, _hold: Duration) -> fernlicht::Result<()> {
        self.draw(lights)
    }

    fn release(&mut self) -> fernlicht::Result<()> {
        self.draw(&Lights::dark())
    }
}

impl<W: Write> Drop for Preview<W> {
    fn drop(&mut self) {
        let _ = write!(self.out, "\x1b[?25h");
        let _ = self.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in s.chars() {
            match (in_escape, c) {
                (false, '\x1b') => in_escape = true,
                (true, 'm') => in_escape = false,
                (false, c) => out.push(c),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn rows_line_up() {
        let text = strip_ansi(&car(&Lights::dark()));
        let widths: Vec<usize> = text.lines().map(|l| l.chars().count()).collect();
        assert_eq!(widths.len(), HEIGHT);
        assert!(widths.iter().all(|&w| w == widths[0]), "{widths:?}");
    }
}
