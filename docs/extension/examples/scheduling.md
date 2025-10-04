# Scheduling Embedding Updates

When the source text data is updated, how and when the embeddings are updated is determined by the value set to the `schedule` parameter in `vectorize.table`s.

The default behavior is `schedule => '* * * * *'`, which means the background worker process checks for changes every minute, and updates the embeddings accordingly. This method requires setting the `updated_at_col` value to point to a colum on the table indicating the time that the input text columns were last changed. `schedule` can be set to any cron-like value.

Alternatively, `schedule => 'realtime` creates triggers on the source table and updates embeddings anytime new records are inserted to the source table or existing records are updated.

Statements below would will result in new embeddings being generated either immediately (`schedule => 'realtime'`) or within the cron schedule set in the `schedule` parameter.

```sql
INSERT INTO products (product_id, product_name, description, product_category, price)
VALUES (12345, 'pizza', 'dish of Italian origin consisting of a flattened disk of bread', 'food', 5.99);

UPDATE products
SET description = 'sling made of fabric, rope, or netting, suspended between two or more points, used for swinging, sleeping, or resting'
WHERE product_name = 'Hammock';
```
