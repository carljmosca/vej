use anyhow::{Context, Result, anyhow};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::models::{TemplateStyle, find_model_spec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRequest {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub state: serde_json::Value,
    #[serde(default)]
    pub choices: Option<Vec<String>>,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub questions: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_temperature() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionSpec {
    #[serde(rename = "type", default = "default_q_type")]
    pub q_type: String,
    pub instructions: Option<serde_json::Value>,
    pub criteria: Option<serde_json::Value>,
    pub options: Option<serde_json::Value>,
    pub choices: Option<serde_json::Value>,
    pub levels: Option<serde_json::Value>,
}

fn default_q_type() -> String {
    "choice".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedResponse {
    Jev(JevResponse),
    Flat(DecisionResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevResponse {
    pub model: String,
    pub answers: serde_json::Map<String, serde_json::Value>,
    pub usage: UsageInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerOutput {
    #[serde(rename = "type")]
    pub q_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noul: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    pub probabilities: HashMap<String, f32>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub decision: String,
    pub confidence: f32,
    pub probabilities: HashMap<String, f32>,
    pub model: String,
    pub latency_ms: f64,
    pub tokens_evaluated: usize,
}

#[derive(Debug, Clone)]
pub struct ChoiceItem {
    pub key: String,
    pub display: String,
}

pub struct DecisionEngine {
    backend: Arc<LlamaBackend>,
    model: Arc<LlamaModel>,
    model_id: String,
    template: TemplateStyle,
    threads: u32,
    n_ctx: u32,
}

impl DecisionEngine {
    pub fn load(
        model_path: &Path,
        model_id: String,
        template: Option<TemplateStyle>,
        n_gpu_layers: u32,
        threads: u32,
        n_ctx: u32,
    ) -> Result<Self> {
        let backend = Arc::new(
            LlamaBackend::init().map_err(|e| anyhow!("Failed to initialize llama backend: {:?}", e))?,
        );

        let model_params = LlamaModelParams::default()
            .with_n_gpu_layers(n_gpu_layers)
            .with_use_mmap(true);

        tracing::info!(
            model_id = %model_id,
            path = ?model_path,
            n_gpu_layers = n_gpu_layers,
            "Loading model into memory"
        );

        let start = Instant::now();
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .with_context(|| format!("Failed to load GGUF model from {:?}", model_path))?;
        tracing::info!(elapsed = ?start.elapsed(), "Model successfully loaded");

        let resolved_template = template.unwrap_or_else(|| {
            if let Some(spec) = find_model_spec(&model_id) {
                spec.template
            } else if model_id.to_lowercase().contains("gemma-4") || model_id.to_lowercase().contains("gemma4") {
                TemplateStyle::Gemma4
            } else if model_id.to_lowercase().contains("llama") {
                TemplateStyle::Llama3
            } else if model_id.to_lowercase().contains("gemma") {
                TemplateStyle::Gemma
            } else {
                TemplateStyle::ChatML
            }
        });

        Ok(Self {
            backend,
            model: Arc::new(model),
            model_id,
            template: resolved_template,
            threads,
            n_ctx,
        })
    }

    pub fn current_model(&self) -> &str {
        &self.model_id
    }

    pub fn format_prompt(
        &self,
        instruction: Option<&str>,
        state_str: &str,
        choices: &[ChoiceItem],
    ) -> String {
        let is_boolean = choices.len() == 2
            && choices.iter().any(|c| c.key.eq_ignore_ascii_case("true"))
            && choices.iter().any(|c| c.key.eq_ignore_ascii_case("false"));

        let is_numeric = !is_boolean && choices.iter().all(|c| c.key.parse::<u32>().is_ok());

        let letters = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P"];

        let (system_msg, formatted_options, selection_prompt) = if is_boolean {
            let mut opts = String::new();
            for item in choices {
                opts.push_str(&format!("\n- {}: {}", item.key, item.display));
            }
            (
                "You are a precise, low-latency decision system. Evaluate the input state and criteria, then respond with true or false.",
                opts,
                "Select true or false:".to_string(),
            )
        } else if is_numeric {
            let mut opts = String::new();
            for item in choices {
                opts.push_str(&format!("\n{}) {}", item.key, item.display));
            }
            let num_list = choices
                .iter()
                .map(|c| c.key.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            (
                "You are a precise, low-latency scoring system. Evaluate the input state and criteria, then respond with the single score number.",
                opts,
                format!("Select the best score from [{}]:", num_list),
            )
        } else {
            let mut opts = String::new();
            for (idx, item) in choices.iter().enumerate() {
                let letter = letters.get(idx).copied().unwrap_or("?");
                opts.push_str(&format!("\n{letter}) {}", item.display));
            }
            let letter_list = choices
                .iter()
                .enumerate()
                .map(|(idx, _)| letters.get(idx).copied().unwrap_or("?"))
                .collect::<Vec<_>>()
                .join(", ");
            (
                "You are a precise, low-latency classification system. Evaluate the input state and criteria, then respond with ONLY the single letter of the best option.",
                opts,
                format!("Select the best option from [{}]:", letter_list),
            )
        };

        let instr = instruction.unwrap_or(
            "Analyze the state and choose the single best option.",
        );

        match self.template {
            TemplateStyle::Llama3 => format!(
                "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{}\n<|eot_id|><|start_header_id|>user<|end_header_id|>\n\nInput State:\n\"\"\"\n{}\n\"\"\"\n\nTask: {}\n\nAvailable Options:{}\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\nAnswer: ",
                system_msg, state_str, instr, formatted_options, selection_prompt
            ),
            TemplateStyle::ChatML => format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\nInput State:\n\"\"\"\n{}\n\"\"\"\n\nTask: {}\n\nAvailable Options:{}\n\n{}<|im_end|>\n<|im_start|>assistant\nAnswer: ",
                system_msg, state_str, instr, formatted_options, selection_prompt
            ),
            TemplateStyle::Gemma => format!(
                "<start_of_turn>user\n{}\n\nInput State:\n\"\"\"\n{}\n\"\"\"\n\nTask: {}\n\nAvailable Options:{}\n\n{}<end_of_turn>\n<start_of_turn>model\nAnswer: ",
                system_msg, state_str, instr, formatted_options, selection_prompt
            ),
            TemplateStyle::Gemma4 => format!(
                "<|turn>system\n{}<turn|>\n<|turn>user\nInput State:\n\"\"\"\n{}\n\"\"\"\n\nTask: {}\n\nAvailable Options:{}\n\n{}<turn|>\n<|turn>model\nAnswer: ",
                system_msg, state_str, instr, formatted_options, selection_prompt
            ),
        }
    }

    pub fn evaluate_single(
        &self,
        instruction: Option<&str>,
        state_str: &str,
        choices: &[ChoiceItem],
        temperature: f32,
    ) -> Result<DecisionResponse> {
        if choices.is_empty() {
            return Err(anyhow!("Choices list cannot be empty"));
        }

        let start_time = Instant::now();

        // 1. Format prompt
        let prompt = self.format_prompt(instruction, state_str, choices);

        // 2. Tokenize prompt (avoid double BOS for templates that already include begin-of-text)
        let add_bos = match self.template {
            TemplateStyle::Llama3 | TemplateStyle::ChatML | TemplateStyle::Gemma4 => AddBos::Never,
            TemplateStyle::Gemma => AddBos::Always,
        };
        let prompt_tokens = self
            .model
            .str_to_token(&prompt, add_bos)
            .map_err(|e| anyhow!("Failed to tokenize prompt: {:?}", e))?;

        let n_tokens = prompt_tokens.len();
        if n_tokens == 0 {
            return Err(anyhow!("Prompt tokenized to zero tokens"));
        }

        // 3. Create context for single forward pass
        let ctx_size = (n_tokens + 64).max(self.n_ctx as usize);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(ctx_size as u32))
            .with_n_batch(ctx_size as u32)
            .with_n_threads(self.threads as i32);

        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow!("Failed to create inference context: {:?}", e))?;

        // 4. Batch add: prompt prefill only, zero decode steps!
        let mut batch = LlamaBatch::new(n_tokens + 16, 1);
        batch
            .add_sequence(&prompt_tokens, 0, false)
            .map_err(|e| anyhow!("Failed to construct batch: {:?}", e))?;

        // 5. Execute single forward pass
        ctx.decode(&mut batch)
            .map_err(|e| anyhow!("Inference prefill failed: {:?}", e))?;

        // 6. Read logits for the last token position
        let logits = ctx.get_logits();


        // 7. Find candidate token logits for each choice using uniform, balanced token spaces
        let is_boolean = choices.len() == 2
            && choices.iter().any(|c| c.key.eq_ignore_ascii_case("true"))
            && choices.iter().any(|c| c.key.eq_ignore_ascii_case("false"));

        let is_numeric = !is_boolean && choices.iter().all(|c| c.key.parse::<u32>().is_ok());
        let letters = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P"];

        let mut candidate_logits: Vec<f32> = Vec::with_capacity(choices.len());

        for (idx, item) in choices.iter().enumerate() {
            let mut variants = Vec::new();

            if is_boolean {
                if item.key.eq_ignore_ascii_case("true") {
                    variants.push(" true".to_string());
                    variants.push("true".to_string());
                    variants.push(" True".to_string());
                    variants.push("True".to_string());
                } else {
                    variants.push(" false".to_string());
                    variants.push("false".to_string());
                    variants.push(" False".to_string());
                    variants.push("False".to_string());
                }
            } else if is_numeric {
                let k = item.key.trim();
                variants.push(format!(" {k}"));
                variants.push(k.to_string());
            } else {
                // Strictly evaluate corresponding letter tokens (A, B, C...) to eliminate vocabulary prior distortion
                if let Some(&letter) = letters.get(idx) {
                    variants.push(format!(" {letter}"));
                    variants.push(letter.to_string());
                    variants.push(format!(" {}", letter.to_lowercase()));
                    variants.push(letter.to_lowercase());
                }
            }

            let mut best_logit = f32::NEG_INFINITY;
            for v in &variants {
                if let Ok(tokens) = self.model.str_to_token(v, AddBos::Never) {
                    if let Some(tok) = tokens.first() {
                        let l = logits[tok.0 as usize];
                        if l > best_logit {
                            best_logit = l;
                        }
                    }
                }
            }

            if best_logit == f32::NEG_INFINITY {
                best_logit = -100.0;
            }

            candidate_logits.push(best_logit);
        }

        // 8. Restricted Softmax calculation
        let temp = if temperature <= 0.0 { 1.0 } else { temperature };

        let max_logit = candidate_logits
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);

        let exp_sum: f32 = candidate_logits
            .iter()
            .map(|&l| ((l - max_logit) / temp).exp())
            .sum();

        let mut probabilities = HashMap::new();
        let mut best_decision = choices[0].key.clone();
        let mut max_prob = -1.0_f32;

        for (idx, item) in choices.iter().enumerate() {
            let prob = if exp_sum > 0.0 {
                ((candidate_logits[idx] - max_logit) / temp).exp() / exp_sum
            } else {
                1.0 / (choices.len() as f32)
            };

            if prob > max_prob {
                max_prob = prob;
                best_decision = item.key.clone();
            }

            probabilities.insert(item.key.clone(), prob);
        }

        let latency_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        Ok(DecisionResponse {
            decision: best_decision,
            confidence: max_prob,
            probabilities,
            model: self.model_id.clone(),
            latency_ms,
            tokens_evaluated: n_tokens,
        })
    }

    pub fn evaluate_unified(&self, req: &DecisionRequest) -> Result<UnifiedResponse> {
        let total_start = Instant::now();

        // Convert state Value to a clean string representation
        let state_str = match &req.state {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            val => serde_json::to_string_pretty(val)?,
        };

        if let Some(ref questions) = req.questions {
            if questions.is_empty() {
                return Err(anyhow!("'questions' map cannot be empty"));
            }

            let mut answers = serde_json::Map::new();
            let mut total_tokens = 0;

            for (q_name, q_val) in questions {
                let q_spec: QuestionSpec = serde_json::from_value(q_val.clone())
                    .map_err(|e| anyhow!("Failed to parse question '{}': {}", q_name, e))?;

                let mut choice_items = Vec::new();
                let is_score = q_spec.q_type.eq_ignore_ascii_case("score");

                let effective_crit = q_spec
                    .criteria
                    .as_ref()
                    .or(q_spec.options.as_ref())
                    .or(q_spec.choices.as_ref())
                    .or(q_spec.levels.as_ref());

                if let Some(crit) = effective_crit {
                    match crit {
                        serde_json::Value::Object(map) => {
                            for (k, v) in map {
                                let desc = v.as_str().unwrap_or("");
                                let mut display = k.clone();
                                if !desc.is_empty() {
                                    display.push_str(": ");
                                    display.push_str(desc);
                                }
                                choice_items.push(ChoiceItem {
                                    key: k.clone(),
                                    display,
                                });
                            }
                        }
                        serde_json::Value::Array(arr) => {
                            for (idx, item) in arr.iter().enumerate() {
                                let text = item.as_str().unwrap_or("");
                                let key = if is_score {
                                    idx.to_string()
                                } else {
                                    text.to_string()
                                };
                                let display = if is_score {
                                    if text.is_empty() {
                                        idx.to_string()
                                    } else {
                                        format!("{}: {}", idx, text)
                                    }
                                } else {
                                    text.to_string()
                                };
                                choice_items.push(ChoiceItem {
                                    key,
                                    display,
                                });
                            }
                        }
                        serde_json::Value::Number(num) if is_score => {
                            let n = num.as_u64().unwrap_or(6) as usize;
                            for lvl in 0..n {
                                choice_items.push(ChoiceItem {
                                    key: lvl.to_string(),
                                    display: format!("Level {}", lvl),
                                });
                            }
                        }
                        _ => {}
                    }
                }

                if choice_items.is_empty() {
                    if is_score {
                        // Default to 6 levels: 0..=5
                        for lvl in 0..=5 {
                            choice_items.push(ChoiceItem {
                                key: lvl.to_string(),
                                display: format!("Level {}", lvl),
                            });
                        }
                    } else if q_spec.q_type.eq_ignore_ascii_case("noul") {
                        choice_items.push(ChoiceItem {
                            key: "true".to_string(),
                            display: "true (yes)".to_string(),
                        });
                        choice_items.push(ChoiceItem {
                            key: "false".to_string(),
                            display: "false (no)".to_string(),
                        });
                    } else {
                        return Err(anyhow!("Question '{}' must define criteria with options", q_name));
                    }
                }

                // Format instructions: can be String or Object (e.g. { question: "...", today: "..." })
                let instr_str: Option<String> = match &q_spec.instructions {
                    Some(serde_json::Value::String(s)) => Some(s.clone()),
                    Some(serde_json::Value::Object(map)) => {
                        let mut parts = Vec::new();
                        if let Some(q) = map.get("question").and_then(|v| v.as_str()) {
                            parts.push(q.to_string());
                        }
                        for (k, v) in map {
                            if k != "question" {
                                if let Some(s) = v.as_str() {
                                    parts.push(format!("{k}: {s}"));
                                } else {
                                    parts.push(format!("{k}: {v}"));
                                }
                            }
                        }
                        Some(parts.join("\n"))
                    }
                    Some(val) => Some(val.to_string()),
                    None => None,
                };

                let single_res = self.evaluate_single(
                    instr_str.as_deref(),
                    &state_str,
                    &choice_items,
                    req.temperature,
                )?;

                total_tokens += single_res.tokens_evaluated;

                let noul_val = if q_spec.q_type.eq_ignore_ascii_case("noul") {
                    single_res.probabilities.get("true").copied()
                } else {
                    None
                };

                let (score_val, display_choice) = if is_score {
                    let mut sum_score = 0.0f32;
                    for (k, prob) in &single_res.probabilities {
                        if let Ok(lvl) = k.parse::<f32>() {
                            sum_score += lvl * prob;
                        }
                    }
                    let rounded = (sum_score * 100.0).round() / 100.0;
                    (Some(rounded), Some(format!("{:.2}", rounded)))
                } else {
                    (None, Some(single_res.decision))
                };

                let answer_val = serde_json::to_value(AnswerOutput {
                    q_type: q_spec.q_type.clone(),
                    choice: display_choice,
                    noul: noul_val,
                    score: score_val,
                    probabilities: single_res.probabilities,
                    confidence: single_res.confidence,
                })?;

                answers.insert(q_name.clone(), answer_val);
            }

            let latency_ms = total_start.elapsed().as_secs_f64() * 1000.0;

            Ok(UnifiedResponse::Jev(JevResponse {
                model: self.model_id.clone(),
                answers,
                usage: UsageInfo {
                    input_tokens: total_tokens,
                    output_tokens: 0,
                },
                latency_ms: Some(latency_ms),
            }))
        } else if let Some(ref choices) = req.choices {
            let choice_items: Vec<ChoiceItem> = choices
                .iter()
                .map(|c| ChoiceItem {
                    key: c.clone(),
                    display: c.clone(),
                })
                .collect();

            let res = self.evaluate_single(
                req.instruction.as_deref(),
                &state_str,
                &choice_items,
                req.temperature,
            )?;

            Ok(UnifiedResponse::Flat(res))
        } else {
            Err(anyhow!(
                "Request must provide either 'questions' (Jev format) or 'choices' (flat format)"
            ))
        }
    }
}
