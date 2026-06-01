use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::mpsc::Sender;
use std::thread;

/// what the worker thread sends back to the ui once an api call finishes.
pub enum AgentResult {
    Pattern(AgentPattern),
    Variations(AgentVariations),
    Error(String),
}

/// run generate_pattern on a worker thread. ui keeps the receiver and polls
/// it each frame, applying the result the moment it lands.
pub fn spawn_pattern(prompt: String, channels: String, tx: Sender<AgentResult>) {
    thread::spawn(move || {
        let result = match generate_pattern(&prompt, &channels) {
            Ok(p) => AgentResult::Pattern(p),
            Err(e) => AgentResult::Error(format!("{e}")),
        };
        let _ = tx.send(result);
    });
}

pub fn spawn_variations(
    hint: String,
    channels: String,
    current_pattern: String,
    tx: Sender<AgentResult>,
) {
    thread::spawn(move || {
        let result = match generate_variations(&hint, &channels, &current_pattern) {
            Ok(v) => AgentResult::Variations(v),
            Err(e) => AgentResult::Error(format!("{e}")),
        };
        let _ = tx.send(result);
    });
}

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-sonnet-4-6";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentPattern {
    pub channels: Vec<AgentChannelPattern>,
    #[serde(default)]
    pub bpm: Option<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentChannelPattern {
    pub index: u16,
    pub steps: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentVariations {
    pub variations: Vec<AgentPattern>,
}

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<Message<'a>>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ResponseBody {
    content: Vec<ResponseContent>,
}

#[derive(Deserialize)]
struct ResponseContent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

fn api_key() -> Result<String> {
    env::var("ANTHROPIC_API_KEY")
        .context("set ANTHROPIC_API_KEY in your env to use the agent")
}

fn call(system: &str, user: &str) -> Result<String> {
    let key = api_key()?;
    let body = RequestBody {
        model: MODEL,
        max_tokens: 2048,
        system,
        messages: vec![Message { role: "user", content: user }],
    };
    let resp = ureq::post(API_URL)
        .set("Content-Type", "application/json")
        .set("x-api-key", &key)
        .set("anthropic-version", API_VERSION)
        .timeout(std::time::Duration::from_secs(30))
        .send_json(serde_json::to_value(&body)?)
        .map_err(|e| anyhow!("anthropic request failed: {e}"))?;
    let parsed: ResponseBody = resp
        .into_json()
        .context("parsing anthropic response body")?;
    let text = parsed
        .content
        .into_iter()
        .find(|c| c.kind == "text")
        .map(|c| c.text)
        .ok_or_else(|| anyhow!("no text content in anthropic response"))?;
    Ok(strip_fence(&text).to_string())
}

fn strip_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim_end_matches("```").trim();
    }
    s
}

/// build a single string that lists the current channels so the model knows
/// what indices to fill and what kind of sound each one is.
pub fn channels_brief(channels: &[(u16, String, String)]) -> String {
    let mut out = String::new();
    for (idx, name, kind) in channels {
        out.push_str(&format!("{idx}: {name} ({kind})\n"));
    }
    out
}

/// describe the active pattern so the model can vary it.
pub fn pattern_brief(channels: &[(u16, String, String, Vec<u8>)]) -> String {
    let mut out = String::new();
    for (idx, name, kind, steps) in channels {
        let s: Vec<String> = steps.iter().map(|b| b.to_string()).collect();
        out.push_str(&format!("{idx}: {name} ({kind}): {}\n", s.join(",")));
    }
    out
}

const PATTERN_SYSTEM: &str = r#"You generate 16-step drum machine patterns. Respond with valid JSON only. No markdown, no commentary, no code fences.

Schema:
{
  "channels": [
    {"index": 0, "steps": [1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0]}
  ],
  "bpm": 120
}

Rules:
- Each steps array must contain exactly 16 integers, each 0 or 1.
- index matches the project channel index given in the prompt.
- Include bpm only if the user mentions a tempo, otherwise omit the field.
- Omit channels that should be silent. Don't fill every step.
- Keep it musical. Common patterns: kick on 1+9, snare on 5+13, hat on the off-eighths or every step.
"#;

const VARIATION_SYSTEM: &str = r#"You generate musical variations of a 16-step drum pattern. Respond with valid JSON only. No markdown, no commentary, no code fences.

Schema:
{
  "variations": [
    {"channels": [{"index": 0, "steps": [1,0,0,0,1,0,0,0,1,0,0,0,1,0,0,0]}]}
  ]
}

Rules:
- Return exactly 3 variations.
- Each variation should feel related to the source but musically different. Move accents, add fills, change density, syncopate.
- Each steps array must contain exactly 16 integers, each 0 or 1.
- Don't return the input pattern unchanged.
"#;

pub fn generate_pattern(prompt: &str, channels: &str) -> Result<AgentPattern> {
    let user = format!(
        "Channels in this project:\n{channels}\nGenerate a pattern for: {prompt}\n"
    );
    let text = call(PATTERN_SYSTEM, &user)?;
    let parsed: AgentPattern = serde_json::from_str(&text)
        .with_context(|| format!("could not parse agent json: {text}"))?;
    Ok(sanitize(parsed))
}

pub fn generate_variations(
    extra_hint: &str,
    channels: &str,
    current_pattern: &str,
) -> Result<AgentVariations> {
    let user = format!(
        "Channels:\n{channels}\nCurrent pattern:\n{current_pattern}\n\
         Generate 3 variations.{}\n",
        if extra_hint.trim().is_empty() {
            String::new()
        } else {
            format!(" Direction: {}", extra_hint.trim())
        }
    );
    let text = call(VARIATION_SYSTEM, &user)?;
    let parsed: AgentVariations = serde_json::from_str(&text)
        .with_context(|| format!("could not parse agent json: {text}"))?;
    Ok(AgentVariations {
        variations: parsed.variations.into_iter().map(sanitize).collect(),
    })
}

fn sanitize(mut p: AgentPattern) -> AgentPattern {
    for ch in &mut p.channels {
        ch.steps.truncate(16);
        while ch.steps.len() < 16 {
            ch.steps.push(0);
        }
        for s in &mut ch.steps {
            *s = if *s != 0 { 1 } else { 0 };
        }
    }
    p
}
