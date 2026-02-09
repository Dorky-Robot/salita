-- Cleanup Duplicate Nodes Migration
-- Removes old auto-generated nodes that are not in current_node table

-- Delete any mesh_nodes that have is_current=1 but are NOT in current_node
-- This cleans up the old "Salita Node" entry from migration 005
DELETE FROM mesh_nodes
WHERE is_current = 1
  AND id NOT IN (SELECT node_id FROM current_node);

-- Clean up the is_current flag (no longer needed since we have current_node table)
-- We'll leave the column for now to avoid breaking existing queries
