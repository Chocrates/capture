## ADDED Requirements

### Requirement: Pages are written in a full editor on every surface

The phone and the desktop SHALL each offer a multi-line editor for a note's
text, with completion after `[[`, `@`, `^` and `#` drawn from one store read.
Saving SHALL write a new version and keep the old one. Leaving the phone's
editor with unsaved changes SHALL ask before discarding them.

#### Scenario: Linking without retyping a slug
- **WHEN** "[[car" is typed in the editor
- **THEN** pages whose names begin "car" are offered
- **AND** choosing one writes its name and closes the brackets

### Requirement: A page is drawn in the capture grammar

A page SHALL be drawn with its line breaks, with links, people, projects and
tags followable, and with checklist lines as tickable items.

#### Scenario: A link on the phone
- **WHEN** a page containing "[[car-maintenance]]" is open on the phone
- **THEN** tapping the link opens that page
