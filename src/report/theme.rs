use std::io::IsTerminal;

use figlet_rs::FIGlet;

const LAYOUT_WIDTH: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RiskTier {
    Minor,
    Moderate,
    Risky,
    High,
}

/// `(label, foreground code, pill code)` for a tier — a green→red gradient.
pub(super) fn tier_palette(tier: RiskTier) -> (&'static str, &'static str, &'static str) {
    match tier {
        RiskTier::Minor => ("MINOR", "38;5;42", "1;30;48;5;42"),
        RiskTier::Moderate => ("MODERATE", "38;5;220", "1;30;48;5;220"),
        RiskTier::Risky => ("RISKY", "38;5;208", "1;30;48;5;208"),
        RiskTier::High => ("HIGH", "38;5;196", "1;97;48;5;196"),
    }
}
pub(super) struct Theme {
    color: bool,
}

impl Theme {
    pub(super) fn detect() -> Self {
        let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        Self { color }
    }

    pub(super) fn paint(&self, text: impl AsRef<str>, code: &str) -> String {
        let text = text.as_ref();
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub(super) fn key(&self, text: &str) -> String {
        self.paint(format!("{text:>12}"), "2;37")
    }

    /// Big ASCII wordmark on a single row, rendered from a FIGlet font and
    /// tinted with a warm vertical "blast" gradient, led by a starburst accent.
    pub(super) fn banner(&self) -> Vec<String> {
        // The `slant` font gives the wordmark some forward motion. Trimming the
        // font's trailing padding keeps the burst + wordmark to ~77 cols.
        let wordmark = FIGlet::slant()
            .ok()
            .and_then(|font| font.convert("BLAST RADIUS").map(|fig| fig.to_string()));

        let Some(wordmark) = wordmark else {
            // Fallback if the font can't be loaded for any reason.
            let burst = self.paint("-=*=-", "1;38;5;226");
            let blast = self.paint("BLAST", "1;38;5;214");
            let radius = self.paint("RADIUS", "1;38;5;202");
            return vec![format!("  {burst}  {blast} {radius}")];
        };

        // Drop trailing per-line padding and the blank row the font appends.
        let mut rows: Vec<&str> = wordmark.lines().map(str::trim_end).collect();
        while rows.last().is_some_and(|l| l.is_empty()) {
            rows.pop();
        }
        let height = rows.len().max(1);

        // Warm vertical gradient (256-color), brightest at the top.
        const GRADIENT: [&str; 6] = [
            "38;5;226", "38;5;220", "38;5;214", "38;5;208", "38;5;202", "38;5;196",
        ];

        // A full starburst accent for "blast", vertically centered on the word.
        const BURST: [&str; 5] = [r"\ ' /", r".\|/.", "-=*=-", r"'/|\'", r"/ . \"];
        let burst_w = BURST.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let pad_top = height.saturating_sub(BURST.len()) / 2;

        let mut out = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let burst_line = i
                .checked_sub(pad_top)
                .and_then(|j| BURST.get(j))
                .copied()
                .unwrap_or("");
            let burst = self.paint(format!("{burst_line:^burst_w$}"), "1;38;5;226");
            let code = GRADIENT[(i * GRADIENT.len()) / height];
            let word = self.paint(row, &format!("1;{code}"));
            out.push(format!(" {burst} {word}"));
        }
        out
    }

    pub(super) fn subject(&self, text: &str) -> String {
        self.paint(text, "1;37")
    }

    pub(super) fn endpoint(&self, text: &str) -> String {
        self.paint(text, "38;5;42")
    }

    pub(super) fn ok(&self, text: &str) -> String {
        self.paint(text, "1;32")
    }

    /// A small severity dot, e.g. for the per-changed-file list.
    pub(super) fn tier_dot(&self, tier: RiskTier) -> String {
        let (_, fg, _) = tier_palette(tier);
        let glyph = match tier {
            RiskTier::Minor => "○",
            RiskTier::Moderate => "◐",
            RiskTier::Risky => "◕",
            RiskTier::High => "●",
        };
        self.paint(glyph, fg)
    }

    /// A solid-color risk chip, e.g. ` MODERATE `.
    pub(super) fn risk_pill(&self, tier: RiskTier) -> String {
        let (label, _, code) = tier_palette(tier);
        self.paint(format!(" {label} "), code)
    }

    /// A 20-cell bar filled in proportion to the tier, tinted by severity.
    pub(super) fn meter(&self, tier: RiskTier) -> String {
        const CELLS: usize = 20;
        let (_, fg, _) = tier_palette(tier);
        let filled = match tier {
            RiskTier::Minor => 5,
            RiskTier::Moderate => 10,
            RiskTier::Risky => 15,
            RiskTier::High => CELLS,
        };
        format!(
            "{}{}",
            self.paint("█".repeat(filled), fg),
            self.paint("░".repeat(CELLS - filled), "2;37")
        )
    }

    /// A full-width section divider: `── LABEL ───────────────`.
    pub(super) fn rule(&self, label: &str) -> String {
        let label = label.to_uppercase();
        let lead = "──";
        // 2 leading + spaces around label, padded out to the layout width.
        let used = 2 + lead.chars().count() + 1 + label.chars().count() + 1;
        let trail = LAYOUT_WIDTH.saturating_sub(used);
        format!(
            "  {} {} {}",
            self.paint(lead, "2;37"),
            self.paint(&label, "1;37"),
            self.paint("─".repeat(trail), "2;37")
        )
    }

    pub(super) fn pkg(&self, text: &str) -> String {
        self.paint(text, "1;36")
    }

    pub(super) fn count(&self, text: &str) -> String {
        self.paint(text, "1;32")
    }

    pub(super) fn path(&self, text: &str) -> String {
        self.paint(text, "36")
    }

    pub(super) fn symbol(&self, text: &str) -> String {
        self.paint(format!("#{text}"), "1;33")
    }

    pub(super) fn number(&self, value: usize) -> String {
        self.paint(value.to_string(), "1;32")
    }

    pub(super) fn file(&self, text: &str) -> String {
        self.paint(text, "34")
    }

    pub(super) fn export(&self, text: &str) -> String {
        self.paint(text, "33")
    }

    pub(super) fn depth(&self, value: usize) -> String {
        self.paint(format!("d{value}"), "2;32")
    }

    pub(super) fn depth_root(&self, text: &str) -> String {
        self.paint(text, "2;32")
    }

    pub(super) fn root(&self, text: &str) -> String {
        self.paint(format!("[{text}]"), "1;30;42")
    }

    pub(super) fn direct(&self, text: &str) -> String {
        self.paint(format!("[{text}]"), "1;30;44")
    }

    pub(super) fn edge_tag(&self, text: &str) -> String {
        self.paint(format!("[{text}]"), "2;34")
    }

    pub(super) fn warn_tag(&self, text: String) -> String {
        self.paint(format!("[{text}]"), "1;33")
    }

    pub(super) fn muted(&self, text: &str) -> String {
        self.paint(text, "2;37")
    }

    pub(super) fn warn(&self, text: &str) -> String {
        self.paint(text, "33")
    }
}
