pub const PLACEHOLDER_OPENAI_KEY: &str = "sk-placeholder-redacted-not-a-real-key";
pub const PLACEHOLDER_GITHUB_TOKEN: &str = "ghp_placeholder_redacted_not_a_real_token";

pub fn placeholder_count() -> usize {
    [PLACEHOLDER_OPENAI_KEY, PLACEHOLDER_GITHUB_TOKEN].len()
}
