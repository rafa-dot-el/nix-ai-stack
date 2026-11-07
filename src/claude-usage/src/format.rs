use std::collections::HashMap;

/// Color output mode for conditional color expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// `%{F#hex}value%{F-}` (polybar/lemonbar)
    Polybar,
    /// ANSI 24-bit true color escape sequences
    Ansi,
    /// No color wrapping, just the raw value
    None,
}

impl std::str::FromStr for ColorMode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "polybar" | "lemonbar" => Self::Polybar,
            "ansi" | "terminal" => Self::Ansi,
            "none" | "raw" => Self::None,
            _ => Self::Polybar,
        })
    }
}

impl ColorMode {
    fn wrap(self, value: &str, hex_color: &str) -> String {
        match self {
            Self::Polybar => format!("%{{F{hex_color}}}{value}%{{F-}}"),
            Self::Ansi => {
                let hex = hex_color.trim_start_matches('#');
                if hex.len() >= 6 {
                    if let (Ok(r), Ok(g), Ok(b)) = (
                        u8::from_str_radix(&hex[0..2], 16),
                        u8::from_str_radix(&hex[2..4], 16),
                        u8::from_str_radix(&hex[4..6], 16),
                    ) {
                        return format!("\x1b[38;2;{r};{g};{b}m{value}\x1b[0m");
                    }
                }
                value.to_string()
            }
            Self::None => value.to_string(),
        }
    }
}

/// Condition in a color expression: `<50:#a3be8c` or `>90:#bf616a` or `:#ebcb8b` (else).
struct ColorRule {
    op: RuleOp,
    color: String,
}

enum RuleOp {
    LessThan(f64),
    LessEqual(f64),
    GreaterThan(f64),
    GreaterEqual(f64),
    Else,
}

impl ColorRule {
    fn matches(&self, val: f64) -> bool {
        match self.op {
            RuleOp::LessThan(t) => val < t,
            RuleOp::LessEqual(t) => val <= t,
            RuleOp::GreaterThan(t) => val > t,
            RuleOp::GreaterEqual(t) => val >= t,
            RuleOp::Else => true,
        }
    }
}

/// Parse `<50:#a3be8c` or `>90:#bf616a` or `:#default` or `<=80:#hex`.
fn parse_rule(s: &str) -> Option<ColorRule> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    if let Some(color) = s.strip_prefix(':') {
        return Some(ColorRule {
            op: RuleOp::Else,
            color: color.to_string(),
        });
    }

    let (op_str, rest) = if let Some(rest) = s.strip_prefix("<=") {
        ("<=", rest)
    } else if let Some(rest) = s.strip_prefix(">=") {
        (">=", rest)
    } else if let Some(rest) = s.strip_prefix('<') {
        ("<", rest)
    } else if let Some(rest) = s.strip_prefix('>') {
        (">", rest)
    } else {
        return None;
    };

    let colon = rest.find(':')?;
    let threshold: f64 = rest[..colon].parse().ok()?;
    let color = rest[colon + 1..].to_string();

    let op = match op_str {
        "<" => RuleOp::LessThan(threshold),
        "<=" => RuleOp::LessEqual(threshold),
        ">" => RuleOp::GreaterThan(threshold),
        ">=" => RuleOp::GreaterEqual(threshold),
        _ => return None,
    };

    Some(ColorRule { op, color })
}

/// Evaluate color rules against a numeric value, returning the first matching color.
fn evaluate_rules(rules: &[ColorRule], raw: &str) -> Option<String> {
    let val: f64 = raw.parse().ok()?;
    rules
        .iter()
        .find(|r| r.matches(val))
        .map(|r| r.color.clone())
}

/// Expand `{VAR}` and `{VAR|<50:#green|<80:#yellow|:#red}` placeholders.
///
/// Syntax:
///   `{VAR}`                              -- simple substitution
///   `{VAR|<50:#a3be8c|<80:#ebcb8b|:#bf616a}`  -- conditional coloring
///
/// Rules are separated by `|` and evaluated in order (first match wins):
///   `<N:#hex`   -- if value < N
///   `<=N:#hex`  -- if value <= N
///   `>N:#hex`   -- if value > N
///   `>=N:#hex`  -- if value >= N
///   `:#hex`     -- else / default
pub fn expand(template: &str, vars: &HashMap<String, String>, color_mode: ColorMode) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            let mut inner = String::new();
            let mut found_close = false;
            for ch in chars.by_ref() {
                if ch == '}' {
                    found_close = true;
                    break;
                }
                inner.push(ch);
            }
            if found_close {
                result.push_str(&expand_expr(&inner, vars, color_mode));
            } else {
                result.push('{');
                result.push_str(&inner);
            }
        } else {
            result.push(c);
        }
    }

    result
}

fn expand_expr(expr: &str, vars: &HashMap<String, String>, color_mode: ColorMode) -> String {
    let pipe_pos = expr.find('|');
    let var_name = match pipe_pos {
        Some(pos) => &expr[..pos],
        None => expr,
    };

    let raw_value = match vars.get(var_name) {
        Some(v) => v,
        None => {
            return format!("{{{expr}}}");
        }
    };

    let rules_str = match pipe_pos {
        Some(pos) => &expr[pos + 1..],
        None => return raw_value.clone(),
    };

    let rules: Vec<ColorRule> = rules_str.split('|').filter_map(parse_rule).collect();

    if rules.is_empty() {
        return raw_value.clone();
    }

    match evaluate_rules(&rules, raw_value) {
        Some(color) => color_mode.wrap(raw_value, &color),
        None => raw_value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_expansion() {
        let mut vars = HashMap::new();
        vars.insert("PCT_5H".into(), "36".into());
        vars.insert("COUNTDOWN_5H".into(), "1h 54m".into());

        assert_eq!(
            expand("{PCT_5H}% | next: {COUNTDOWN_5H}", &vars, ColorMode::None),
            "36% | next: 1h 54m"
        );
    }

    #[test]
    fn unknown_vars_pass_through() {
        let vars = HashMap::new();
        assert_eq!(
            expand("color: %{F#abc}test", &vars, ColorMode::None),
            "color: %{F#abc}test"
        );
    }

    #[test]
    fn conditional_color_low() {
        let mut vars = HashMap::new();
        vars.insert("PCT_5H".into(), "30".into());

        let out = expand(
            "{PCT_5H|<50:#a3be8c|<80:#ebcb8b|:#bf616a}%",
            &vars,
            ColorMode::Polybar,
        );
        assert_eq!(out, "%{F#a3be8c}30%{F-}%");
    }

    #[test]
    fn conditional_color_mid() {
        let mut vars = HashMap::new();
        vars.insert("PCT_5H".into(), "65".into());

        let out = expand(
            "{PCT_5H|<50:#a3be8c|<80:#ebcb8b|:#bf616a}%",
            &vars,
            ColorMode::Polybar,
        );
        assert_eq!(out, "%{F#ebcb8b}65%{F-}%");
    }

    #[test]
    fn conditional_color_high() {
        let mut vars = HashMap::new();
        vars.insert("PCT_5H".into(), "92".into());

        let out = expand(
            "{PCT_5H|<50:#a3be8c|<80:#ebcb8b|:#bf616a}%",
            &vars,
            ColorMode::Polybar,
        );
        assert_eq!(out, "%{F#bf616a}92%{F-}%");
    }

    #[test]
    fn conditional_ansi_mode() {
        let mut vars = HashMap::new();
        vars.insert("PCT_5H".into(), "10".into());

        let out = expand("{PCT_5H|<50:#00ff00|:#ff0000}", &vars, ColorMode::Ansi);
        assert_eq!(out, "\x1b[38;2;0;255;0m10\x1b[0m");
    }

    #[test]
    fn conditional_none_mode() {
        let mut vars = HashMap::new();
        vars.insert("PCT_5H".into(), "10".into());

        let out = expand("{PCT_5H|<50:#00ff00|:#ff0000}", &vars, ColorMode::None);
        assert_eq!(out, "10");
    }

    #[test]
    fn greater_than_rule() {
        let mut vars = HashMap::new();
        vars.insert("PCT_WEEK".into(), "95".into());

        let out = expand(
            "{PCT_WEEK|>90:#ff0000|:#00ff00}%",
            &vars,
            ColorMode::Polybar,
        );
        assert_eq!(out, "%{F#ff0000}95%{F-}%");
    }

    #[test]
    fn mixed_plain_and_colored() {
        let mut vars = HashMap::new();
        vars.insert("PCT_5H".into(), "36".into());
        vars.insert("COUNTDOWN_5H".into(), "1h 54m".into());

        let out = expand(
            "{PCT_5H|<50:#a3be8c|:#bf616a}% | {COUNTDOWN_5H}",
            &vars,
            ColorMode::Polybar,
        );
        assert_eq!(out, "%{F#a3be8c}36%{F-}% | 1h 54m");
    }

    #[test]
    fn non_numeric_value_no_color() {
        let mut vars = HashMap::new();
        vars.insert("COUNTDOWN_5H".into(), "1h 54m".into());

        let out = expand(
            "{COUNTDOWN_5H|<50:#a3be8c|:#bf616a}",
            &vars,
            ColorMode::Polybar,
        );
        assert_eq!(out, "1h 54m");
    }

    // ---- edge cases ----

    #[test]
    fn empty_template() {
        let vars = HashMap::new();
        assert_eq!(expand("", &vars, ColorMode::None), "");
    }

    #[test]
    fn plain_text_no_vars() {
        let vars = HashMap::new();
        assert_eq!(
            expand("hello world 123", &vars, ColorMode::None),
            "hello world 123"
        );
    }

    #[test]
    fn unclosed_brace_passthrough() {
        let vars = HashMap::new();
        assert_eq!(expand("{unclosed", &vars, ColorMode::None), "{unclosed");
    }

    #[test]
    fn empty_braces() {
        let vars = HashMap::new();
        assert_eq!(expand("{}", &vars, ColorMode::None), "{}");
    }

    #[test]
    fn adjacent_vars() {
        let mut vars = HashMap::new();
        vars.insert("A".into(), "1".into());
        vars.insert("B".into(), "2".into());
        assert_eq!(expand("{A}{B}", &vars, ColorMode::None), "12");
    }

    #[test]
    fn var_at_start() {
        let mut vars = HashMap::new();
        vars.insert("X".into(), "hello".into());
        assert_eq!(expand("{X} world", &vars, ColorMode::None), "hello world");
    }

    #[test]
    fn var_at_end() {
        let mut vars = HashMap::new();
        vars.insert("X".into(), "world".into());
        assert_eq!(expand("hello {X}", &vars, ColorMode::None), "hello world");
    }

    #[test]
    fn percent_brace_not_confused_with_var() {
        let vars = HashMap::new();
        let out = expand("100% {F#abc} done", &vars, ColorMode::None);
        assert_eq!(out, "100% {F#abc} done");
    }

    // ---- all comparison operators ----

    #[test]
    fn less_equal_at_boundary() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "50".into());
        let out = expand("{V|<=50:#00ff00|:#ff0000}", &vars, ColorMode::Polybar);
        assert_eq!(out, "%{F#00ff00}50%{F-}");
    }

    #[test]
    fn less_equal_above_boundary() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "51".into());
        let out = expand("{V|<=50:#00ff00|:#ff0000}", &vars, ColorMode::Polybar);
        assert_eq!(out, "%{F#ff0000}51%{F-}");
    }

    #[test]
    fn greater_equal_at_boundary() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "90".into());
        let out = expand("{V|>=90:#ff0000|:#00ff00}", &vars, ColorMode::Polybar);
        assert_eq!(out, "%{F#ff0000}90%{F-}");
    }

    #[test]
    fn greater_equal_below_boundary() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "89".into());
        let out = expand("{V|>=90:#ff0000|:#00ff00}", &vars, ColorMode::Polybar);
        assert_eq!(out, "%{F#00ff00}89%{F-}");
    }

    #[test]
    fn less_than_at_boundary_not_matched() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "50".into());
        let out = expand("{V|<50:#00ff00|:#ff0000}", &vars, ColorMode::Polybar);
        // 50 is NOT < 50, so falls through to else
        assert_eq!(out, "%{F#ff0000}50%{F-}");
    }

    #[test]
    fn greater_than_at_boundary_not_matched() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "90".into());
        let out = expand("{V|>90:#ff0000|:#00ff00}", &vars, ColorMode::Polybar);
        // 90 is NOT > 90
        assert_eq!(out, "%{F#00ff00}90%{F-}");
    }

    // ---- value edge cases ----

    #[test]
    fn zero_value_color() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "0".into());
        let out = expand("{V|<50:#00ff00|:#ff0000}", &vars, ColorMode::Polybar);
        assert_eq!(out, "%{F#00ff00}0%{F-}");
    }

    #[test]
    fn negative_value_color() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "-5".into());
        let out = expand("{V|<0:#0000ff|:#ff0000}", &vars, ColorMode::Polybar);
        assert_eq!(out, "%{F#0000ff}-5%{F-}");
    }

    #[test]
    fn large_value() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "999".into());
        let out = expand("{V|<100:#00ff00|:#ff0000}", &vars, ColorMode::Polybar);
        assert_eq!(out, "%{F#ff0000}999%{F-}");
    }

    #[test]
    fn fractional_value() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "49.9".into());
        let out = expand("{V|<50:#00ff00|:#ff0000}", &vars, ColorMode::None);
        assert_eq!(out, "49.9");
    }

    // ---- no rules match (no else) ----

    #[test]
    fn no_else_rule_no_match() {
        let mut vars = HashMap::new();
        vars.insert("V".into(), "75".into());
        let out = expand("{V|<50:#00ff00}", &vars, ColorMode::Polybar);
        // 75 is not < 50, no else rule → raw value
        assert_eq!(out, "75");
    }

    // ---- multiple colored vars in one template ----

    #[test]
    fn multiple_colored_vars() {
        let mut vars = HashMap::new();
        vars.insert("A".into(), "30".into());
        vars.insert("B".into(), "80".into());
        let out = expand(
            "{A|<50:#00ff00|:#ff0000}% / {B|<50:#00ff00|:#ff0000}%",
            &vars,
            ColorMode::Polybar,
        );
        assert_eq!(out, "%{F#00ff00}30%{F-}% / %{F#ff0000}80%{F-}%");
    }

    // ---- ANSI color wrapping ----

    #[test]
    fn ansi_wrap_valid_hex() {
        let result = ColorMode::Ansi.wrap("hello", "#ff8800");
        assert_eq!(result, "\x1b[38;2;255;136;0mhello\x1b[0m");
    }

    #[test]
    fn ansi_wrap_short_hex_falls_back() {
        let result = ColorMode::Ansi.wrap("hello", "#fff");
        assert_eq!(result, "hello");
    }

    #[test]
    fn ansi_wrap_no_hash() {
        let result = ColorMode::Ansi.wrap("hello", "ff8800");
        assert_eq!(result, "\x1b[38;2;255;136;0mhello\x1b[0m");
    }

    // ---- ColorMode::from_str ----

    fn parse_mode(s: &str) -> ColorMode {
        s.parse().unwrap()
    }

    #[test]
    fn color_mode_from_str() {
        assert_eq!(parse_mode("polybar"), ColorMode::Polybar);
        assert_eq!(parse_mode("POLYBAR"), ColorMode::Polybar);
        assert_eq!(parse_mode("lemonbar"), ColorMode::Polybar);
        assert_eq!(parse_mode("ansi"), ColorMode::Ansi);
        assert_eq!(parse_mode("terminal"), ColorMode::Ansi);
        assert_eq!(parse_mode("ANSI"), ColorMode::Ansi);
        assert_eq!(parse_mode("none"), ColorMode::None);
        assert_eq!(parse_mode("raw"), ColorMode::None);
        assert_eq!(parse_mode("unknown"), ColorMode::Polybar);
    }

    // ---- parse_rule ----

    #[test]
    fn parse_rule_less_than() {
        let r = parse_rule("<50:#a3be8c").unwrap();
        assert!(matches!(r.op, RuleOp::LessThan(v) if (v - 50.0).abs() < 0.001));
        assert_eq!(r.color, "#a3be8c");
    }

    #[test]
    fn parse_rule_less_equal() {
        let r = parse_rule("<=80:#ebcb8b").unwrap();
        assert!(matches!(r.op, RuleOp::LessEqual(v) if (v - 80.0).abs() < 0.001));
    }

    #[test]
    fn parse_rule_greater_than() {
        let r = parse_rule(">90:#bf616a").unwrap();
        assert!(matches!(r.op, RuleOp::GreaterThan(v) if (v - 90.0).abs() < 0.001));
    }

    #[test]
    fn parse_rule_greater_equal() {
        let r = parse_rule(">=100:#ff0000").unwrap();
        assert!(matches!(r.op, RuleOp::GreaterEqual(v) if (v - 100.0).abs() < 0.001));
    }

    #[test]
    fn parse_rule_else() {
        let r = parse_rule(":#ffffff").unwrap();
        assert!(matches!(r.op, RuleOp::Else));
        assert_eq!(r.color, "#ffffff");
    }

    #[test]
    fn parse_rule_empty() {
        assert!(parse_rule("").is_none());
    }

    #[test]
    fn parse_rule_invalid_no_colon() {
        assert!(parse_rule("<50").is_none());
    }

    #[test]
    fn parse_rule_invalid_no_threshold() {
        assert!(parse_rule("<:#abc").is_none());
    }

    #[test]
    fn parse_rule_fractional_threshold() {
        let r = parse_rule("<33.3:#abc").unwrap();
        assert!(matches!(r.op, RuleOp::LessThan(v) if (v - 33.3).abs() < 0.001));
    }
}
