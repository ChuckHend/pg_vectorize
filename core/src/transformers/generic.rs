use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::env;

use crate::{
    transformers::types::{EmbeddingPayload, EmbeddingRequest, Inputs},
    types,
};

use crate::transformers::providers::openai::trim_inputs;

lazy_static! {
    static ref REGEX: Regex = Regex::new(r"\$\{([^}]+)\}").expect("Invalid regex");
}

// finds all placeholders in a string
pub fn find_placeholders(var: &str) -> Option<Vec<String>> {
    let placeholders: HashSet<String> = REGEX
        .captures_iter(var)
        .filter_map(|cap| cap.get(1))
        .map(|match_| match_.as_str().to_owned())
        .collect();
    if placeholders.is_empty() {
        None
    } else {
        Some(placeholders.into_iter().collect())
    }
}

// interpolates a string with given env vars
pub fn interpolate(base_str: &str, env_vars: Vec<String>) -> Result<String> {
    let mut interpolated_str = base_str.to_string();
    for p in env_vars.iter() {
        let env_val = env::var(p).context(format!("failed to get env var: {}", p))?;
        interpolated_str = interpolated_str.replace(&format!("${{{}}}", p), &env_val);
    }
    Ok(interpolated_str)
}

pub fn prepare_generic_embedding_request(
    job_meta: types::VectorizeMeta,
    inputs: &[Inputs],
    url: String,
) -> Result<EmbeddingRequest> {
    let text_inputs = trim_inputs(inputs);
    let payload = EmbeddingPayload {
        input: text_inputs,
        model: job_meta.transformer.to_string(),
    };

    let job_params: types::JobParams = serde_json::from_value(job_meta.params)?;

    Ok(EmbeddingRequest {
        url,
        payload,
        api_key: job_params.api_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_placeholders() {
        let base_str = "http://${TEST_ENV_0}/test/${TEST_ENV_1}";
        let placeholders = find_placeholders(base_str).unwrap();
        assert!(placeholders.contains(&"TEST_ENV_0".to_owned()));
        assert!(placeholders.contains(&"TEST_ENV_1".to_owned()));

        // no placeholders
        let base_str = "http://TEST_ENV_0/test/TEST_ENV_1";
        let placeholders = find_placeholders(base_str);
        assert!(placeholders.is_none());
    }

    #[test]
    fn test_interpolate() {
        unsafe {
            env::set_var("TEST_ENV_0", "A");
            env::set_var("TEST_ENV_1", "B");
        }
        let base_str = "http://${TEST_ENV_0}/test/${TEST_ENV_1}";
        let interpolated = interpolate(
            base_str,
            vec!["TEST_ENV_0".to_string(), "TEST_ENV_1".to_string()],
        )
        .unwrap();
        assert_eq!(interpolated, "http://A/test/B");

        // change order
        let base_str = "http://${TEST_ENV_1}/test/${TEST_ENV_0}";
        let interpolated = interpolate(
            base_str,
            vec!["TEST_ENV_0".to_string(), "TEST_ENV_1".to_string()],
        )
        .unwrap();
        assert_eq!(interpolated, "http://B/test/A");

        // repeated str
        let base_str = "http://${TEST_ENV_0}/test/${TEST_ENV_1}/${TEST_ENV_0}";
        let interpolated = interpolate(
            base_str,
            vec!["TEST_ENV_0".to_string(), "TEST_ENV_1".to_string()],
        )
        .unwrap();
        assert_eq!(interpolated, "http://A/test/B/A");

        // missing env var should err
        let base_str = "http://${TEST_ENV_0}/test/${TEST_ENV_1}/${TEST_ENV_2}";
        let interpolated = interpolate(
            base_str,
            vec![
                "TEST_ENV_0".to_string(),
                "TEST_ENV_1".to_string(),
                "TEST_ENV_2".to_string(),
            ],
        );
        assert!(interpolated.is_err());
    }
}
