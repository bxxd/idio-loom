//! Pattern DSL parser and executor.
//!
//! Two input formats, same execution model:
//!
//! ## YAML (user-facing)
//! ```yaml
//! name: my-research
//! description: Custom research with critique
//! steps:
//!   - agent: axe
//!     nudge: "$INTAKE"
//!     stage: research
//!   - agent: bobby
//!     hears: axe
//!     nudge: "attack this thesis"
//!     stage: critique
//! ```
//!
//! ## .loom text (CLI/workshop)
//! ```text
//! do agent "nudge"                     @stage
//! do agent hears source "nudge"        @stage
//! do agent "nudge" use system-agent    @stage
//! # comments
//! ```
//!
//! Variables: $INTAKE, $NAME, $MODEL (substituted at parse/convert time)

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::loom::RunOpts;
use crate::thread::Thread;

/// Hook invoked before each pattern step. Return `Err` to abort (e.g. on
/// cancellation). Borrowed so callers can pass a closure capturing local state.
pub type BeforeTurnHook<'a> = dyn Fn(usize, &Step) -> Result<()> + 'a;

// =============================================================================
// Core types
// =============================================================================

/// A single step in a pattern — always a turn (gates are automatic).
#[derive(Debug, Clone)]
pub struct Step {
    pub agent: String,
    pub nudge: Option<String>,
    pub source: Option<String>,
    pub system: Option<String>,
    pub stage: Option<String>,
    pub model: Option<String>,
}

/// A parsed pattern program — ready to execute.
#[derive(Debug)]
pub struct Pattern {
    pub steps: Vec<Step>,
}

// =============================================================================
// YAML pattern definition (user-facing, serializable)
// =============================================================================

/// YAML-serializable pattern definition. The user-facing format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<StepDef>,
}

/// A single step in a YAML pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDef {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nudge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hears: Option<String>,
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl PatternDef {
    /// Parse a YAML pattern definition.
    pub fn from_yaml(source: &str) -> Result<Self> {
        serde_yaml::from_str(source).map_err(|e| anyhow::anyhow!("invalid pattern YAML: {}", e))
    }

    /// Serialize to YAML string.
    pub fn to_yaml(&self) -> Result<String> {
        serde_yaml::to_string(self).map_err(|e| anyhow::anyhow!("YAML serialize error: {}", e))
    }

    /// Convert to executable Pattern, applying variable substitutions.
    ///
    /// Variables are substituted in every string field, matching the .loom text
    /// parser which replaces vars over the entire source before tokenizing.
    pub fn to_pattern(&self, vars: &[(&str, &str)]) -> Pattern {
        let sub = |text: &str| -> String {
            let mut out = text.to_string();
            for (name, value) in vars {
                out = out.replace(name, value);
            }
            out
        };
        let steps = self
            .steps
            .iter()
            .map(|s| Step {
                agent: sub(&s.agent),
                nudge: s.nudge.as_deref().map(&sub),
                source: s.hears.as_deref().map(&sub),
                system: s.use_system.as_deref().map(&sub),
                stage: s.stage.as_deref().map(&sub),
                model: s.model.as_deref().map(&sub),
            })
            .collect();
        Pattern { steps }
    }

    /// Validate: check that steps are non-empty, agents are named, $INTAKE is used.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            bail!("pattern name is required");
        }
        if self.steps.is_empty() {
            bail!("pattern must have at least one step");
        }
        for (i, step) in self.steps.iter().enumerate() {
            if step.agent.is_empty() {
                bail!("step {}: agent is required", i + 1);
            }
            if step.stage.as_deref().map(|s| s.is_empty()).unwrap_or(true) {
                bail!("step {}: stage is required", i + 1);
            }
        }

        // At least one step must consume $INTAKE — otherwise the user's request is ignored
        let has_intake = self.steps.iter().any(|s| {
            s.nudge
                .as_deref()
                .map(|n| n.contains("$INTAKE"))
                .unwrap_or(false)
        });
        if !has_intake {
            bail!("pattern must include $INTAKE in at least one step's nudge — otherwise the user's request is ignored");
        }

        Ok(())
    }
}

// =============================================================================
// .loom text parser (CLI/workshop format)
// =============================================================================

/// Parse a .loom text pattern file.
///
/// Variable substitution is applied after tokenization, on each token value.
/// This prevents values containing `"`, `\n`, or `@` from corrupting the
/// tokenizer / stage extractor — the parser only ever sees the literal source.
pub fn parse(source: &str, vars: &[(&str, &str)]) -> Result<Pattern> {
    let mut steps = Vec::new();
    for (line_num, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();

        // Skip blanks, comments, and legacy gate markers
        if line.is_empty() || line.starts_with('#') || line == "---" {
            continue;
        }

        if !line.starts_with("do ") {
            bail!(
                "line {}: expected 'do' or comment, got: {}",
                line_num + 1,
                line
            );
        }

        steps.push(parse_do(line, line_num + 1, vars)?);
    }

    Ok(Pattern { steps })
}

/// Parse a `do` instruction line.
fn parse_do(line: &str, line_num: usize, vars: &[(&str, &str)]) -> Result<Step> {
    let sub = |text: &str| -> String {
        let mut out = text.to_string();
        for (name, value) in vars {
            out = out.replace(name, value);
        }
        out
    };

    let (line, stage) = extract_stage(line);
    let stage = stage.as_deref().map(&sub);
    let tokens = tokenize(&line).with_context(|| format!("line {}", line_num))?;
    if tokens.len() < 2 {
        bail!("line {}: 'do' requires at least an agent name", line_num);
    }

    let agent = sub(&tokens[1]);
    let mut nudge = None;
    let mut source = None;
    let mut system = None;

    let mut i = 2;
    while i < tokens.len() {
        match tokens[i].as_str() {
            "hears" => {
                if i + 1 >= tokens.len() {
                    bail!("line {}: 'hears' requires a source agent", line_num);
                }
                source = Some(sub(&tokens[i + 1]));
                i += 2;
            }
            "use" => {
                if i + 1 >= tokens.len() {
                    bail!("line {}: 'use' requires a system agent", line_num);
                }
                system = Some(sub(&tokens[i + 1]));
                i += 2;
            }
            _ => {
                if nudge.is_some() {
                    bail!("line {}: unexpected token '{}'", line_num, tokens[i]);
                }
                nudge = Some(sub(&tokens[i]));
                i += 1;
            }
        }
    }

    Ok(Step {
        agent,
        nudge,
        source,
        system,
        stage,
        model: None,
    })
}

/// Extract @stage-name from end of line. Returns (remaining, Option<stage>).
fn extract_stage(line: &str) -> (String, Option<String>) {
    let mut in_quotes = false;
    let mut last_at = None;
    for (i, ch) in line.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == '@' && !in_quotes {
            last_at = Some(i);
        }
    }

    if let Some(pos) = last_at {
        let before = line[..pos].trim_end().to_string();
        let stage = line[pos + 1..].trim().to_string();
        if stage.is_empty() {
            (before, None)
        } else {
            (before, Some(stage))
        }
    } else {
        (line.to_string(), None)
    }
}

/// Simple tokenizer: splits on whitespace but keeps quoted strings together.
fn tokenize(line: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if in_quotes {
        bail!("unterminated quote");
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

// =============================================================================
// Executor
// =============================================================================

/// Execute a pattern against a thread.
/// `on_before_turn` is called before each step — return Err to abort (e.g. cancellation).
pub fn execute(
    config: &Config,
    name: &str,
    pattern: &Pattern,
    model: Option<&str>,
    on_before_turn: Option<&BeforeTurnHook>,
) -> Result<()> {
    let thread = Thread::new(config, name);

    for (i, step) in pattern.steps.iter().enumerate() {
        // Auto-gate: check before every turn
        if let Some(ref check) = on_before_turn {
            check(i, step)?;
        }

        let stage_label = step.stage.as_deref().unwrap_or("");
        eprintln!(
            "[loom] step {}: do {}{}{}{}{}",
            i,
            step.agent,
            step.source
                .as_ref()
                .map(|s| format!(" hears {}", s))
                .unwrap_or_default(),
            step.nudge
                .as_ref()
                .map(|n| {
                    let preview = if n.len() > 60 {
                        let mut e = 60;
                        while !n.is_char_boundary(e) {
                            e -= 1;
                        }
                        &n[..e]
                    } else {
                        n
                    };
                    format!(" \"{}\"", preview)
                })
                .unwrap_or_default(),
            step.system
                .as_ref()
                .map(|s| format!(" use {}", s))
                .unwrap_or_default(),
            if stage_label.is_empty() {
                String::new()
            } else {
                format!("  @{}", stage_label)
            },
        );

        let step_model = step.model.as_deref().or(model);
        let result = thread.run_opts(&RunOpts {
            agent: &step.agent,
            nudge: step.nudge.as_deref(),
            source: step.source.as_deref(),
            system: step.system.as_deref(),
            model: step_model,
            stage: step.stage.as_deref(),
            timeout: None,
        })?;

        eprintln!(
            "[loom] step {}: {} chars, {:.0}s{}",
            i,
            result.output.len(),
            result.elapsed_s,
            result
                .cost_usd
                .map(|c| format!(", ${:.3}", c))
                .unwrap_or_default(),
        );
    }

    println!("{}", thread.show()?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let src = r#"
# A simple pattern
do axe "hello world"  @start
do bobby hears axe "review this"  @review
do axe hears bobby  @rebuttal
do axe "write memo" use writer  @memo
"#;
        let p = parse(src, &[]).unwrap();
        assert_eq!(p.steps.len(), 4);

        assert_eq!(p.steps[0].agent, "axe");
        assert_eq!(p.steps[0].nudge.as_deref(), Some("hello world"));
        assert!(p.steps[0].source.is_none());
        assert_eq!(p.steps[0].stage.as_deref(), Some("start"));

        assert_eq!(p.steps[1].agent, "bobby");
        assert_eq!(p.steps[1].source.as_deref(), Some("axe"));
        assert_eq!(p.steps[1].nudge.as_deref(), Some("review this"));

        assert_eq!(p.steps[2].agent, "axe");
        assert_eq!(p.steps[2].source.as_deref(), Some("bobby"));
        assert!(p.steps[2].nudge.is_none());

        assert_eq!(p.steps[3].system.as_deref(), Some("writer"));
        assert_eq!(p.steps[3].stage.as_deref(), Some("memo"));
    }

    #[test]
    fn parse_legacy_gates_ignored() {
        let src = "do axe \"hello\"\n---\ndo bobby \"world\"";
        let p = parse(src, &[]).unwrap();
        assert_eq!(p.steps.len(), 2); // gate silently ignored
    }

    #[test]
    fn parse_variable_substitution() {
        let src = r#"do axe "$INTAKE"  @start"#;
        let p = parse(src, &[("$INTAKE", "analyze NVDA")]).unwrap();
        assert_eq!(p.steps[0].nudge.as_deref(), Some("analyze NVDA"));
    }

    #[test]
    fn parse_variable_with_double_quotes() {
        // Var values containing `"` must not corrupt tokenization —
        // substitution happens after tokenizing.
        let src = r#"do axe "$INTAKE"  @start"#;
        let p = parse(src, &[("$INTAKE", r#"he said "hi""#)]).unwrap();
        assert_eq!(p.steps[0].nudge.as_deref(), Some(r#"he said "hi""#));
        assert_eq!(p.steps[0].stage.as_deref(), Some("start"));
    }

    #[test]
    fn parse_variable_with_newline() {
        // Multi-line var values must not be split across source lines.
        let src = r#"do axe "$INTAKE"  @start"#;
        let p = parse(src, &[("$INTAKE", "line1\nline2")]).unwrap();
        assert_eq!(p.steps[0].nudge.as_deref(), Some("line1\nline2"));
    }

    #[test]
    fn parse_variable_with_at_sign() {
        // `@` in var values must not be mistaken for a stage marker.
        let src = r#"do axe "$INTAKE"  @start"#;
        let p = parse(src, &[("$INTAKE", "mention @alice")]).unwrap();
        assert_eq!(p.steps[0].nudge.as_deref(), Some("mention @alice"));
        assert_eq!(p.steps[0].stage.as_deref(), Some("start"));
    }

    #[test]
    fn parse_hears_all() {
        let src = r#"do writer hears all "write the report""#;
        let p = parse(src, &[]).unwrap();
        assert_eq!(p.steps[0].agent, "writer");
        assert_eq!(p.steps[0].source.as_deref(), Some("all"));
        assert_eq!(p.steps[0].nudge.as_deref(), Some("write the report"));
    }

    #[test]
    fn parse_no_nudge() {
        let src = r#"do axe hears bobby"#;
        let p = parse(src, &[]).unwrap();
        assert_eq!(p.steps[0].agent, "axe");
        assert_eq!(p.steps[0].source.as_deref(), Some("bobby"));
        assert!(p.steps[0].nudge.is_none());
    }

    #[test]
    fn parse_error_missing_do() {
        assert!(parse("axe hello", &[]).is_err());
    }

    #[test]
    fn parse_error_unterminated_quote_has_line_num() {
        // Line 2 has an unterminated quote — error should mention the line.
        let src = "do axe \"ok\"\ndo bobby \"oops\ndo cat \"fine\"";
        let err = parse(src, &[]).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("line 2"), "error missing line number: {}", msg);
        assert!(
            msg.contains("unterminated quote"),
            "error missing cause: {}",
            msg
        );
    }

    #[test]
    fn parse_stage_extraction() {
        let (line, stage) = extract_stage(r#"do axe "hello @world"  @my-stage"#);
        assert_eq!(stage.as_deref(), Some("my-stage"));
        assert_eq!(line, r#"do axe "hello @world""#);
    }

    #[test]
    fn tokenize_basic() {
        let tokens = tokenize(r#"do axe "hello world" use writer"#).unwrap();
        assert_eq!(tokens, vec!["do", "axe", "hello world", "use", "writer"]);
    }

    // --- YAML tests ---

    #[test]
    fn yaml_parse_roundtrip() {
        let yaml = r#"
name: test-pattern
description: A test
steps:
  - agent: axe
    nudge: "hello world"
    stage: start
  - agent: bobby
    hears: axe
    nudge: "review this"
    stage: review
  - agent: axe
    nudge: "write memo"
    use: writer
    stage: memo
"#;
        let def = PatternDef::from_yaml(yaml).unwrap();
        assert_eq!(def.name, "test-pattern");
        assert_eq!(def.steps.len(), 3);
        assert_eq!(def.steps[0].agent, "axe");
        assert_eq!(def.steps[1].hears.as_deref(), Some("axe"));
        assert_eq!(def.steps[2].use_system.as_deref(), Some("writer"));

        // Roundtrip
        let out = def.to_yaml().unwrap();
        let def2 = PatternDef::from_yaml(&out).unwrap();
        assert_eq!(def2.name, "test-pattern");
        assert_eq!(def2.steps.len(), 3);
    }

    #[test]
    fn yaml_to_pattern_with_vars() {
        let def = PatternDef {
            name: "test".to_string(),
            description: String::new(),
            steps: vec![StepDef {
                agent: "axe".to_string(),
                nudge: Some("$INTAKE".to_string()),
                hears: None,
                use_system: None,
                stage: Some("start".to_string()),
                model: None,
            }],
        };
        let pattern = def.to_pattern(&[("$INTAKE", "analyze NVDA")]);
        assert_eq!(pattern.steps[0].nudge.as_deref(), Some("analyze NVDA"));
    }

    #[test]
    fn yaml_to_pattern_substitutes_all_fields() {
        // Matches .loom text parser: vars substitute in every string field.
        let def = PatternDef {
            name: "test".to_string(),
            description: String::new(),
            steps: vec![StepDef {
                agent: "axe".to_string(),
                nudge: Some("write draft id: $NAME".to_string()),
                hears: None,
                use_system: None,
                stage: Some("stage-$NAME".to_string()),
                model: Some("$MODEL".to_string()),
            }],
        };
        let pattern = def.to_pattern(&[("$NAME", "abc"), ("$MODEL", "haiku")]);
        assert_eq!(
            pattern.steps[0].nudge.as_deref(),
            Some("write draft id: abc")
        );
        assert_eq!(pattern.steps[0].stage.as_deref(), Some("stage-abc"));
        assert_eq!(pattern.steps[0].model.as_deref(), Some("haiku"));
    }

    #[test]
    fn yaml_validate() {
        let mut def = PatternDef {
            name: String::new(),
            description: String::new(),
            steps: vec![],
        };
        assert!(def.validate().is_err()); // empty name

        def.name = "test".to_string();
        assert!(def.validate().is_err()); // empty steps

        def.steps.push(StepDef {
            agent: String::new(),
            nudge: None,
            hears: None,
            use_system: None,
            stage: None,
            model: None,
        });
        assert!(def.validate().is_err()); // empty agent

        def.steps[0].agent = "axe".to_string();
        assert!(def.validate().is_err()); // missing stage

        def.steps[0].stage = Some("research".to_string());
        assert!(def.validate().is_err()); // missing $INTAKE

        def.steps[0].nudge = Some("$INTAKE".to_string());
        assert!(def.validate().is_ok());
    }
}
