use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::render::put_text;
use super::*;
use crate::protocol::PaneSurfaceFrame;

pub(super) fn render_live_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: Option<&ClientShellSnapshot>,
    config: &ClientShellConfig,
    pane_surface: Option<&PaneSurfaceFrame>,
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
            Style::default().fg(palette.overlay0).add_modifier(Modifier::BOLD),
        )
    };

    put_text(buffer, area.x, area.y + 1, area.width, &title_text, title_style);

    let content_y = area.y + 2;
    let available_rows = area.height.saturating_sub(2);
    if available_rows == 0 {
        return;
    }

    // Extract live terminal cells
    let mut lines_to_show = Vec::new();
    if let Some(surface) = pane_surface {
        let w = surface.frame.width as usize;
        let h = surface.frame.height as usize;
        if w > 0 && h > 0 && surface.frame.cells.len() >= w * h {
            for r in 0..h {
                let row_cells = &surface.frame.cells[r * w..(r + 1) * w];
                let raw_line: String = row_cells.iter().map(|c| c.symbol.as_str()).collect();
                let trimmed = raw_line.trim_end();
                if !trimmed.is_empty() {
                    lines_to_show.push(trimmed.to_string());
                }
            }
        }
    }

    // Filter for code diffs / changes
    let mut diff_lines = Vec::new();
    for line in &lines_to_show {
        let t = line.trim_start();
        if t.starts_with('+')
            || t.starts_with('-')
            || t.starts_with("@@")
            || t.starts_with("diff ")
            || t.starts_with("---")
            || t.starts_with("+++")
        {
            diff_lines.push(line.clone());
        }
    }

    let display_lines = if !diff_lines.is_empty() {
        diff_lines
    } else if !lines_to_show.is_empty() {
        let start = lines_to_show.len().saturating_sub(available_rows as usize);
        lines_to_show[start..].to_vec()
    } else {
        vec![
            "  waiting for agent tasks...".to_string(),
            "  (monitoring swarm feed)".to_string(),
        ]
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
            Style::default().fg(palette.accent)
        } else if t.starts_with("diff") || t.starts_with("index") {
            Style::default().fg(palette.yellow)
        } else {
            Style::default().fg(palette.text).add_modifier(Modifier::DIM)
        };

        put_text(buffer, area.x, y, area.width, line, style);
    }
}
