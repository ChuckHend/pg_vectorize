CREATE TABLE vectorize.job (
    job_id bigserial,
    name TEXT NOT NULL UNIQUE,
    index_dist_type TEXT NOT NULL DEFAULT 'pgv_hsnw_cosine',
    transformer TEXT NOT NULL,
    params jsonb NOT NULL
);

CREATE TABLE vectorize.prompts (
    prompt_type TEXT NOT NULL UNIQUE,
    sys_prompt TEXT NOT NULL,
    user_prompt TEXT NOT NULL
);

-- allow pg_monitor to read from vectorize schema
GRANT USAGE ON SCHEMA vectorize TO pg_monitor;
GRANT SELECT ON ALL TABLES IN SCHEMA vectorize TO pg_monitor;
GRANT SELECT ON ALL SEQUENCES IN SCHEMA vectorize TO pg_monitor;
ALTER DEFAULT PRIVILEGES IN SCHEMA vectorize GRANT SELECT ON TABLES TO pg_monitor;
ALTER DEFAULT PRIVILEGES IN SCHEMA vectorize GRANT SELECT ON SEQUENCES TO pg_monitor;

CREATE FUNCTION vectorize.handle_table_drop()
RETURNS event_trigger AS $$
DECLARE
    obj RECORD;
    schema_name TEXT;
    table_name TEXT;
BEGIN
    FOR obj IN SELECT * FROM pg_event_trigger_dropped_objects() LOOP
        IF obj.object_type = 'table' THEN
            schema_name := split_part(obj.object_identity, '.', 1);  
            table_name := split_part(obj.object_identity, '.', 2);  
            
            -- Perform cleanup: delete the associated job from the vectorize.job table
            DELETE FROM vectorize.job
            WHERE params ->> 'relation' = table_name
            AND params ->> 'schema' = schema_name;
        END IF;
    END LOOP;
END;
$$ LANGUAGE plpgsql;

DROP EVENT TRIGGER IF EXISTS vectorize_job_drop_trigger;

CREATE EVENT TRIGGER vectorize_job_drop_trigger
ON sql_drop
WHEN TAG IN ('DROP TABLE')
EXECUTE FUNCTION handle_table_drop();

INSERT INTO vectorize.prompts (prompt_type, sys_prompt, user_prompt)
VALUES (
    'question_answer',
    'You are an expert Q&A system.\nYou must always answer the question using the provided context information. Never use any prior knowledge.\nAdditional rules to follow:\n1. Never directly reference the given context in your answer.\n2. Never use responses like ''Based on the context, ...'' or ''The context information ...'' or any responses similar to that.',
    'Context information is below.\n---------------------\n{{ context_str }}\n---------------------\nGiven the context information and not prior knowledge, answer the query.\n Query: {{ query_str }}\nAnswer: '
)
ON CONFLICT (prompt_type)
DO NOTHING;

--- called by the trigger function when a table is updated
--- handles enqueueing the embedding transform jobs
CREATE OR REPLACE FUNCTION vectorize._handle_table_update(
    job_name text,
    record_ids text[]
) RETURNS void AS $$
DECLARE
    batch_size integer;
    batch_result RECORD;
    job_messages jsonb[] := '{}';
BEGIN
    -- create jobs of size batch_size
    batch_size := current_setting('vectorize.batch_size')::integer;
    FOR batch_result IN SELECT batch FROM vectorize.batch_texts(record_ids, batch_size) LOOP
        -- only append non-null, non-empty batches
        IF array_length(batch_result.batch, 1) > 0 THEN
            job_messages := array_append(
                job_messages,
                jsonb_build_object(
                    'job_name', job_name,
                    'record_ids', batch_result.batch
                )
            );
        END IF;
    END LOOP;

    PERFORM pgmq.send_batch(
        queue_name=>'vectorize_jobs'::text,
        msgs=>job_messages::jsonb[])
    ;

END;
$$ LANGUAGE plpgsql;