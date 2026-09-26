-- Per-job batch size: the maximum number of rows per queued message, and so
-- per embedding request. Changed through PATCH /api/v1/table/{job_name}.
ALTER TABLE vectorize.job ADD COLUMN IF NOT EXISTS batch_size INTEGER NOT NULL DEFAULT 1000;

-- Existing jobs' triggers still read the vectorize.batch_size setting, so
-- record what that setting is now instead of claiming the 1000 default.
UPDATE vectorize.job
SET batch_size = COALESCE(NULLIF(current_setting('vectorize.batch_size', true), '')::integer, 1000);

-- Triggers for jobs created or updated from this version on pass their batch
-- size in. The two-argument version is kept for triggers created earlier.
CREATE OR REPLACE FUNCTION vectorize._handle_table_update(
    job_name text,
    record_ids text[],
    batch_size integer
) RETURNS void AS $$
DECLARE
    batch_result RECORD;
    job_messages jsonb[] := '{}';
BEGIN
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
