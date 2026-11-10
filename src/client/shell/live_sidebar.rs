use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::render::put_text;
use super::*;
use crate::protocol::PaneSurfaceFrame;

fn is_code_or_diff(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    // 1. Blacklist checks (strict rejection of UI chrome, banners, chat, prompts)
    let lower = trimmed.to_lowercase();
    if lower.contains("antigravity cli")
        || lower.contains("@gmail.com")
        || lower.contains("google ai")
        || lower.contains("gemini ")
        || lower.contains("claude ")
        || lower.contains("chatgpt")
        || lower.contains("openai")
        || lower.contains("sign-in update")
        || lower.contains("sign in again")
        || lower.contains("missing permissions")
        || lower.contains("generating...")
        || lower.contains("esc to cancel")
        || lower.contains("thinking...")
        || lower.contains("analyzing...")
        || lower.contains("press enter")
        || lower.contains("working...")
        || lower.contains("run /logout")
    {
        return false;
    }

    // Reject block/box-drawing ASCII art characters
    if trimmed.chars().any(|c| matches!(c, '█' | '▀' | '▄' | '░' | '▒' | '▓' | '▌' | '▐' | '╭' | '╰' | '╮' | '╯')) {
        return false;
    }

    // Reject shell prompts / terminal markers
    if trimmed == ">"
        || trimmed == "~"
        || trimmed == "$"
        || trimmed == "#"
        || trimmed.starts_with("root@")
        || (trimmed.ends_with('$') && trimmed.contains('@'))
        || (trimmed.ends_with('#') && trimmed.contains('@'))
    {
        return false;
    }

    // Reject user chat prompt (e.g. "> siap kerja bro?")
    if trimmed.starts_with('>') {
        let after_gt = trimmed[1..].trim();
        if !after_gt.starts_with('<') && !after_gt.starts_with('/') && !after_gt.starts_with('-') {
            let has_letters = after_gt.chars().any(|c| c.is_alphabetic());
            let is_code_like = after_gt.contains(';')
                || after_gt.contains('{')
                || after_gt.contains("def ")
                || after_gt.contains("fn ")
                || after_gt.contains("let ");
            if has_letters && !is_code_like {
                return false;
            }
        }
    }

    // 2. Whitelist checks (code diffs & syntax)
    // Git diff lines
    if (trimmed.starts_with('+') && !trimmed.starts_with("+++ "))
        || (trimmed.starts_with('-') && !trimmed.starts_with("--- "))
        || trimmed.starts_with("@@")
        || trimmed.starts_with("diff --git")
        || trimmed.starts_with("diff -")
        || trimmed.starts_with("index ")
        || trimmed.starts_with("--- a/")
        || trimmed.starts_with("+++ b/")
    {
        return true;
    }

    // Tool call / file modification indicators
    if trimmed.starts_with("[edit]")
        || trimmed.starts_with("[write]")
        || trimmed.starts_with("Path:")
        || trimmed.starts_with("Target:")
        || trimmed.starts_with("Replacement:")
        || trimmed.starts_with("```")
    {
        return true;
    }

    // Code keywords (Rust, Python, JS/TS, Go, C, Shell)
    let keywords = [
        "fn ", "pub fn ", "pub struct ", "pub enum ", "pub const ", "pub trait ",
        "struct ", "impl ", "enum ", "let ", "const ", "static ", "use ", "mod ",
        "def ", "class ", "import ", "from ", "async def ", "return ", "yield ",
        "function ", "export ", "var ", "interface ", "type ",
        "if ", "else ", "elif ", "for ", "while ", "match ", "switch ", "case ",
        "curl ", "chmod ", "mkdir ", "git ", "cargo ", "npm ", "sudo "
    ];
    for kw in &keywords {
        if trimmed.starts_with(kw) {
            return true;
        }
    }

    // Code comments
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("* ") {
        return true;
    }
    if trimmed.starts_with('#') && !trimmed.starts_with("#!") && trimmed.len() > 1 && trimmed.chars().nth(1) == Some(' ') {
        return true;
    }

    // Indented lines with code punctuation
    if line.starts_with("  ") || line.starts_with('\t') {
        if trimmed.ends_with(';')
            || trimmed.ends_with('{')
            || trimmed.ends_with('}')
            || trimmed.ends_with(':')
            || trimmed.contains(" = ")
            || trimmed.contains(" := ")
            || trimmed.contains("::")
            || trimmed.contains("->")
            || trimmed.contains("=>")
            || trimmed.contains("()")
            || (trimmed.contains('(') && trimmed.contains(')'))
            || (trimmed.contains('[') && trimmed.contains(']'))
        {
            return true;
        }
    }

    false
}

fn append_unique_stream(history: &mut Vec<String>, incoming: &[String], max_capacity: usize) {
    if incoming.is_empty() {
        return;
    }
    let mut best_overlap = 0;
    let max_check = incoming.len().min(history.len());
    for overlap in (1..=max_check).rev() {
        let history_slice = &history[history.len() - overlap..];
        let incoming_slice = &incoming[..overlap];
        if history_slice == incoming_slice {
            best_overlap = overlap;
            break;
        }
    }
    for line in &incoming[best_overlap..] {
        history.push(line.clone());
    }
    if history.len() > max_capacity {
        let drain_count = history.len() - max_capacity;
        history.drain(0..drain_count);
    }
}

pub(super) fn render_live_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: Option<&ClientShellSnapshot>,
    config: &ClientShellConfig,
    pane_surface: Option<&PaneSurfaceFrame>,
    live_code_history: &mut Vec<String>,
    _hits: &mut ShellHitMap,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let palette = &config.palette;

    // Header divider
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(palette.surface_dim),
    );
    if area.height < 2 {
        return;
    }

    // Check active working agent
    let mut active_agent_name = None;
    if let Some(snap) = snapshot {
        for ag in &snap.agents {
            if ag.agent_status == crate::api::schema::AgentStatus::Working {
                active_agent_name = ag.name.as_deref().or(ag.agent.as_deref());
                break;
            }
        }
    }

    let (title_text, title_style) = if let Some(name) = active_agent_name {
        (
            format!(" ● live ({name})"),
            Style::default().fg(palette.green).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            " live coding".to_string(),
            Style::default().fg(palette.overlay1).add_modifier(Modifier::BOLD),
        )
    };

    put_text(buffer, area.x, area.y + 1, area.width, &title_text, title_style);

    let content_y = area.y + 2;
    let available_rows = area.height.saturating_sub(2);
    if available_rows == 0 {
        return;
    }

    // Extract live terminal cells
    let mut raw_code_lines = Vec::new();
    if let Some(surface) = pane_surface {
        let w = surface.frame.width as usize;
        let h = surface.frame.height as usize;
        if w > 0 && h > 0 && surface.frame.cells.len() >= w * h {
            for r in 0..h {
                let row_cells = &surface.frame.cells[r * w..(r + 1) * w];
                let raw_line: String = row_cells.iter().map(|c| c.symbol.as_str()).collect();
                let trimmed = raw_line.trim_end();
                if is_code_or_diff(trimmed) {
                    raw_code_lines.push(trimmed.to_string());
                }
            }
        }
    }

    // Append newly detected code lines to persistent history
    append_unique_stream(live_code_history, &raw_code_lines, 200);

    // Prepare display lines
    let fallback_idle = [
        "  waiting for code changes...".to_string(),
        "  (idle · watching background diff)".to_string(),
    ];
    let display_lines: &[String] = if !live_code_history.is_empty() {
        live_code_history.as_slice()
    } else {
        &fallback_idle
    };

    let start_idx = display_lines.len().saturating_sub(available_rows as usize);
    for (i, line) in display_lines[start_idx..].iter().enumerate() {
        let y = content_y + (i as u16);
        if y >= area.bottom() {
            break;
        }
        let t = line.trim_start();
        let style = if t.starts_with('+') {
            Style::default().fg(palette.green)
        } else if t.starts_with('-') {
            Style::default().fg(palette.red)
        } else if t.starts_with("@@") {
            Style::default().fg(palette.accent).add_modifier(Modifier::BOLD)
        } else if t.starts_with("diff") || t.starts_with("index") || t.starts_with("---") || t.starts_with("+++") {
            Style::default().fg(palette.yellow)
        } else if t.starts_with("//") || t.starts_with('#') || t.starts_with("/*") || t.starts_with('*') {
            Style::default().fg(palette.overlay1)
        } else if t.starts_with("fn ")
            || t.starts_with("pub ")
            || t.starts_with("def ")
            || t.starts_with("class ")
            || t.starts_with("import ")
            || t.starts_with("from ")
            || t.starts_with("let ")
            || t.starts_with("const ")
            || t.starts_with("return ")
            || t.starts_with("struct ")
            || t.starts_with("impl ")
        {
            Style::default().fg(palette.mauve).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text)
        };

        put_text(buffer, area.x, y, area.width, line, style);
    }
}
