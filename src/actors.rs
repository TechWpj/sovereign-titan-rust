//! Cognitive Actors — Tokio-based actor loops for each brain in the swarm.
//!
//! Each actor runs as an independent `tokio::spawn` task communicating via
//! `mpsc` channels, achieving true memory-safe concurrency without segfaults.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use crate::cognitive::metacognition;
use crate::cognitive::working_memory::WorkingMemory;
use crate::knowledge::graph::KnowledgeGraph;
use crate::memory::ledger::Ledger;
use crate::messages::{CognitiveMessage, SubconsciousCommand, WardenCommand};
use crate::nexus::{ModelNexus, ModelTarget};

// ─────────────────────────────────────────────────────────────────────────────
// Prime Actor (14B GPU)
// ─────────────────────────────────────────────────────────────────────────────

/// The Prime actor — main user-facing inference brain.
///
/// Listens on an `mpsc::Receiver<CognitiveMessage>` and handles:
/// - `UserChat`: generates a response via the Nexus and replies.
/// - `SecurityAlert`: interrupts current work to address the threat.
/// - `SubconsciousInsight`: stores insights for context enrichment.
pub async fn prime_actor(
    nexus: Arc<ModelNexus>,
    mut rx: mpsc::Receiver<CognitiveMessage>,
    ledger: Arc<Ledger>,
    knowledge: Arc<Mutex<KnowledgeGraph>>,
) {
    info!("Prime actor started (with memory + knowledge + working memory)");

    // Rolling buffer of subconscious insights for context enrichment.
    let mut insights: Vec<String> = Vec::new();
    const MAX_INSIGHTS: usize = 10;

    // Working memory for cross-turn context continuity.
    let mut working_memory = WorkingMemory::with_defaults();

    while let Some(msg) = rx.recv().await {
        match msg {
            CognitiveMessage::UserChat { text, reply } => {
                info!("Prime: received user chat ({} chars)", text.len());

                // ── Store user message in working memory ────────────────
                working_memory.add(&text, "user", 0.8);

                // ── Load memory context ──────────────────────────────────
                let mut context_parts: Vec<String> = Vec::new();

                // Working memory block (highest salience items).
                let wm_block = working_memory.get_prompt_block();
                if !wm_block.is_empty() {
                    context_parts.push(wm_block);
                }

                // Ledger summary (goals, preferences, status).
                let ledger_summary = ledger.get_summary();
                if !ledger_summary.is_empty() {
                    context_parts.push(format!("[Memory]\n{ledger_summary}"));
                }

                // Knowledge graph context — extract entity names from the query
                // and look up adjacent knowledge.
                {
                    let kg = knowledge.lock().await;
                    for word in text.split_whitespace() {
                        let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
                        if clean.len() >= 3 {
                            if kg.find_entity(clean).is_some() {
                                let kg_ctx = kg.get_context(clean);
                                context_parts.push(format!("[Knowledge: {clean}]\n{kg_ctx}"));
                            }
                        }
                    }
                }

                // Subconscious insights.
                if !insights.is_empty() {
                    context_parts.push(format!(
                        "[Background awareness]\n{}",
                        insights.join("\n")
                    ));
                }

                // Build final prompt.
                let user_question = text.clone();
                let prompt = if context_parts.is_empty() {
                    text
                } else {
                    let context = context_parts.join("\n\n");
                    format!("{context}\n\n[User]\n{text}")
                };

                // ── Metacognitive verified generation ────────────────────
                match metacognition::verified_generate(
                    &nexus,
                    &prompt,
                    &user_question,
                    512,
                    0.7,
                )
                .await
                {
                    Ok(response) => {
                        // Store the response in working memory for continuity.
                        working_memory.add(&response, "assistant", 0.6);
                        let _ = reply.send(response);
                    }
                    Err(e) => {
                        error!("Prime generation failed: {e:#}");
                        let _ = reply.send(format!("[ERROR] Generation failed: {e}"));
                    }
                }
            }

            CognitiveMessage::SecurityAlert {
                threat_level,
                details,
            } => {
                warn!("Prime: SECURITY INTERRUPT — level={threat_level}, details={details}");

                // Generate a security response immediately.
                let prompt = format!(
                    "[SECURITY ALERT — PRIORITY INTERRUPT]\n\
                     Threat Level: {threat_level}\n\
                     Details: {details}\n\n\
                     Analyze this threat and recommend immediate action."
                );

                match nexus.generate(&prompt, ModelTarget::Prime, 256, 0.3).await {
                    Ok(response) => {
                        info!("Prime security response: {response}");
                    }
                    Err(e) => {
                        error!("Prime security response failed: {e:#}");
                    }
                }
            }

            CognitiveMessage::SubconsciousInsight { memory_summary } => {
                info!(
                    "Prime: received subconscious insight ({} chars)",
                    memory_summary.len()
                );
                insights.push(memory_summary);
                if insights.len() > MAX_INSIGHTS {
                    insights.remove(0);
                }
            }
        }
    }

    info!("Prime actor stopped (channel closed)");
}

// ─────────────────────────────────────────────────────────────────────────────
// Subconscious Actor (3B CPU)
// ─────────────────────────────────────────────────────────────────────────────

/// The Subconscious actor — background inner monologue brain.
///
/// Runs an endless loop: sleep → reflect → send insight to Prime.
/// Can also be triggered on-demand via `SubconsciousCommand::Reflect`.
pub async fn subconscious_actor(
    nexus: Arc<ModelNexus>,
    prime_tx: mpsc::Sender<CognitiveMessage>,
    mut cmd_rx: mpsc::Receiver<SubconsciousCommand>,
    interval: Duration,
) {
    info!(
        "Subconscious actor started (interval={}s)",
        interval.as_secs()
    );

    loop {
        // Wait for either the interval to elapse or an explicit command.
        let context = tokio::select! {
            _ = tokio::time::sleep(interval) => {
                // Periodic autonomous reflection.
                String::from(
                    "Reflect on your current state. What patterns do you notice? \
                     What should you be aware of? Summarize any important insights."
                )
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(SubconsciousCommand::Reflect { context }) => context,
                    Some(SubconsciousCommand::Shutdown) | None => {
                        info!("Subconscious actor shutting down");
                        return;
                    }
                }
            }
        };

        // Generate an insight using the 3B subconscious model.
        match nexus
            .generate(&context, ModelTarget::Subconscious, 256, 0.6)
            .await
        {
            Ok(insight) => {
                if !insight.trim().is_empty() {
                    info!(
                        "Subconscious insight: {}...",
                        &insight[..insight.len().min(80)]
                    );
                    let msg = CognitiveMessage::SubconsciousInsight {
                        memory_summary: insight,
                    };
                    if prime_tx.send(msg).await.is_err() {
                        warn!("Subconscious: Prime channel closed, stopping");
                        return;
                    }
                }
            }
            Err(e) => {
                warn!("Subconscious generation failed: {e:#}");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Warden Actor (3B CPU)
// ─────────────────────────────────────────────────────────────────────────────

/// The Warden actor — autonomous security scanning brain.
///
/// Runs periodic security scans and sends `SecurityAlert` messages to Prime
/// when threats are detected.
pub async fn warden_actor(
    nexus: Arc<ModelNexus>,
    prime_tx: mpsc::Sender<CognitiveMessage>,
    mut cmd_rx: mpsc::Receiver<WardenCommand>,
    interval: Duration,
) {
    info!("Warden actor started (interval={}s)", interval.as_secs());

    loop {
        // Wait for either the scan interval or an explicit command.
        let context = tokio::select! {
            _ = tokio::time::sleep(interval) => {
                String::from(
                    "Perform a security assessment. Check for anomalies, \
                     suspicious processes, unusual network activity, or \
                     unauthorized access patterns. Report threat level: \
                     NONE, LOW, MEDIUM, HIGH, or CRITICAL."
                )
            }
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WardenCommand::Scan { context }) => context,
                    Some(WardenCommand::Shutdown) | None => {
                        info!("Warden actor shutting down");
                        return;
                    }
                }
            }
        };

        // Run the security scan via the 3B warden model.
        match nexus
            .generate(&context, ModelTarget::Warden, 256, 0.3)
            .await
        {
            Ok(assessment) => {
                // Parse threat level from response — look for keywords.
                let threat_level = if assessment.contains("CRITICAL") {
                    "CRITICAL"
                } else if assessment.contains("HIGH") {
                    "HIGH"
                } else if assessment.contains("MEDIUM") {
                    "MEDIUM"
                } else if assessment.contains("LOW") {
                    "LOW"
                } else {
                    "NONE"
                };

                if threat_level != "NONE" {
                    warn!("Warden: threat detected — level={threat_level}");
                    let msg = CognitiveMessage::SecurityAlert {
                        threat_level: threat_level.to_string(),
                        details: assessment,
                    };
                    if prime_tx.send(msg).await.is_err() {
                        warn!("Warden: Prime channel closed, stopping");
                        return;
                    }
                } else {
                    info!("Warden: perimeter nominal");
                }
            }
            Err(e) => {
                warn!("Warden scan failed: {e:#}");
            }
        }
    }
}
