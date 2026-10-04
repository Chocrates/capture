## ADDED Requirements

### Requirement: A near miss is offered where a suggestion is offered

A surface SHALL show a near miss wherever it shows a recognition it was not
confident enough to act on, and SHALL allow it to be accepted.

Accepting SHALL write an ordinary edit that inserts what was missing. The system
SHALL NOT make that edit on its own: captured text is kept verbatim (§3.1.3),
and a correction to it is a thing a person does.

The offer SHALL say what will change, because it rewrites the text of a capture
rather than adding a promotion beside it — which is a different kind of act from
accepting a type, and the surface should not make them look the same.

#### Scenario: Accepting a missing colon
- **WHEN** a capture missing its colon is shown
- **THEN** the offer SHALL name what will be inserted
- **AND** accepting SHALL make the capture a list

#### Scenario: Declining changes nothing
- **WHEN** a near miss is offered and not accepted
- **THEN** the capture SHALL be unchanged
- **AND** the offer SHALL still be available later

#### Scenario: Once it is a list there is nothing to offer
- **WHEN** a near miss has been accepted
- **THEN** no near miss SHALL be offered for that capture
