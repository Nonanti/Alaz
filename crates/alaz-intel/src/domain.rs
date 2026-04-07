//! Content domain detection via keyword-based heuristics.
//! No LLM calls — fast classification for routing to appropriate extraction prompts.

use serde::{Deserialize, Serialize};

/// The detected domain of a piece of content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentDomain {
    Coding,
    Personal,
    Research,
    Health,
    Finance,
    General,
}

impl ContentDomain {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::Personal => "personal",
            Self::Research => "research",
            Self::Health => "health",
            Self::Finance => "finance",
            Self::General => "general",
        }
    }
}

impl std::fmt::Display for ContentDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detect the content domain using keyword scoring.
/// Returns the highest-scoring domain, or `General` if no clear signal.
pub fn detect_domain(content: &str) -> ContentDomain {
    let lower = content.to_lowercase();

    let scores = [
        (ContentDomain::Coding, score_coding(&lower)),
        (ContentDomain::Personal, score_personal(&lower)),
        (ContentDomain::Research, score_research(&lower)),
        (ContentDomain::Health, score_health(&lower)),
        (ContentDomain::Finance, score_finance(&lower)),
    ];

    let (best_domain, best_score) = scores
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .expect("scores array is non-empty");

    // Need at least 3 keyword hits to be confident
    if *best_score >= 3 {
        *best_domain
    } else {
        ContentDomain::General
    }
}

fn score_coding(text: &str) -> u32 {
    let keywords = [
        "fn ",
        "impl ",
        "async ",
        "struct ",
        "enum ",
        "pub ",
        "mod ",
        "error",
        "bug",
        "debug",
        "compile",
        "cargo",
        "npm",
        "git ",
        "function",
        "class ",
        "import ",
        "export ",
        "const ",
        "let ",
        "return ",
        "if ",
        "else ",
        "for ",
        "while ",
        "loop ",
        "api",
        "endpoint",
        "database",
        "query",
        "migration",
        "test",
        "assert",
        "deploy",
        "docker",
        "kubernetes",
        "refactor",
        "commit",
        "branch",
        "merge",
        "pull request",
        "stack trace",
        "exception",
        "null",
        "undefined",
        "```",
        "rust",
        "python",
        "javascript",
        "typescript",
    ];
    count_matches(text, &keywords)
}

fn score_personal(text: &str) -> u32 {
    let keywords = [
        "feel",
        "feeling",
        "today",
        "yesterday",
        "tomorrow",
        "plan",
        "goal",
        "want to",
        "need to",
        "should",
        "meeting",
        "call",
        "appointment",
        "schedule",
        "friend",
        "family",
        "relationship",
        "love",
        "happy",
        "sad",
        "birthday",
        "anniversary",
        "holiday",
        "vacation",
        "trip",
        "morning",
        "evening",
        "night",
        "weekend",
        "remember",
        "forget",
        "think about",
        "worry",
        "habit",
        "routine",
        "journal",
        "diary",
        "note to self",
        "grocery",
        "shopping",
        "cook",
        "recipe",
    ];
    count_matches(text, &keywords)
}

fn score_research(text: &str) -> u32 {
    let keywords = [
        "research",
        "study",
        "paper",
        "article",
        "journal",
        "hypothesis",
        "experiment",
        "data",
        "analysis",
        "result",
        "conclusion",
        "abstract",
        "methodology",
        "literature",
        "reference",
        "citation",
        "source",
        "evidence",
        "theory",
        "model",
        "framework",
        "concept",
        "university",
        "academic",
        "professor",
        "lecture",
        "book",
        "chapter",
        "page",
        "notes",
        "summary",
        "learn",
        "understand",
        "explore",
        "investigate",
    ];
    count_matches(text, &keywords)
}

fn score_health(text: &str) -> u32 {
    let keywords = [
        "health",
        "doctor",
        "hospital",
        "medicine",
        "prescription",
        "symptom",
        "diagnosis",
        "treatment",
        "therapy",
        "exercise",
        "workout",
        "gym",
        "run",
        "walk",
        "yoga",
        "diet",
        "nutrition",
        "calorie",
        "protein",
        "vitamin",
        "sleep",
        "rest",
        "stress",
        "anxiety",
        "meditation",
        "weight",
        "blood pressure",
        "heart rate",
        "bmi",
        "allergy",
        "pain",
        "headache",
        "fever",
    ];
    count_matches(text, &keywords)
}

fn score_finance(text: &str) -> u32 {
    let keywords = [
        "finance",
        "money",
        "budget",
        "expense",
        "income",
        "invest",
        "stock",
        "bond",
        "portfolio",
        "dividend",
        "bank",
        "account",
        "credit",
        "debit",
        "loan",
        "mortgage",
        "tax",
        "salary",
        "payment",
        "bill",
        "receipt",
        "saving",
        "retirement",
        "insurance",
        "asset",
        "crypto",
        "bitcoin",
        "ethereum",
        "trading",
        "price",
        "cost",
        "profit",
        "loss",
        "revenue",
    ];
    count_matches(text, &keywords)
}

fn count_matches(text: &str, keywords: &[&str]) -> u32 {
    keywords.iter().filter(|kw| text.contains(**kw)).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_coding() {
        let content = "I need to fix a bug in the async function. The cargo build fails with a compile error in the struct impl.";
        assert_eq!(detect_domain(content), ContentDomain::Coding);
    }

    #[test]
    fn test_detect_personal() {
        let content = "Today I had a meeting with a friend. I feel happy about the plan we made for the weekend vacation. Need to remember to call family tomorrow.";
        assert_eq!(detect_domain(content), ContentDomain::Personal);
    }

    #[test]
    fn test_detect_general() {
        let content = "The weather is nice.";
        assert_eq!(detect_domain(content), ContentDomain::General);
    }

    #[test]
    fn test_detect_finance() {
        let content = "I need to review my budget for this month. My income from salary covers the mortgage payment, but I should invest more in my stock portfolio.";
        assert_eq!(detect_domain(content), ContentDomain::Finance);
    }

    #[test]
    fn test_detect_health() {
        let content = "After my doctor appointment, I started a new exercise routine. I do yoga for stress relief and track my sleep and heart rate daily.";
        assert_eq!(detect_domain(content), ContentDomain::Health);
    }

    #[test]
    fn test_detect_mixed_domain_picks_highest() {
        // Content with both coding and personal keywords — coding has more
        let content = "Today I feel happy about fixing the async bug in the struct impl. \
                       The cargo build finally passes after the refactor of the function.";
        let domain = detect_domain(content);
        // Coding keywords: "async ", "bug", "struct ", "impl ", "cargo", "function", "refactor"
        // Personal keywords: "today", "feel", "happy"
        // Coding should win with more hits
        assert_eq!(domain, ContentDomain::Coding);
    }

    #[test]
    fn test_detect_turkish_coding_content() {
        // Turkish comments mixed with code keywords
        let content = "Bu fonksiyonu refactor ettim. Async struct impl üzerinde debug yaptım. \
                       Cargo test ile assert kontrol ettim. Git commit ve deploy tamamlandı.";
        assert_eq!(detect_domain(content), ContentDomain::Coding);
    }

    #[test]
    fn test_detect_empty_string() {
        assert_eq!(detect_domain(""), ContentDomain::General);
    }

    #[test]
    fn test_detect_very_short_string() {
        assert_eq!(detect_domain("hi"), ContentDomain::General);
    }

    #[test]
    fn test_detect_research() {
        let content = "The research paper presents a new methodology for data analysis. \
                       The hypothesis was tested through an experiment with promising results. \
                       The conclusion references previous literature.";
        assert_eq!(detect_domain(content), ContentDomain::Research);
    }
}
