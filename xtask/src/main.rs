//! Repository chores. Run with `cargo xtask <task>`.
//!
//! - `logo`: redraws `docs/logo-*.svg` and `brand/icon.svg` from the pixel
//!   grids in `brand/`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("logo") => logo(),
        _ => bail!("usage: cargo xtask logo"),
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").to_owned()
}

/// Grid legend: `#` body, `o` beam, `*` beam core, `+` halo, `.` and ` ` empty.
struct Theme {
    body: &'static str,
    text: &'static str,
}

const LIGHT: Theme = Theme { body: "#1a1712", text: "#1a1712" };
const DARK: Theme = Theme { body: "#ece6da", text: "#ece6da" };
const BEAM: &str = "#ffb000";
const BEAM_CORE: &str = "#ffe9a8";
const ICON_BACKGROUND: &str = "#0a0805";

fn grid(name: &str) -> anyhow::Result<Vec<Vec<char>>> {
    let path = root().join("brand").join(name);
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text.trim_end().lines().map(|line| line.chars().collect()).collect())
}

fn mark_fill(theme: &Theme, cell: char) -> Option<String> {
    Some(match cell {
        '#' => format!(r#"fill="{}""#, theme.body),
        'o' => format!(r#"fill="{BEAM}""#),
        '*' => format!(r#"fill="{BEAM_CORE}""#),
        '+' => format!(r#"fill="{BEAM}" opacity=".22""#),
        _ => return None,
    })
}

fn pixels(
    rows: &[Vec<char>],
    px: usize,
    x0: usize,
    y0: usize,
    fill: impl Fn(char) -> Option<String>,
) -> String {
    let mut out = String::new();
    for (y, row) in rows.iter().enumerate() {
        for (x, &cell) in row.iter().enumerate() {
            if let Some(fill) = fill(cell) {
                let (x, y) = (x0 + x * px, y0 + y * px);
                let _ = write!(out, r#"<rect x="{x}" y="{y}" width="{px}" height="{px}" {fill}/>"#);
            }
        }
    }
    out
}

fn svg(width: usize, height: usize, body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" shape-rendering="crispEdges">{body}</svg>"#
    ) + "\n"
}

fn wordmark_logo(theme: &Theme, mark: &[Vec<char>], word: &[Vec<char>]) -> String {
    const PX: usize = 10;
    const WORD_PX: usize = 8;
    const GAP: usize = 3 * PX;

    let mark_width = mark[0].len() * PX;
    let height = mark.len() * PX;
    let width = mark_width + GAP + word[0].len() * WORD_PX;
    let word_y = (height - word.len() * WORD_PX) / 2;

    let mut body = pixels(mark, PX, 0, 0, |c| mark_fill(theme, c));
    body += &pixels(word, WORD_PX, mark_width + GAP, word_y, |c| {
        (c == '#').then(|| format!(r#"fill="{}""#, theme.text))
    });
    svg(width, height, &body)
}

fn icon(mark: &[Vec<char>]) -> String {
    const PX: usize = 32;
    const SIZE: usize = 512;
    let x0 = (SIZE - mark[0].len() * PX) / 2;
    let y0 = (SIZE - mark.len() * PX) / 2;
    let mut body = format!(r#"<rect width="{SIZE}" height="{SIZE}" fill="{ICON_BACKGROUND}"/>"#);
    body += &pixels(mark, PX, x0, y0, |c| mark_fill(&DARK, c));
    svg(SIZE, SIZE, &body)
}

fn logo() -> anyhow::Result<()> {
    let mark = grid("mark.txt")?;
    let word = grid("wordmark.txt")?;
    let root = root();
    let outputs = [
        (root.join("docs/logo-light.svg"), wordmark_logo(&LIGHT, &mark, &word)),
        (root.join("docs/logo-dark.svg"), wordmark_logo(&DARK, &mark, &word)),
        (root.join("brand/icon.svg"), icon(&mark)),
    ];
    for (path, content) in outputs {
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.strip_prefix(&root).unwrap_or(&path).display());
    }
    Ok(())
}
