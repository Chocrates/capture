## ADDED Requirements

### Requirement: A list is readable and completable in the window

The window SHALL offer the items written under a tag, gathered from every
capture that wrote them, and SHALL allow each to be completed in place.

A collection is the one surface whose purpose is working through something, and
it was the one surface the window did not have. The note page draws a single
capture's items, which answers a different question: a grocery list is spread
over however many times somebody thought of something.

Completion SHALL be by position within the capture the item came from, never by
its words. Two items reading the same are ordinary on a shopping list and their
words cannot tell them apart.

A read of a list SHALL therefore answer which capture each line came from. A
shape that omits it cannot support completion at all, whatever it draws.

#### Scenario: The items under a tag are shown
- **WHEN** a tag has items written under it by more than one capture
- **THEN** the window SHALL show all of them under that tag

#### Scenario: An item is completed from the list
- **WHEN** an item is ticked in the list view
- **THEN** it SHALL be recorded complete against the capture that wrote it
- **AND** the list SHALL show it complete

#### Scenario: Two items reading the same are told apart
- **WHEN** two captures under one tag wrote the same words
- **THEN** ticking one SHALL leave the other untouched

#### Scenario: A collection says it is one
- **WHEN** a list gathers items from more than one capture
- **THEN** the window SHALL say so rather than presenting them as one list

### Requirement: A window command that nothing can reach is not shipped

Every command the window registers SHALL be reachable from the window, and every
command the window calls SHALL exist.

This is §8.4 applied to the boundary rather than to the feature: a command
registered and never called is a capability the window does not have, and a
command called and never registered fails only when somebody presses the
control — reported to them as the store refusing rather than as a fault in the
page.

The check SHALL be mechanical. Both sides are enumerable — a command is an
annotated function, it reaches the page only if the handler list names it, and
the page calls it by a literal — so this needs no call graph and SHALL NOT
depend on one. A command name the page computes rather than writes SHALL be a
failure, because it makes the check blind.

#### Scenario: A registered command nothing calls
- **WHEN** the window registers a command the page never calls
- **THEN** the check SHALL fail, naming it

#### Scenario: A command the page calls that does not exist
- **WHEN** the page calls a command name that is not registered
- **THEN** the check SHALL fail, naming it

#### Scenario: A computed command name
- **WHEN** the page builds a command name instead of writing it
- **THEN** the check SHALL fail, because it can no longer see what is called
