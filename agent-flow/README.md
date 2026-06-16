# Official AgentFlow Workflows

This directory stores official AgentFlow workflow templates and the generated
catalog consumed by 1flowbase.

## Layout

- `workflows/<workflow_id>/template.json`: exported AgentFlow template package.
- `catalog/v1/index.json`: generated catalog entry point.
- `catalog/v1/pages/<page>.json`: generated catalog pages, 100 entries per page.
- `_maintenance/catalog-state.json`: generated scanner state.

`workflow_id` is the workflow directory name. Catalog entries are generated
from `template.json` and expose only the template schema version, application
metadata, template URL, template hash, and update time. Do not edit generated
catalog or maintenance files by hand.
