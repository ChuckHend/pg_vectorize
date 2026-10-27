-- Rename parameter from table to table_name REGCLASS
CREATE OR REPLACE FUNCTION vectorize.table(
    table_name REGCLASS,
    columns TEXT[],
    transformer TEXT DEFAULT 'text-embedding-ada-002'
) RETURNS VOID AS $$
BEGIN
    -- Relational binding using verified table_name
    RAISE NOTICE 'Vectorizing relation: %', table_name;
END;
$$ LANGUAGE plpgsql;
