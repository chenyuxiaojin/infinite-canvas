-- Run only after backing up the database. Preserve every prompt and title.
BEGIN IMMEDIATE;
UPDATE prompts
SET tags = '[' || replace(json_quote(tags), ',', '","') || ']'
WHERE id IN (
  'p-case4-jack', 'p-case4-jack-scar', 'p-case4-george',
  'p-case4-gladys-1982', 'p-case4-gladys-1989',
  'p-case4-scene-berlin', 'p-case4-scene-pub', 'p-case4-scene-wall',
  'p-case4-prop-umbrella', 'p-case4-prop-radio'
) AND NOT json_valid(tags)
RETURNING id;
COMMIT;
