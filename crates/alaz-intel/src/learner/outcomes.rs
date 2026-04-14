use alaz_core::Result;
use alaz_db::repos::ProcedureRepo;
use serde::Deserialize;
use tracing::{debug, info, warn};

/// LLM response for procedure outcome detection.
#[derive(Deserialize)]
struct ProcedureOutcomeResult {
    #[serde(default)]
    outcomes: Vec<DetectedOutcome>,
}

#[derive(Deserialize)]
struct DetectedOutcome {
    /// The procedure ID that was followed.
    id: String,
    /// Whether the procedure was executed successfully.
    success: bool,
}

const PROCEDURE_OUTCOME_PROMPT: &str = r#"You are analyzing a session transcript to detect which known procedures were followed.

Below is a list of known procedures (ID: title). For each procedure that was actually executed or followed in this session, determine if it succeeded or failed.

RULES:
- Only include procedures that were CLEARLY followed in the transcript
- A procedure "succeeded" if the steps completed without errors
- A procedure "failed" if it encountered errors or was abandoned
- Do NOT guess — only report procedures you're confident were used
- If no procedures were followed, return an empty list

Known procedures:
{procedures}

Return ONLY valid JSON:
{"outcomes": [{"id": "procedure_id", "success": true/false}]}
"#;

impl super::SessionLearner {
    /// Detect which existing procedures were followed in this session and record outcomes.
    ///
    /// Fetches recent procedures for the project, asks the LLM to match them against
    /// the transcript, and calls `record_outcome` for each detected usage.
    pub(crate) async fn detect_procedure_outcomes(
        &self,
        transcript: &str,
        project_id: Option<&str>,
    ) -> Result<usize> {
        // Only match procedures within the same project to avoid cross-project
        // false positives. If no project_id, skip outcome detection entirely.
        let Some(pid) = project_id else {
            debug!("no project_id, skipping procedure outcome detection");
            return Ok(0);
        };
        let filter = alaz_core::models::ListProceduresFilter {
            project: Some(pid.to_string()),
            limit: Some(50),
            ..Default::default()
        };
        let procedures = ProcedureRepo::list(&self.pool, &filter).await?;

        if procedures.is_empty() {
            return Ok(0);
        }

        // Build procedure list for the prompt
        let proc_list: String = procedures
            .iter()
            .map(|p| format!("- {}: {}", p.id, p.title))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = PROCEDURE_OUTCOME_PROMPT.replace("{procedures}", &proc_list);

        // Use the tail of the transcript (~8K chars / ~32KB) — most relevant for outcomes
        let byte_offset = transcript.len().saturating_sub(32000);
        let start = if byte_offset == 0 {
            0
        } else {
            let mut i = byte_offset;
            while i < transcript.len() && !transcript.is_char_boundary(i) {
                i += 1;
            }
            i
        };
        let transcript_tail = &transcript[start..];

        let result = match self
            .llm
            .chat_json::<ProcedureOutcomeResult>(&prompt, transcript_tail, 0.1)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "LLM procedure outcome detection failed");
                return Ok(0);
            }
        };

        // Validate IDs against actual procedure IDs to prevent hallucination
        let valid_ids: std::collections::HashSet<&str> =
            procedures.iter().map(|p| p.id.as_str()).collect();

        let mut recorded = 0usize;
        for outcome in &result.outcomes {
            if !valid_ids.contains(outcome.id.as_str()) {
                debug!(id = %outcome.id, "ignoring hallucinated procedure ID");
                continue;
            }

            match ProcedureRepo::record_outcome(&self.pool, &outcome.id, outcome.success).await {
                Ok(()) => {
                    info!(
                        id = %outcome.id,
                        success = outcome.success,
                        "recorded procedure outcome"
                    );
                    recorded += 1;
                }
                Err(e) => {
                    warn!(id = %outcome.id, error = %e, "failed to record procedure outcome");
                }
            }
        }

        if recorded > 0 {
            info!(recorded, "procedure outcome detection complete");
        }

        Ok(recorded)
    }
}
