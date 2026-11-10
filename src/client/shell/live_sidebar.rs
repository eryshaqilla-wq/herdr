use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::render::{put_right_text, put_text};
use super::*;
use crate::protocol::PaneSurfaceFrame;

fn is_known_command(cmd: &str) -> bool {
    let clean = cmd.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
    matches!(
        clean,
        "curl" | "git" | "cd" | "ls" | "cat" | "grep" | "find" | "mkdir" | "rm" | "mv" | "cp"
        | "chmod" | "chown" | "su" | "sudo" | "sh" | "bash" | "zsh" | "python" | "python3"
        | "node" | "cargo" | "npm" | "pnpm" | "yarn" | "gh" | "herdr" | "baw" | "baw2" | "baw3"
        | "ai" | "web" | "pmt" | "pemantau" | "telegram" | "termux-ipc" | "screencap"
        | "echo" | "export" | "source" | "apt" | "pkg" | "apk" | "kill" | "pkill" | "pgrep"
        | "ps" | "tar" | "unzip" | "adb" | "ssh" | "scp" | "install" | "sed" | "awk"
        | "diff" | "patch" | "head" | "tail" | "touch" | "which" | "whereis" | "make"
        | "pip" | "pip3" | "rustc" | "zig" | "zigcc"
    )
}

fn is_code_keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "pub" | "struct" | "impl" | "enum" | "let" | "const" | "static" | "use" | "mod"
        | "def" | "class" | "import" | "from" | "return" | "yield" | "async"
        | "function" | "export" | "var" | "interface" | "type"
        | "if" | "else" | "elif" | "for" | "while" | "match" | "switch" | "case"
    )
}

fn parse_code_or_command(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Reject standalone markdown symbols, table pipes, or empty list bullets
    if trimmed == "|"
        || trimmed == "│"
        || trimmed == ":"
        || trimmed == "-"
        || trimmed == "•"
        || trimmed == "---"
        || trimmed == "***"
    {
        return None;
    }

    // 1. AGY Tool runner line format: Running (<command>...) (ctrl+o to expand)
    // IMPORTANT: Check this FIRST before blacklist because AGY lines often contain "(esc to cancel)"!
    if trimmed.contains("Running (") {
        if let Some(start_idx) = trimmed.find("Running (") {
            let rest = &trimmed[start_idx + "Running (".len()..];
            let cmd_text = if let Some(end_idx) = rest.find(") (") {
                &rest[..end_idx]
            } else if let Some(end_idx) = rest.rfind(')') {
                &rest[..end_idx]
            } else {
                rest
            };
            let clean_cmd = cmd_text.trim();
            // Hanya terima jika perintah utuh dan tidak terpotong tanda ...
            if !clean_cmd.is_empty() && !clean_cmd.ends_with("...") {
                return Some(clean_cmd.to_string());
            }
        }
    }

    // 2. Blacklist checks (strict rejection of UI chrome, banners, chat, prompts)
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
        || lower.contains("thought for ")
        || lower.contains("press enter")
        || lower.contains("run /logout")
    {
        return None;
    }

    // Tolak baris status/progress terminal yang terpotong elipsis (...)
    if trimmed.ends_with("...") || trimmed.contains("... ×") || trimmed.contains("...) (") {
        return None;
    }

    // Reject block/box-drawing ASCII art characters
    if trimmed.chars().any(|c| matches!(c, '█' | '▀' | '▄' | '░' | '▒' | '▓' | '▌' | '▐' | '╭' | '╰' | '╮' | '╯')) {
        return None;
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
        return None;
    }

    // Reject markdown bullet lists from chat (e.g. "• Hanya menerima...", "- Panel 1...")
    if trimmed.starts_with("• ") {
        return None;
    }
    if trimmed.starts_with("- ") {
        let after_dash = trimmed[2..].trim();
        let first_word = after_dash.split_whitespace().next().unwrap_or("");
        let is_code_like = after_dash.contains(';')
            || after_dash.contains('{')
            || after_dash.contains('}')
            || is_known_command(first_word)
            || is_code_keyword(first_word);
        if !is_code_like {
            return None;
        }
    }

    // Reject user chat prompt (e.g. "> siap kerja bro?")
    if trimmed.starts_with('>') {
        let after_gt = trimmed[1..].trim();
        let first_word = after_gt.split_whitespace().next().unwrap_or("");
        let is_cmd = is_known_command(first_word);
        let is_code_like = after_gt.contains(';')
            || after_gt.contains('{')
            || after_gt.contains("def ")
            || after_gt.contains("fn ");
        if !is_cmd && !is_code_like {
            return None;
        }
    }

    // Shell prompts with ❯ or ➜ (Zsh/Starship/Oh-My-Zsh) -> tampilkan perintah murni tanpa $
    if trimmed.starts_with('❯') || trimmed.starts_with('➜') {
        let after_prompt = trimmed.trim_start_matches(|c: char| c == '❯' || c == '➜' || c == ' ').trim();
        let first_word = after_prompt.split_whitespace().next().unwrap_or("");
        if is_known_command(first_word) {
            return Some(after_prompt.to_string());
        }
    }

    // 3. Direct shell command detection ($ command or # command) -> tampilkan murni tanpa $ / #
    if let Some(cmd) = trimmed.strip_prefix("$ ") {
        let c = cmd.trim();
        if !c.is_empty() {
            return Some(c.to_string());
        }
    }
    if let Some(cmd) = trimmed.strip_prefix("# ") {
        let c = cmd.trim();
        if !c.is_empty() {
            return Some(c.to_string());
        }
    }

    // 4. Starts with a known binary or shell command -> tampilkan murni tanpa $
    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    if is_known_command(first_word) {
        return Some(trimmed.to_string());
    }

    // 5. Git diff lines
    if (trimmed.starts_with('+') && !trimmed.starts_with("+++ "))
        || (trimmed.starts_with('-') && !trimmed.starts_with("--- ") && !trimmed.starts_with("- "))
        || trimmed.starts_with("@@")
        || trimmed.starts_with("diff --git")
        || trimmed.starts_with("diff -")
        || trimmed.starts_with("index ")
        || trimmed.starts_with("--- a/")
        || trimmed.starts_with("+++ b/")
    {
        return Some(trimmed.to_string());
    }

    // 6. Tool call / file modification indicators
    if trimmed.starts_with("[edit]")
        || trimmed.starts_with("[write]")
        || trimmed.starts_with("[patch]")
        || trimmed.starts_with("edit_file(")
        || trimmed.starts_with("write_file(")
        || trimmed.starts_with("```")
    {
        return Some(trimmed.to_string());
    }

    // 7. Code keywords (Rust, Python, JS/TS, Go, C)
    let keywords = [
        "fn ", "pub fn ", "pub struct ", "pub enum ", "pub const ", "pub trait ",
        "struct ", "impl ", "enum ", "let ", "const ", "static ", "use ", "mod ",
        "def ", "class ", "import ", "from ", "async def ", "return ", "yield ",
        "function ", "export ", "var ", "interface ", "type ",
        "if ", "else ", "elif ", "for ", "while ", "match ", "switch ", "case "
    ];
    for kw in &keywords {
        if trimmed.starts_with(kw) {
            return Some(trimmed.to_string());
        }
    }

    // Code comments
    if trimmed.starts_with("//") || trimmed.starts_with("/*") {
        return Some(trimmed.to_string());
    }
    if trimmed.starts_with('#') && !trimmed.starts_with("#!") && trimmed.len() > 1 && trimmed.chars().nth(1) == Some(' ') {
        return Some(trimmed.to_string());
    }

    // Indented lines with code punctuation
    if line.starts_with("  ") || line.starts_with('\t') {
        if trimmed.ends_with(';')
            || trimmed.ends_with('{')
            || trimmed.ends_with('}')
            || trimmed.contains(" = ")
            || trimmed.contains(" := ")
            || trimmed.contains("::")
            || trimmed.contains("->")
            || trimmed.contains("=>")
            || trimmed.contains("()")
        {
            return Some(trimmed.to_string());
        }
    }

    None
}

fn wrap_code_line(line: &str, max_width: usize) -> Vec<String> {
    let clean = line.trim();
    if clean.is_empty() || max_width == 0 {
        return Vec::new();
    }
    let chars: Vec<char> = clean.chars().collect();
    if chars.len() <= max_width {
        return vec![clean.to_string()];
    }

    let mut result = Vec::new();
    let mut start = 0;

    while start < chars.len() {
        let remaining = chars.len() - start;
        let is_first = start == 0;
        let current_max = if is_first {
            max_width.max(1)
        } else {
            max_width.saturating_sub(2).max(1)
        };

        if remaining <= current_max {
            let chunk: String = chars[start..].iter().collect();
            let chunk_trimmed = chunk.trim();
            if !chunk_trimmed.is_empty() {
                if is_first {
                    result.push(chunk_trimmed.to_string());
                } else {
                    result.push(format!("  {chunk_trimmed}"));
                }
            }
            break;
        }

        let mut break_pos = start + current_max;
        let search_start = start + (current_max / 2).max(1);
        let mut found_break = false;

        if search_start < break_pos {
            for pos in (search_start..=break_pos).rev() {
                if pos < chars.len() && matches!(chars[pos], ' ' | ',' | ';' | '(' | ')' | '{' | '}' | '[' | ']') {
                    break_pos = pos + 1;
                    found_break = true;
                    break;
                }
            }
        }

        if !found_break {
            break_pos = start + current_max;
        }

        let chunk: String = chars[start..break_pos].iter().collect();
        let chunk_trimmed = chunk.trim();
        if !chunk_trimmed.is_empty() {
            if is_first {
                result.push(chunk_trimmed.to_string());
            } else {
                result.push(format!("  {chunk_trimmed}"));
            }
        }

        start = break_pos;
    }

    result
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
    endpoints: Option<&[ClientShellEndpoint]>,
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

    // Check all active working agents (either in snapshot or endpoints)
    let mut working_agents: Vec<String> = Vec::new();
    if let Some(snap) = snapshot {
        for ag in &snap.agents {
            if ag.agent_status == crate::api::schema::AgentStatus::Working {
                let name = ag.name.as_deref().or(ag.agent.as_deref()).unwrap_or("master");
                if !working_agents.contains(&name.to_string()) {
                    working_agents.push(name.to_string());
                }
            }
        }
    }
    if let Some(eps) = endpoints {
        for ep in eps {
            if let Some(snap) = ep.snapshot.as_deref() {
                for ag in &snap.agents {
                    if ag.agent_status == crate::api::schema::AgentStatus::Working {
                        let name = ag.name.as_deref().or(ag.agent.as_deref()).unwrap_or("agent");
                        if !working_agents.contains(&name.to_string()) {
                            working_agents.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Header title: serasi dengan " machines" dan " agents" (bold overlay0)
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " codeviewer",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );

    // Label kanan: merah saat idle, hijau tebal saat ada agent aktif (master / agents (n))
    let (right_label, right_style) = match working_agents.len() {
        0 => ("idle".to_string(), Style::default().fg(palette.red)),
        1 => (working_agents[0].clone(), Style::default().fg(palette.green).add_modifier(Modifier::BOLD)),
        n => (format!("agents ({n})"), Style::default().fg(palette.green).add_modifier(Modifier::BOLD)),
    };
    put_right_text(
        buffer,
        Rect::new(area.x, area.y + 1, area.width.saturating_sub(1), 1),
        area.y + 1,
        &right_label,
        right_style,
    );

    let content_y = area.y + 2;
    let available_rows = area.height.saturating_sub(2);
    if available_rows == 0 {
        return;
    }

    // Lebar konten: pas dengan panel, menyisakan 1 spasi kiri dan 1 margin kanan
    let content_width = area.width.saturating_sub(2).max(1) as usize;

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
                if let Some(parsed) = parse_code_or_command(trimmed) {
                    raw_code_lines.push(parsed);
                }
            }
        }
    }

    // Append newly detected code/command lines to persistent history
    append_unique_stream(live_code_history, &raw_code_lines, 200);

    // Prepare display lines dengan auto-wrapping agar teks tidak terpotong
    let fallback_idle = [
        "waiting for background tasks...".to_string(),
        "(idle · watching commands & diff)".to_string(),
    ];
    let lines_source: &[String] = if !live_code_history.is_empty() {
        live_code_history.as_slice()
    } else {
        &fallback_idle
    };

    let mut wrapped_lines: Vec<(String, Style)> = Vec::new();
    for raw_line in lines_source {
        let t = raw_line.trim_start();
        // Aturan styling asli: hanya baris git diff yang diberi warna khusus, selebihnya warna teks normal
        let style = if t.starts_with('+') {
            Style::default().fg(palette.green)
        } else if t.starts_with('-') {
            Style::default().fg(palette.red)
        } else if t.starts_with("@@") {
            Style::default().fg(palette.yellow).add_modifier(Modifier::BOLD)
        } else if t.starts_with("diff") || t.starts_with("index") || t.starts_with("---") || t.starts_with("+++") {
            Style::default().fg(palette.yellow)
        } else {
            Style::default().fg(palette.text)
        };

        for wrapped in wrap_code_line(raw_line, content_width) {
            wrapped_lines.push((wrapped, style));
        }
    }

    let start_idx = wrapped_lines.len().saturating_sub(available_rows as usize);
    for (i, (line, style)) in wrapped_lines[start_idx..].iter().enumerate() {
        let y = content_y + (i as u16);
        if y >= area.bottom() {
            break;
        }
        // Samakan persis dengan panel lain: mulai dari area.x + 1 (1 spasi dari batas kiri)
        put_text(buffer, area.x + 1, y, area.width.saturating_sub(1), line, *style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_code_line() {
        let line = "pub fn test(a: i32, b: i32) -> i32 { a + b }";
        let wrapped = wrap_code_line(line, 20);
        assert!(wrapped.len() > 1);
        for w in &wrapped {
            assert!(w.len() <= 20);
        }
    }

    #[test]
    fn test_wrap_no_empty_lines() {
        assert!(wrap_code_line("", 20).is_empty());
        assert!(wrap_code_line("   ", 20).is_empty());
    }

    #[test]
    fn test_parse_code_or_command() {
        // Baris yang terpotong elipsis (...) ditolak agar tidak menampilkan kode terpotong
        assert_eq!(
            parse_code_or_command("Running (su -c \"screencap -p...) (ctrl+o to expand)"),
            None
        );
        // Perintah utuh diterima dan ditampilkan murni tanpa awalan $
        assert_eq!(
            parse_code_or_command("Running (curl -s https://api.binance.com) (esc to cancel)"),
            Some("curl -s https://api.binance.com".to_string())
        );
        assert_eq!(
            parse_code_or_command("❯ git status"),
            Some("git status".to_string())
        );
        assert_eq!(
            parse_code_or_command("curl -s https://api.binance.com"),
            Some("curl -s https://api.binance.com".to_string())
        );
        assert_eq!(
            parse_code_or_command("git status -s"),
            Some("git status -s".to_string())
        );
        assert_eq!(
            parse_code_or_command("+pub fn main() {"),
            Some("+pub fn main() {".to_string())
        );
        assert_eq!(parse_code_or_command("Antigravity CLI 1.5.4"), None);
        assert_eq!(parse_code_or_command("> siap kerja bro?"), None);
        assert_eq!(parse_code_or_command("Generating..."), None);
        assert_eq!(parse_code_or_command("|"), None);
        assert_eq!(parse_code_or_command("• Hanya menerima git diff"), None);
        assert_eq!(parse_code_or_command("- Panel 1: machines"), None);
    }
}
