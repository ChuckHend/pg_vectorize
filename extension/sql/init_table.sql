-- Function to initialize a table for vector search
CREATE OR REPLACE FUNCTION vectorize.table(
    relation text,
    job_name text,
    columns text[],
    primary_key text,
    transformer text DEFAULT 'sentence-transformers/all-MiniLM-L6-v2',
    schema text DEFAULT 'public',
    update_col text DEFAULT NULL,
    index_dist_type text DEFAULT 'pgv_hnsw_cosine',
    table_method text DEFAULT 'join',
    schedule text DEFAULT 'realtime'  -- Default to daily at midnight
) RETURNS text AS $$
DECLARE
    update_time_dtype text;
    pkey_type text;
    model_dim integer;
    valid_params jsonb;
    trigger_handler text;
    insert_trigger text;
    update_trigger text;
BEGIN
    -- Validate table method
    IF schedule = 'realtime' AND table_method != 'join' THEN
        RAISE EXCEPTION 'realtime schedule is only compatible with the join table method';
    END IF;

    -- Validate update_col if provided
    IF update_col IS NOT NULL THEN
        SELECT data_type INTO update_time_dtype
        FROM information_schema.columns
        WHERE table_schema = schema
        AND relation = relation
        AND column_name = update_col;

        IF update_time_dtype != 'timestamp with time zone' THEN
            RAISE EXCEPTION 'update_col must be of type timestamp with time zone. column: % is of type %', update_col, update_time_dtype;
        END IF;
    END IF;

    -- Get primary key type
    SELECT data_type INTO pkey_type
    FROM information_schema.columns
    WHERE table_schema = schema
    AND relation = relation
    AND column_name = primary_key;

    -- Initialize pgmq if not already initialized
    PERFORM pgmq.create('vectorize_jobs');

    -- Get model dimension based on transformer
    -- Note: This is a placeholder - you'll need to implement the actual logic
    -- to get the model dimension based on your transformer configuration
    model_dim := 1536; -- Default for OpenAI models

    -- Create valid params JSON
    valid_params := jsonb_build_object(
        'schema', schema,
        'relation', relation,
        'columns', columns,
        'update_time_col', update_col,
        'table_method', table_method,
        'primary_key', primary_key,
        'pkey_type', pkey_type,
        'schedule', schedule
    );

    -- Insert job record
    INSERT INTO vectorize.job (name, index_dist_type, transformer, params)
    VALUES (job_name, index_dist_type, transformer, valid_params);

    -- Create embedding table based on table method
    IF table_method = 'join' THEN
        -- Create embeddings table for join method
        EXECUTE format('
            CREATE TABLE IF NOT EXISTS vectorize._embeddings_%I (
                %I %s,
                embeddings vector(%s),
                updated_at timestamptz DEFAULT now()
            )',
            job_name,
            primary_key,
            pkey_type,
            model_dim
        );

        -- Create index on embeddings
        EXECUTE format('
            CREATE INDEX IF NOT EXISTS _embeddings_%I_idx ON vectorize._embeddings_%I USING hnsw (embeddings vector_cosine_ops)',
            job_name,
            job_name
        );
    ELSE
        -- For append method, add embedding column to existing table
        EXECUTE format('
            ALTER TABLE %I.%I 
            ADD COLUMN IF NOT EXISTS %I_embeddings vector(%s),
            ADD COLUMN IF NOT EXISTS %I_updated_at timestamptz DEFAULT now()',
            schema,
            relation,
            job_name,
            model_dim,
            job_name
        );

        -- Create index on embeddings
        EXECUTE format('
            CREATE INDEX IF NOT EXISTS %I_embeddings_idx ON %I.%I USING hnsw (%I_embeddings vector_cosine_ops)',
            job_name,
            schema,
            relation,
            job_name
        );
    END IF;

    -- Set up triggers or cron based on schedule
    IF schedule = 'realtime' THEN
        -- Create trigger handler function
        EXECUTE format('
            CREATE OR REPLACE FUNCTION vectorize._%I_trigger_handler()
            RETURNS trigger AS $trigger$
            BEGIN
                PERFORM vectorize._handle_table_update(
                    %L,
                    ARRAY[NEW.%I::text]
                );
                RETURN NEW;
            END;
            $trigger$ LANGUAGE plpgsql',
            job_name,
            job_name,
            primary_key
        );

        -- Create insert trigger
        EXECUTE format('
            CREATE TRIGGER %I_insert_trigger
            AFTER INSERT ON %I.%I
            FOR EACH ROW
            EXECUTE FUNCTION vectorize._%I_trigger_handler()',
            job_name,
            schema,
            relation,
            job_name
        );

        -- Create update trigger
        EXECUTE format('
            CREATE TRIGGER %I_update_trigger
            AFTER UPDATE ON %I.%I
            FOR EACH ROW
            EXECUTE FUNCTION vectorize._%I_trigger_handler()',
            job_name,
            schema,
            relation,
            job_name
        );
    ELSE
        -- Initialize cron job
        -- Note: This assumes you have pg_cron extension installed
        EXECUTE format('
            SELECT cron.schedule(
                %L,
                %L,
                %L
            )',
            job_name,
            schedule,
            format('SELECT vectorize._handle_table_update(%L, ARRAY(SELECT %I::text FROM %I.%I))',
                job_name,
                primary_key,
                schema,
                relation
            )
        );
    END IF;

    -- Initialize first batch load
    -- This will be handled by the background worker
    -- The worker will pick up the initial job from the queue

    RETURN format('Successfully created job: %s', job_name);
END;
$$ LANGUAGE plpgsql;


CREATE OR REPLACE FUNCTION vectorize.batch_texts(
    record_ids TEXT[],
    batch_size INTEGER
) 
RETURNS TABLE(batch TEXT[])
LANGUAGE plpgsql
AS $$
DECLARE
    total_records INTEGER;
    num_batches INTEGER;
    i INTEGER;
    start_idx INTEGER;
    end_idx INTEGER;
    current_batch TEXT[];
BEGIN
    total_records := array_length(record_ids, 1);
    
    -- Handle edge cases
    IF batch_size <= 0 OR total_records IS NULL OR total_records <= batch_size THEN
        batch := record_ids;
        RETURN NEXT;
        RETURN;
    END IF;
    
    num_batches := (total_records + batch_size - 1) / batch_size;
    
    FOR i IN 0..(num_batches - 1) LOOP
        start_idx := i * batch_size + 1;  -- PostgreSQL arrays are 1-indexed
        end_idx := LEAST(start_idx + batch_size - 1, total_records);
        
        current_batch := record_ids[start_idx:end_idx];
        batch := current_batch;
        RETURN NEXT;
    END LOOP;
    
    RETURN;
END;
$$;