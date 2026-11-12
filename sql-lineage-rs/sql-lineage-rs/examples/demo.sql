-- Example 1: map dependencies of a reporting view.
CREATE VIEW reporting.customer_revenue AS
SELECT c.customer_id, SUM(o.amount) AS total_amount
FROM raw.orders AS o
JOIN crm.customers AS c ON c.customer_id = o.customer_id
GROUP BY c.customer_id;

-- Example 2: a common table expression is NOT a physical source table.
WITH recent_orders AS (
  SELECT order_id, customer_id FROM raw.orders WHERE year = 2026
)
INSERT INTO mart.active_customers
SELECT customer_id FROM recent_orders JOIN crm.customers USING (customer_id);

-- Example 3: exploring the sources of an ad hoc query.
SELECT * FROM "finance"."invoices";
