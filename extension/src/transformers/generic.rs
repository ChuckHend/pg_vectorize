use anyhow::Result;

use vectorize_core::transformers::generic::{find_placeholders, interpolate};

/// Interpolates environment variables into a string
/// if env var is missing, the placeholder is left as a raw string
pub fn env_interpolate_string(input: &str) -> Result<String> {
    if let Some(phs) = find_placeholders(input) {
        let interpolated = interpolate(input, phs)?;
        Ok(interpolated)
    } else {
        Ok(input.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_interpolate_string() {
        let input = "http://${HOST}:8000";
        // set env var
        std::env::set_var("HOST", "localhost");
        let result = env_interpolate_string(input).unwrap();
        assert_eq!(result, "http://localhost:8000");
    }

    #[test]
    fn test_env_interpolate_string_with_placeholder() {
        // a missing env var results in the placeholder being left as a raw string
        let input = "http://localhost:8000/embeddings/{model_name}";
        let result = env_interpolate_string(input);
        assert_eq!(result.unwrap(), input.to_string());
    }
}
