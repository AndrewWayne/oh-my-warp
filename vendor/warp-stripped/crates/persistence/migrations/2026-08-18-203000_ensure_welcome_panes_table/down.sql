-- This repair migration may not have created the table. Never remove data
-- owned by the historical migration when rolling this migration back.
SELECT 1;
