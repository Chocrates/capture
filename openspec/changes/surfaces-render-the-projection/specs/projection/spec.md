## ADDED Requirements

### Requirement: The rendering a surface draws comes from the projection

The rendering of a promotion SHALL be supplied by the projection rather than
composed by each surface. A surface SHALL NOT format fields into a description
of its own.

Two surfaces composing the same fields independently would be two renderings
that can disagree, and the disagreement would be invisible — each looks correct
on its own screen. This is the same argument that puts the descriptor in the
projection rather than computing it per view.

#### Scenario: One rendering, every surface
- **WHEN** a promoted note is drawn on two surfaces
- **THEN** both show the same rendering
- **AND** neither composed it from the fields itself

#### Scenario: The rendering is available without reading the text
- **WHEN** a surface asks for a note
- **THEN** the promotion's kind and its rendering are answered with it
