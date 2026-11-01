# Editable Taichuy calendar page template

One page contains one JSX block explicitly composing `CalendarProvider`, `Calendar`,
`Almanac`, `BirthInfo`, `ZiWei`, and `Bazi`. Edit its JSX to change columns, tabs,
component order or logic; there is no full-page black-box component in the template.

Install, assign and enable `taichuy-calendar@0.1.6` first. Use the existing application-template
preview/import/install flow. Installation creates a new page and remaps the plugin installation
ID; it does not overwrite the current calendar page. The package contains definitions and source,
not user data, sessions or credentials. The JSX editor's page-template chooser stores reusable
code templates; this directory contains the separate portable full-page template format.

After editing the plugin's `examples/composed-page.tsx`, run `node scripts/build-page-template.mjs`
to regenerate shards and ZIP. Source availability is not a signed official cloud catalog release;
publication follows the existing applications-demo release workflow.

[中文](README.md)
