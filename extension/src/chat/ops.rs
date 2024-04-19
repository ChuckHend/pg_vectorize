use crate::guc;
use crate::search;
use crate::util::get_vectorize_meta_spi;

use anyhow::{anyhow, Result};
use handlebars::Handlebars;
use openai_api_rs::v1::api::Client;
use openai_api_rs::v1::chat_completion::{self, ChatCompletionRequest};
use pgrx::prelude::*;
use vectorize_core::transformers::ollama::LLMFunctions;
use vectorize_core::transformers::ollama::OllamaInstance;
use vectorize_core::types::Model;
use vectorize_core::types::ModelSource;

use crate::chat::types::{ChatResponse, ContextualSearch, PromptTemplate, RenderedPrompt};
use tiktoken_rs::{get_bpe_from_model, model::get_context_size, CoreBPE};
use vectorize_core::types::{JobParams, VectorizeMeta};

pub fn call_chat(
    agent_name: &str,
    query: &str,
    chat_model: &Model,
    task: &str,
    api_key: Option<String>,
    num_context: i32,
    force_trim: bool,
) -> Result<ChatResponse> {
    // get job metadata
    let project_meta: VectorizeMeta = get_vectorize_meta_spi(agent_name)?;

    let job_params = serde_json::from_value::<JobParams>(project_meta.params.clone())
        .unwrap_or_else(|e| error!("failed to deserialize job params: {}", e));

    // for various token count estimations
    let bpe = match chat_model.source {
        ModelSource::Ollama => {
            // Using gpt-3.5-turbo tokenizer for Ollama since the library does not support llama2
            get_bpe_from_model("gpt-3.5-turbo").expect("failed to get BPE from model")
        }
        ModelSource::OpenAI => {
            get_bpe_from_model(&chat_model.name).expect("failed to get BPE from model")
        }
        ModelSource::SentenceTransformers => {
            error!("SentenceTransformers not supported for chat completions")
        }
    };

    // can only be 1 column in a chat job, for now, so safe to grab first element
    let content_column = job_params.columns[0].clone();
    let pk = job_params.primary_key;
    let columns = vec![pk.clone(), content_column.clone()];

    let raw_search = search::search(
        agent_name,
        query,
        api_key.clone(),
        columns,
        num_context,
        None,
    )?;

    let mut search_results: Vec<ContextualSearch> = Vec::new();
    for s in raw_search {
        let row_js: serde_json::Value = s.0;
        let record_id = row_js
            .get(&pk)
            .unwrap_or_else(|| error!("`{pk}` not found"));
        let content = row_js
            .get(&content_column)
            .unwrap_or_else(|| error!("`{content_column}` not found"));
        let text_content =
            serde_json::to_string(content).expect("failed to serialize content to string");
        let token_ct = bpe.encode_ordinary(&text_content).len() as i32;
        search_results.push(ContextualSearch {
            record_id: serde_json::to_string(record_id)
                .expect("failed to serialize record_id to string"),
            content: text_content,
            token_ct,
        });
    }

    // read prompt template
    let res_prompts: Result<PromptTemplate, spi::Error> = Spi::connect(|c| {
        let q = format!("select * from vectorize.prompts where prompt_type = '{task}'");
        let tup_table = c.select(&q, None, None)?;
        let mut sys_prompt = String::new();
        let mut user_prompt = String::new();
        for row in tup_table {
            sys_prompt = row["sys_prompt"]
                .value::<String>()?
                .expect("sys_prompt is null");
            user_prompt = row["user_prompt"]
                .value::<String>()?
                .expect("user_prompt is null");
        }
        Ok(PromptTemplate {
            sys_prompt,
            user_prompt,
        })
    });
    let p_ok = res_prompts?;

    let sys_prompt_template = p_ok.sys_prompt;
    let user_prompt_template = p_ok.user_prompt;

    let max_context_length = get_context_size(&chat_model.name) as i32;

    let rendered_prompt = prepared_prompt(
        &search_results,
        &sys_prompt_template,
        &user_prompt_template,
        query,
        force_trim,
        &bpe,
        max_context_length,
    )?;

    // http request to chat completions
    let chat_response = match chat_model.source {
        ModelSource::OpenAI => call_chat_completions(rendered_prompt, &chat_model.name, api_key)?,
        ModelSource::SentenceTransformers => {
            error!("SentenceTransformers not supported for chat completions");
        }
        ModelSource::Ollama => call_ollama_chat_completions(rendered_prompt, &chat_model.name)?,
    };

    Ok(ChatResponse {
        context: search_results,
        chat_response,
    })
}

fn render_user_message(user_prompt_template: &str, context: &str, query: &str) -> Result<String> {
    let handlebars = Handlebars::new();
    let render_vals = serde_json::json!({
        "context_str": context,
        "query_str": query,
    });
    let user_rendered: String = handlebars.render_template(user_prompt_template, &render_vals)?;
    Ok(user_rendered)
}

fn call_chat_completions(
    prompts: RenderedPrompt,
    model: &str,
    api_key: Option<String>,
) -> Result<String> {
    let openai_key = match api_key {
        Some(k) => k.to_string(),
        None => match guc::get_guc(guc::VectorizeGuc::OpenAIKey) {
            Some(k) => k,
            None => {
                error!("failed to get API key from GUC");
            }
        },
    };

    let client = Client::new(openai_key);
    let sys_msg = chat_completion::ChatCompletionMessage {
        role: chat_completion::MessageRole::system,
        content: chat_completion::Content::Text(prompts.sys_rendered),
        name: None,
    };
    let usr_msg = chat_completion::ChatCompletionMessage {
        role: chat_completion::MessageRole::user,
        content: chat_completion::Content::Text(prompts.user_rendered),
        name: None,
    };

    let req = ChatCompletionRequest::new(model.to_string(), vec![sys_msg, usr_msg]);
    let result = client.chat_completion(req)?;
    // currently we only support single query, and not a conversation
    // so we can safely select the first response for now
    let responses = &result.choices[0];
    let chat_response: String = responses
        .message
        .content
        .clone()
        .expect("no response from chat model");
    Ok(chat_response)
}

fn call_ollama_chat_completions(prompts: RenderedPrompt, model: &str) -> Result<String> {
    // get url from guc
    let url = match guc::get_guc(guc::VectorizeGuc::OllamaServiceUrl) {
        Some(k) => k,
        None => {
            error!("failed to get Ollama url from GUC");
        }
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap_or_else(|e| error!("failed to initialize tokio runtime: {}", e));

    let instance = OllamaInstance::new(model.to_string(), url.to_string());

    let response = runtime.block_on(async {
        instance
            .generate_reponse(prompts.sys_rendered + "\n" + &prompts.user_rendered)
            .await
    });

    match response {
        Ok(k) => Ok(k),
        Err(k) => {
            error!("Unable to generate response. Error: {k}");
        }
    }
}

// Trims the context to fit within the token limit when force_trim = True
// Otherwise returns an error if the context exceeds the token limit
fn trim_context(context: &str, overage: i32, bpe: &CoreBPE) -> Result<String> {
    // we separate each contextual result with a newline, which adds 1 token

    let tokens = bpe.split_by_token(context, false)?;
    let token_ct = tokens.len() as i32;

    let to_index = token_ct - overage;

    if to_index < 0 {
        let err_msg = format!(
            "prompt template exceeds context limit: {} > {}",
            token_ct,
            token_ct - overage
        );
        return Err(anyhow!(err_msg));
    }

    // naively trimming the context
    let trimmed_context: String = tokens
        .iter()
        .take(to_index as usize)
        .cloned()
        .collect::<String>();
    Ok(trimmed_context)
}

// handles all preparation of prompt with context
// optionally rims the context to fit within the token limit
fn prepared_prompt(
    searches: &[ContextualSearch],
    sys_prompt_template: &str,
    user_prompt_template: &str,
    query: &str,
    force_trim: bool,
    bpe: &CoreBPE,
    max_context_length: i32,
) -> Result<RenderedPrompt> {
    let combined_string = searches
        .iter()
        .map(|s| s.content.as_str())
        .collect::<Vec<&str>>()
        .join("\n\n");

    let user_message = render_user_message(user_prompt_template, &combined_string, query)?;

    // get the token count of the user message
    let user_message_ct = bpe.encode_ordinary(&user_message).len() as i32;

    let sys_prompt_token_ct = bpe.encode_ordinary(sys_prompt_template).len() as i32;
    let user_prompt_token_ct = bpe.encode_ordinary(user_prompt_template).len() as i32;

    let remaining_tokens = max_context_length - sys_prompt_token_ct - user_prompt_token_ct;

    // overage
    let overage = user_message_ct >= remaining_tokens;
    if overage && !force_trim {
        let err_msg = format!(
            "context exceeds limit: {} > {}",
            user_message_ct, remaining_tokens
        );
        return Err(anyhow!(err_msg));
    }
    if !overage {
        return Ok(RenderedPrompt {
            sys_rendered: sys_prompt_template.to_string(),
            user_rendered: user_message,
        });
    }

    let overage_amt = user_message_ct - remaining_tokens;

    // there is an overage in context
    let trimmed_context = trim_context(&combined_string, overage_amt, bpe)?;

    let user_message = render_user_message(user_prompt_template, &trimmed_context, query)?;

    Ok(RenderedPrompt {
        sys_rendered: sys_prompt_template.to_string(),
        user_rendered: user_message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepared_prompt() {
        let bpe = get_bpe_from_model("gpt-3.5-turbo").unwrap();
        let sys_prompt_template = "You are a sky expert";
        let user_prompt_template = "Here is context: {{context_str}} \nQuestion: {{query_str}}";
        let query = "What color is the sky?";
        let searches = vec![ContextualSearch {
            record_id: "1".to_string(),
            content: "The sky is the color blue.".to_string(),
            token_ct: 7,
        }];
        let rendered = prepared_prompt(
            &searches,
            sys_prompt_template,
            user_prompt_template,
            query,
            true,
            &bpe,
            36,
        )
        .unwrap();
        assert_eq!(rendered.sys_rendered, sys_prompt_template);
        // content must be trimmed to fit within the token limit when force_trim = True
        assert_eq!(
            rendered.user_rendered,
            "Here is context: The sky is \nQuestion: What color is the sky?"
        );
        // error when force_trim = False and context exceeds token limit
        let rendered = prepared_prompt(
            &searches,
            sys_prompt_template,
            user_prompt_template,
            query,
            false,
            &bpe,
            36,
        );
        assert!(rendered.is_err());
        // no trim when length is within token limit
        let rendered = prepared_prompt(
            &searches,
            sys_prompt_template,
            user_prompt_template,
            query,
            false,
            &bpe,
            1000,
        )
        .expect("failed to prepare prompt");
        assert_eq!(
            rendered.user_rendered,
            "Here is context: The sky is the color blue. \nQuestion: What color is the sky?"
        );

        // no trim when length is within token limit, and force_trim = True
        let rendered = prepared_prompt(
            &searches,
            sys_prompt_template,
            user_prompt_template,
            query,
            true,
            &bpe,
            1000,
        )
        .expect("failed to prepare prompt");
        assert_eq!(
            rendered.user_rendered,
            "Here is context: The sky is the color blue. \nQuestion: What color is the sky?"
        );
    }

    #[test]
    fn test_trim_context() {
        let bpe = get_bpe_from_model("gpt-3.5-turbo").unwrap();
        let context = "The sky is the color blue.";

        let overage = 1;
        let trimmed = trim_context(context, overage, &bpe).unwrap();
        assert_eq!("The sky is the color blue", trimmed);

        let overage = 2;
        let trimmed = trim_context(context, overage, &bpe).unwrap();
        assert_eq!("The sky is the color", trimmed);

        let overage = 5;
        let trimmed = trim_context(context, overage, &bpe).unwrap();
        assert_eq!("The sky", trimmed);
    }

    #[test]
    fn test_render_user_message() {
        let prompt_template =
            "You are a sky expert, and here is context: {{context_str}} Question: {{query_str}}";
        let context = "The sky is the color blue.";
        let query = "What color is the sky?";
        let rendered = render_user_message(prompt_template, context, query).unwrap();
        assert_eq!("You are a sky expert, and here is context: The sky is the color blue. Question: What color is the sky?", rendered);
    }
}
